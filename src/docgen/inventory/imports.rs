//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;

/// 解析一个文件的 import/use/mod，返回仓库内被依赖文件的相对路径。
pub fn resolve_imports(lang: Lang, rel: &str, text: &str, root: &Path) -> Vec<String> {
    let mut out = BTreeSet::new();
    let dir = Path::new(rel).parent().unwrap_or_else(|| Path::new(""));
    let crate_prefix = cargo_crate_name(root)
        .map(|n| format!("{n}::"))
        .unwrap_or_else(|| "crate::".to_string());

    let candidate = |base: &Path| -> Option<String> {
        let abs = root.join(base);
        if abs.is_file() {
            return Some(norm(base));
        }
        for ext in ["py", "ts", "tsx", "js", "jsx", "mjs", "go", "rs"] {
            let p = abs.with_extension(ext);
            if p.is_file() {
                return Some(norm(p.strip_prefix(root).ok()?));
            }
        }
        for index in ["index.ts", "index.js", "index.tsx", "mod.rs"] {
            let p = abs.join(index);
            if p.is_file() {
                return Some(norm(p.strip_prefix(root).ok()?));
            }
        }
        let init = abs.join("__init__.py");
        if init.is_file() {
            return Some(norm(init.strip_prefix(root).ok()?));
        }
        None
    };

    for raw in text.lines() {
        let line = raw.trim();
        match lang {
            Lang::Python => {
                let target = if let Some(rest) = line.strip_prefix("from ") {
                    rest.split_whitespace().next()
                } else if let Some(rest) = line.strip_prefix("import ") {
                    rest.split([',', ' ']).next()
                } else {
                    None
                };
                if let Some(module) = target {
                    if module.starts_with('.') {
                        let depth = module.chars().take_while(|c| *c == '.').count();
                        let rest = module.trim_start_matches('.');
                        let mut base = dir.to_path_buf();
                        for _ in 1..depth {
                            base = base.parent().unwrap_or(Path::new("")).to_path_buf();
                        }
                        let joined = base.join(rest.replace('.', "/"));
                        if let Some(p) = candidate(&joined) {
                            out.insert(p);
                        }
                    } else {
                        let joined = PathBuf::from(module.replace('.', "/"));
                        if let Some(p) = candidate(&joined) {
                            out.insert(p);
                        }
                    }
                }
            }
            Lang::TsJs => {
                // 自审 critical（unit-034 修正）：旧实现 needle "from '" 含引号，
                // pos+6 落在引号**后**——引号判定恒错：单引号 import 全部解析成带
                // `';` 尾巴的垃圾 spec（整条边丢失）；模块路径以多字节字符开头时
                // rest[1..] 切在字符中间直接 panic（门 0 崩溃）。现在先找 from，
                // 再从引号本身开始切，rest[0] 恒为 ASCII 引号，切屑安全。
                let rest = if let Some(pos) = line.find("from '") {
                    Some(&line[pos + 5..])
                } else if let Some(pos) = line.find("from \"") {
                    Some(&line[pos + 5..])
                } else if let Some(pos) = line.find("from ") {
                    Some(line[pos + 5..].trim_start())
                } else {
                    None
                };
                if let Some(rest) = rest {
                    match rest.chars().next() {
                        Some(q @ ('\'' | '"')) => {
                            let end = rest[1..].find(q).map(|i| i + 1).unwrap_or(rest.len());
                            let spec = rest[..end].trim_matches(q).trim();
                            add_relative(&mut out, dir, spec, &candidate);
                        }
                        _ => {} // 非字符串字面量形态（import x = require 等由 require 分支接）
                    }
                } else if let Some(pos) = line.find("require(") {
                    let rest = &line[pos + 8..];
                    let spec: String = rest
                        .trim_start()
                        .trim_start_matches(['\'', '"'])
                        .chars()
                        .take_while(|c| *c != '\'' && *c != '"' && *c != ')')
                        .collect();
                    add_relative(&mut out, dir, &spec, &candidate);
                }
            }
            Lang::Go => {
                if let Some(rest) = line.strip_prefix("import ") {
                    let spec = rest.trim_matches(['"', '(', ')', ' ']);
                    if let Some(p) = resolve_by_suffix(root, spec) {
                        out.insert(p);
                    }
                } else if line.starts_with('"')
                    && line.ends_with('"')
                    && let Some(p) = resolve_by_suffix(root, line.trim_matches('"'))
                {
                    out.insert(p);
                }
            }
            Lang::Rust => {
                let rest = line.strip_prefix("pub ").unwrap_or(line);
                if let Some(after) = rest.strip_prefix("use ") {
                    let target = after
                        .strip_prefix("crate::")
                        .or_else(|| after.strip_prefix(crate_prefix.as_str()));
                    if let Some(target) = target {
                        let path: String = target
                            .split(|c: char| c == ';' || c == '{' || c.is_whitespace())
                            .next()
                            .unwrap_or("")
                            .replace("::", "/");
                        let base = Path::new("src").join(&path);
                        if let Some(p) = candidate(&base) {
                            out.insert(p);
                        }
                    }
                } else if let Some(after) = rest.strip_prefix("mod ") {
                    let name = after.trim_end_matches(';').trim();
                    let base = dir.join(name);
                    if let Some(p) = candidate(&base) {
                        out.insert(p);
                    }
                }
            }
            Lang::Other => {}
        }
    }
    out.into_iter().collect()
}

fn add_relative<F>(out: &mut BTreeSet<String>, dir: &Path, spec: &str, candidate: &F)
where
    F: Fn(&Path) -> Option<String>,
{
    if (spec.starts_with("./") || spec.starts_with("../"))
        && let Some(p) = candidate(&dir.join(spec))
    {
        out.insert(p);
    }
}

/// Rust crate 名（Cargo.toml 的 `name`，`-` → `_`）；无则 None。
fn cargo_crate_name(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let name = rest.trim().trim_matches('"');
                if !name.is_empty() {
                    return Some(name.replace('-', "_"));
                }
            }
        }
    }
    None
}

fn resolve_by_suffix(root: &Path, spec: &str) -> Option<String> {
    let tail = spec.rsplit('/').next().unwrap_or(spec);
    let mut stack = vec![root.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                stack.push(path);
            } else if path.file_stem().and_then(|s| s.to_str()) == Some(tail) {
                let rel = path.strip_prefix(root).ok()?;
                return Some(norm(rel));
            }
        }
    }
    None
}

pub(super) fn norm(p: &Path) -> String {
    // 自审 critical：必须词法归一 `.`/`..` 分量——相对 import 解出的 base 带 `./`
    //（dir.join("./helper") = "src/./helper"），不归一则与盘扫路径永不相等，
    // TS/JS 相对依赖的边全部悬空（图连不起来）。
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    parts.join("/")
}

#[cfg(test)]
#[path = "imports_tests.rs"]
mod tests;

