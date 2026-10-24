//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;
use super::imports::norm;
use super::scanner::is_skipped_dir;

/// 约定面抽取（T002）：源代码里读的环境变量键 → 配置项条目。
pub fn config_keys(lang: Lang, text: &str) -> Vec<(usize, String)> {
    let markers: &[&str] = match lang {
        Lang::Python => &["os.environ.get(", "os.environ[", "os.getenv("],
        Lang::TsJs => &["process.env.", "process.env["],
        Lang::Go => &["os.Getenv(", "os.LookupEnv("],
        Lang::Rust => &["env::var(", "env::var_os("],
        Lang::Other => &[],
    };
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        for name in keys_after(line, markers) {
            out.push((idx + 1, name));
        }
    }
    out
}

/// 环境变量样板文件（.env.example / .env.sample）里的键。
pub fn env_file_keys(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let s = s.strip_prefix("export ").unwrap_or(s);
        if let Some((k, _)) = s.split_once('=') {
            let k = k.trim();
            if !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                out.push((idx + 1, k.to_string()));
            }
        }
    }
    out
}

/// 数据层（T002）：SQL DDL 里的表/视图名。
pub fn sql_objects(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let lower = line.trim().to_ascii_lowercase();
        let rest = match lower
            .strip_prefix("create table ")
            .or_else(|| lower.strip_prefix("create view "))
        {
            Some(r) => r,
            None => continue,
        };
        let rest = rest.trim_start_matches("if not exists ").trim();
        let name = rest
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or("")
            .trim_matches(|c| c == '"' || c == '`' || c == ';' || c == '\'');
        if !name.is_empty() {
            out.push((idx + 1, name.to_string()));
        }
    }
    out
}

/// 构建/启动命令（T002）：Makefile 目标。
pub fn make_targets(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.starts_with('\t') || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((head, _)) = line.split_once(':') else {
            continue;
        };
        let name = head.trim();
        if !name.is_empty()
            && !name.contains('=')
            && !name.contains(' ')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            out.push((idx + 1, name.to_string()));
        }
    }
    out
}

/// 构建/启动命令（T002）：package.json 的 scripts 名。
pub fn package_scripts(text: &str) -> Vec<(usize, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    scripts
        .keys()
        .map(|k| {
            let line = text
                .lines()
                .position(|l| l.contains(&format!("\"{k}\"")))
                .map(|i| i + 1)
                .unwrap_or(1);
            (line, k.clone())
        })
        .collect()
}

/// 从一行里抽标记后面的键（跳过空白与引号/括号，取到下一个引号/边界）。
fn keys_after(line: &str, markers: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for m in markers {
        let mut from = 0;
        while let Some(pos) = line[from..].find(m) {
            let start = from + pos + m.len();
            let mut s = line[start..].trim_start();
            for p in ["[\"", "['", "(\"", "('", "[", "(", "\"", "'"] {
                if let Some(r) = s.strip_prefix(p) {
                    s = r;
                    break;
                }
            }
            let end = s.find(['"', '\'', ')', ']', ',', ';', ' ', '}']).unwrap_or(s.len());
            let name = &s[..end];
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
                && !out.iter().any(|e| e == name)
            {
                out.push(name.to_string());
            }
            from = start;
        }
    }
    out
}

/// 约定面文件（不走源文件扫描的）：Makefile / package.json / env 样板 / SQL。
pub fn scan_support_files(root: &Path) -> Result<Vec<String>> {
    const NAMES: &[&str] = &["Makefile", "makefile", "package.json"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if is_skipped_dir(&name) {
                    continue;
                }
                stack.push(p);
                continue;
            }
            let name_is = NAMES.contains(&name.as_str())
                || name.ends_with(".sql")
                || name.ends_with(".env.example")
                || name.ends_with(".env.sample");
            if name_is
                && let Ok(stripped) = p.strip_prefix(root)
            {
                out.push(norm(stripped));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 测试文件判定：_test 后缀 / tests 目录 / .test./.spec. 中缀。
pub fn is_test_file(rel: &str) -> bool {
    let p = Path::new(rel);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = p
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    rel.split('/').any(|c| c == "tests" || c == "test" || c == "__tests__")
        || stem.ends_with("_test")
        || stem.starts_with("test_")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || name.ends_with("_test.go")
}

/// 测试即可执行规格（T003）：从测试反抽「行为条目」——只取名字与位置，不照拄断言。
///
/// 为什么只取名字：断言是实现细节的镜像，文档要的是「这条行为存在且可验证」，
/// 而它可验证这件事由 `path:line` 自己担保。
pub fn test_specs(lang: Lang, text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut prev_was_rust_attr = false;
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        match lang {
            Lang::Python => {
                if let Some(rest) = line.strip_prefix("async def ").or_else(|| line.strip_prefix("def "))
                    && let Some(name) = rest.split('(').next()
                    && name.starts_with("test")
                {
                    out.push((idx + 1, name.to_string()));
                }
            }
            Lang::TsJs => {
                for kw in ["it(", "test("] {
                    if let Some(rest) = line.strip_prefix(kw)
                        && let Some(q) = rest.trim_start().strip_prefix(['"', '\'', '`'])
                        && let Some(name) = q.split(['"', '\'', '`']).next()
                        && !name.is_empty()
                    {
                        out.push((idx + 1, name.to_string()));
                    }
                }
            }
            Lang::Go => {
                if let Some(rest) = line.strip_prefix("func Test")
                    && let Some(name) = rest.split('(').next()
                {
                    out.push((idx + 1, format!("Test{name}")));
                }
            }
            Lang::Rust => {
                if line == "#[test]" || line.starts_with("#[test ") {
                    prev_was_rust_attr = true;
                    continue;
                }
                if prev_was_rust_attr
                    && let Some(rest) = line.strip_prefix("fn ")
                    && let Some(name) = rest.split('(').next()
                {
                    out.push((idx + 1, name.to_string()));
                }
                prev_was_rust_attr = false;
            }
            Lang::Other => {}
        }
    }
    out
}
