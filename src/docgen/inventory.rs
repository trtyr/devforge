//! 清单（inventory）：反向写文档的"分母"。
//!
//! 三件事（D002/D013）：
//! 1. 机械抽取：源文件 → module 条目；公开符号 → symbol 条目；入口点 → entry 条目
//! 2. 三道闭包校验：入口可达 / 文件覆盖 / 符号覆盖（不过则覆盖率数字无意义）
//! 3. 单元编号预分配：门 0 一次性分配，局部重跑不漂移
//!
//! 本版为**启发式 v1**（无 tree-sitter 依赖）：按语言的行首特征抽公开符号，
//! 按 import/use/mod 行建文件级依赖。codegraph 索引作为增强来源由上层接入，
//! 结果记入 `meta.codegraph_indexed`，缺索引不阻塞（清单只是缺点精度，不是错的）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, Result};

/// 参与清单的源文件扩展名（D004：多语言；未覆盖语言登记为缺口）。
pub const SOURCE_EXTENSIONS: &[(&str, Lang)] = &[
    ("py", Lang::Python),
    ("ts", Lang::TsJs),
    ("tsx", Lang::TsJs),
    ("js", Lang::TsJs),
    ("jsx", Lang::TsJs),
    ("mjs", Lang::TsJs),
    ("go", Lang::Go),
    ("rs", Lang::Rust),
    ("java", Lang::Other),
    ("cs", Lang::Other),
    ("php", Lang::Other),
    ("rb", Lang::Other),
    ("c", Lang::Other),
    ("h", Lang::Other),
    ("cc", Lang::Other),
    ("cpp", Lang::Other),
    ("hpp", Lang::Other),
    ("kt", Lang::Other),
    ("swift", Lang::Other),
    ("scala", Lang::Other),
    ("dart", Lang::Other),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Python,
    TsJs,
    Go,
    Rust,
    /// 已知扩展名但不做符号抽取（D004：登记为缺口）
    Other,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        SOURCE_EXTENSIONS
            .iter()
            .find(|(e, _)| *e == ext)
            .map(|(_, l)| *l)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Entry,
    Module,
    Symbol,
    Config,
    Data,
    Behavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coverage {
    Uncovered,
    Covered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub id: String,
    pub kind: EntryKind,
    /// 相对仓库根的路径（统一 `/` 分隔，便于跨机比对）
    pub path: String,
    /// 1-based 行号；module 条目为 1
    #[serde(default = "one")]
    pub line: usize,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub status: Coverage,
    /// 门 0 预分配的单元号（D020），空 = 未分组
    #[serde(default)]
    pub unit_id: String,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryMeta {
    pub root: String,
    pub codegraph_indexed: bool,
    /// 未支持符号抽取的语言（D004 缺口登记）
    pub unsupported_languages: Vec<String>,
    /// 单元编号序列水位（预分配不漂移）
    pub unit_seq: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub meta: InventoryMeta,
    pub entries: Vec<InventoryEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closure {
    EntryReachability,
    FileCoverage,
    SymbolCoverage,
}

impl Closure {
    pub fn as_str(self) -> &'static str {
        match self {
            Closure::EntryReachability => "entry-reachability",
            Closure::FileCoverage => "file-coverage",
            Closure::SymbolCoverage => "symbol-coverage",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClosureFinding {
    pub closure: Closure,
    pub path: String,
    pub line: usize,
    pub message: String,
}

impl Inventory {
    pub fn entry_paths(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Module)
            .map(|e| e.path.clone())
            .collect()
    }

    pub fn symbol_names(&self) -> BTreeSet<(String, String)> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .map(|e| (e.path.clone(), e.canonical_name.clone()))
            .collect()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx(format!("建目录 {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).ctx(format!("写清单 {}", path.display()))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).ctx(format!("读清单 {}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    }
}

/// 从磁盘机械构建清单（启发式 v1）。
pub fn build_inventory(root: &Path) -> Result<Inventory> {
    let root = root
        .canonicalize()
        .ctx(format!("解析仓库根 {}", root.display()))?;
    let files = scan_source_files(&root)?;

    let mut unsupported = BTreeSet::new();
    let mut entries = Vec::new();

    for (rel, lang) in &files {
        entries.push(InventoryEntry {
            id: format!("mod:{rel}"),
            kind: EntryKind::Module,
            path: rel.clone(),
            line: 1,
            canonical_name: module_name(rel),
            aliases: Vec::new(),
            status: Coverage::Uncovered,
            unit_id: String::new(),
        });

        match lang {
            Lang::Other => {
                unsupported.insert(
                    Path::new(rel)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("?")
                        .to_string(),
                );
            }
            _ => {
                let abs = root.join(rel);
                let text = std::fs::read_to_string(&abs)
                    .ctx(format!("读取源文件 {}", abs.display()))?;
                for (line, name) in public_symbols(*lang, &text) {
                    entries.push(InventoryEntry {
                        id: format!("sym:{rel}:{name}"),
                        kind: EntryKind::Symbol,
                        path: rel.clone(),
                        line,
                        canonical_name: name,
                        aliases: Vec::new(),
                        status: Coverage::Uncovered,
                        unit_id: String::new(),
                    });
                }
            }
        }

        if is_entry_file(rel) {
            entries.push(InventoryEntry {
                id: format!("entry:{rel}"),
                kind: EntryKind::Entry,
                path: rel.clone(),
                line: 1,
                canonical_name: module_name(rel),
                aliases: Vec::new(),
                status: Coverage::Uncovered,
                unit_id: String::new(),
            });
        }
    }

    let mut inv = Inventory {
        meta: InventoryMeta {
            root: root.display().to_string(),
            codegraph_indexed: root.join(".codegraph").is_dir(),
            unsupported_languages: unsupported.into_iter().collect(),
            unit_seq: 0,
        },
        entries,
    };
    assign_units(&mut inv);
    Ok(inv)
}

/// 扫描源文件（跳过隐藏目录与构建/依赖目录），返回相对路径 + 语言。
pub fn scan_source_files(root: &Path) -> Result<Vec<(String, Lang)>> {
    const SKIP_DIRS: &[&str] = &[
        "node_modules",
        "target",
        "dist",
        "build",
        "out",
        "__pycache__",
        "venv",
        "env",
        "vendor",
        "docs",
    ];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let rd = match std::fs::read_dir(&cur) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) || name.ends_with(".venv") || name == ".venv" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Some(lang) = Lang::from_path(&path) else {
                continue;
            };
            if name.ends_with(".min.js") {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, lang));
        }
    }
    out.sort();
    Ok(out)
}

fn module_name(rel: &str) -> String {
    rel.trim_end_matches(|c: char| c != '/')
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(rel)
        .to_string()
}

/// 入口点文件判定（启发式：文件名/目录约定）。
pub fn is_entry_file(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    matches!(
        file,
        "main.py"
            | "__main__.py"
            | "app.py"
            | "server.py"
            | "manage.py"
            | "main.go"
            | "main.rs"
            | "lib.rs"
            | "index.ts"
            | "index.js"
            | "cli.py"
    ) || lower.starts_with("cmd/")
        || lower.starts_with("bin/")
        || lower.contains("/bin/")
}

/// 抽公开符号（行号 + 名称）。启发式：按语言的行首特征。
pub fn public_symbols(lang: Lang, text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        let line_no = idx + 1;
        match lang {
            Lang::Python => {
                let rest = line
                    .strip_prefix("async def ")
                    .or_else(|| line.strip_prefix("def "))
                    .or_else(|| line.strip_prefix("class "));
                if let Some(rest) = rest {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() && !name.starts_with('_') {
                        out.push((line_no, name));
                    }
                }
            }
            Lang::TsJs => {
                if let Some(rest) = line.strip_prefix("export ") {
                    let rest = rest
                        .strip_prefix("default ")
                        .unwrap_or(rest)
                        .strip_prefix("async ")
                        .unwrap_or(rest);
                    for kw in ["function ", "class ", "const ", "let ", "interface ", "type ", "enum "] {
                        if let Some(after) = rest.strip_prefix(kw) {
                            let name: String = after
                                .chars()
                                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                                .collect();
                            if !name.is_empty() {
                                out.push((line_no, name));
                            }
                            break;
                        }
                    }
                }
            }
            Lang::Go => {
                if let Some(rest) = line.strip_prefix("func ") {
                    let rest = if rest.starts_with('(') {
                        match rest.find(')') {
                            Some(close) => rest[close + 1..].trim_start(),
                            None => rest,
                        }
                    } else {
                        rest
                    };
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        out.push((line_no, name));
                    }
                }
            }
            Lang::Rust => {
                if let Some(rest) = line.strip_prefix("pub ") {
                    let rest = rest.strip_prefix("async ").unwrap_or(rest);
                    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
                    for kw in ["fn ", "struct ", "enum ", "trait ", "const ", "mod "] {
                        if let Some(after) = rest.strip_prefix(kw) {
                            let name: String = after
                                .chars()
                                .take_while(|c| c.is_alphanumeric() || *c == '_')
                                .collect();
                            if !name.is_empty() {
                                out.push((line_no, name));
                            }
                            break;
                        }
                    }
                }
            }
            Lang::Other => {}
        }
    }
    out
}

/// 单元编号预分配（D020）：按顶层目录分组，编号一次分配、重复调用不漂移。
pub fn assign_units(inv: &mut Inventory) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, e) in inv.entries.iter().enumerate() {
        if e.kind != EntryKind::Module {
            continue;
        }
        groups.entry(group_of(&e.path)).or_default().push(idx);
    }

    // 已有 unit_id 的分组保持不变；新分组从水位往后接
    let mut used: BTreeMap<String, u32> = BTreeMap::new();
    for idx in groups.values().flatten() {
        let uid = &inv.entries[*idx].unit_id;
        if let Some(num) = parse_unit_seq(uid) {
            used.insert(uid.clone(), num);
        }
    }
    let mut next = inv
        .meta
        .unit_seq
        .max(used.values().copied().max().unwrap_or(0));

    for (group, idxs) in &groups {
        let existing = idxs
            .iter()
            .map(|i| inv.entries[*i].unit_id.clone())
            .find(|u| !u.is_empty());
        let unit = match existing {
            Some(u) => u,
            None => {
                next += 1;
                let slug = sanitize(group);
                format!("unit-{slug}-{next:02}")
            }
        };
        for i in idxs {
            inv.entries[*i].unit_id = unit.clone();
        }
    }
    // symbol/entry 条目挂到所属文件单元
    let module_units: BTreeMap<String, String> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Module)
        .map(|e| (e.path.clone(), e.unit_id.clone()))
        .collect();
    for e in &mut inv.entries {
        if e.kind != EntryKind::Module
            && let Some(u) = module_units.get(&e.path)
        {
            e.unit_id = u.clone();
        }
    }
    inv.meta.unit_seq = next;
}

fn group_of(rel: &str) -> String {
    match rel.split_once('/') {
        Some((top, _)) => top.to_string(),
        None => "root".to_string(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn parse_unit_seq(uid: &str) -> Option<u32> {
    uid.rsplit('-').next().and_then(|s| s.parse::<u32>().ok())
}

/// 三道闭包校验（D002）：不过则分母无意义。
pub fn check_closures(root: &Path, inv: &Inventory) -> Result<Vec<ClosureFinding>> {
    let root = root
        .canonicalize()
        .ctx(format!("解析仓库根 {}", root.display()))?;
    let disk = scan_source_files(&root)?;
    let mut findings = Vec::new();

    // ① 文件覆盖：磁盘上的源文件必须都在清单里
    let inventory_files = inv.entry_paths();
    for (rel, _) in &disk {
        if !inventory_files.contains(rel) {
            findings.push(ClosureFinding {
                closure: Closure::FileCoverage,
                path: rel.clone(),
                line: 1,
                message: "源文件不在清单中（分母漏项）".into(),
            });
        }
    }

    // ② 符号覆盖：磁盘上抽得出的公开符号必须都在清单里
    let inventory_symbols = inv.symbol_names();
    for (rel, lang) in &disk {
        if *lang == Lang::Other {
            continue;
        }
        let text = std::fs::read_to_string(root.join(rel))
            .ctx(format!("读取源文件 {rel}"))?;
        for (line, name) in public_symbols(*lang, &text) {
            if !inventory_symbols.contains(&(rel.clone(), name.clone())) {
                findings.push(ClosureFinding {
                    closure: Closure::SymbolCoverage,
                    path: rel.clone(),
                    line,
                    message: format!("公开符号 {name} 不在清单中"),
                });
            }
        }
    }

    // ③ 入口可达：从入口文件沿 import 边 BFS，未覆盖的文件报出
    let entries: Vec<String> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Entry)
        .map(|e| e.path.clone())
        .collect();
    if entries.is_empty() {
        for (rel, _) in &disk {
            findings.push(ClosureFinding {
                closure: Closure::EntryReachability,
                path: rel.clone(),
                line: 1,
                message: "清单里没有入口点（入度 0 无法确认）".into(),
            });
        }
    } else {
        let mut reachable: BTreeSet<String> = entries.iter().cloned().collect();
        let mut queue: VecDeque<String> = entries.iter().cloned().collect();
        while let Some(cur) = queue.pop_front() {
            let abs = root.join(&cur);
            let Ok(text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let Some(lang) = Lang::from_path(Path::new(&cur)) else {
                continue;
            };
            for target in resolve_imports(lang, &cur, &text, &root) {
                if reachable.insert(target.clone()) {
                    queue.push_back(target);
                }
            }
        }
        for (rel, _) in &disk {
            if !reachable.contains(rel) {
                findings.push(ClosureFinding {
                    closure: Closure::EntryReachability,
                    path: rel.clone(),
                    line: 1,
                    message: "从任何入口点都不可达（可能是死代码或漏登入口）".into(),
                });
            }
        }
    }

    findings.sort_by(|a, b| {
        (a.closure.as_str(), &a.path, a.line).cmp(&(b.closure.as_str(), &b.path, b.line))
    });
    Ok(findings)
}

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
                if let Some(pos) = line.find("from '").or_else(|| line.find("from \"")) {
                    let rest = &line[pos + 6..];
                    let quote = if rest.starts_with('\'') { '\'' } else { '"' };
                    let end = rest[1..].find(quote).map(|i| i + 1).unwrap_or(rest.len());
                    let spec = &rest[..end].trim_matches(|c| c == ' ').trim_matches(quote);
                    add_relative(&mut out, dir, spec, &candidate);
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

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn build_inventory_collects_modules_symbols_and_entries() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "src/main.py", "def main():\n    pass\n");
        touch(tmp.path(), "src/core.py", "def helper():\n    pass\n\nclass Store:\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        assert!(inv.entries.iter().any(|e| e.kind == EntryKind::Entry && e.path == "src/main.py"));
        assert!(inv
            .entries
            .iter()
            .any(|e| e.kind == EntryKind::Symbol && e.canonical_name == "Store"));
        // 私有符号（_开头）不入清单
        assert!(!inv.entries.iter().any(|e| e.canonical_name == "_secret"));
    }

    #[test]
    fn private_symbols_are_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.py", "def _secret():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        assert!(inv
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .all(|e| !e.canonical_name.starts_with('_')));
    }

    #[test]
    fn other_languages_are_recorded_as_unsupported_gap() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "app.java", "public class App {}\n");
        let inv = build_inventory(tmp.path()).unwrap();
        assert!(inv.meta.unsupported_languages.contains(&"java".to_string()));
        assert!(inv.entries.iter().any(|e| e.path == "app.java"));
    }

    #[test]
    fn unit_numbering_is_stable_across_runs() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "core/a.py", "def a():\n    pass\n");
        touch(tmp.path(), "web/b.py", "def b():\n    pass\n");
        let mut inv = build_inventory(tmp.path()).unwrap();
        let first: Vec<(String, String)> = inv
            .entries
            .iter()
            .map(|e| (e.id.clone(), e.unit_id.clone()))
            .collect();
        assign_units(&mut inv);
        let second: Vec<(String, String)> = inv
            .entries
            .iter()
            .map(|e| (e.id.clone(), e.unit_id.clone()))
            .collect();
        assert_eq!(first, second, "重复分配不得漂移");
        // 符号继承所属文件单元
        let sym = inv.entries.iter().find(|e| e.kind == EntryKind::Symbol).unwrap();
        let module = inv
            .entries
            .iter()
            .find(|e| e.kind == EntryKind::Module && e.path == sym.path)
            .unwrap();
        assert_eq!(sym.unit_id, module.unit_id);
    }

    #[test]
    fn file_coverage_closure_catches_missing_module() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.py", "def a():\n    pass\n");
        touch(tmp.path(), "b.py", "def b():\n    pass\n");
        let mut inv = build_inventory(tmp.path()).unwrap();
        // 反向验证：删掉 b.py 的 module 条目
        inv.entries.retain(|e| !(e.kind == EntryKind::Module && e.path == "b.py"));
        let findings = check_closures(tmp.path(), &inv).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.closure == Closure::FileCoverage && f.path == "b.py"));
    }

    #[test]
    fn symbol_coverage_closure_catches_missing_symbol() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.py", "def a():\n    pass\n\ndef zzz():\n    pass\n");
        let mut inv = build_inventory(tmp.path()).unwrap();
        inv.entries
            .retain(|e| !(e.kind == EntryKind::Symbol && e.canonical_name == "zzz"));
        let findings = check_closures(tmp.path(), &inv).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.closure == Closure::SymbolCoverage && f.message.contains("zzz")));
    }

    #[test]
    fn entry_reachability_follows_python_imports() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "main.py", "from core import a\n\na()\n");
        touch(tmp.path(), "core.py", "def a():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let findings = check_closures(tmp.path(), &inv).unwrap();
        assert!(
            findings.iter().all(|f| f.closure != Closure::EntryReachability),
            "{findings:?}"
        );
    }

    #[test]
    fn unreachable_file_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "main.py", "print('hi')\n");
        touch(tmp.path(), "orphan.py", "def lonely():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let findings = check_closures(tmp.path(), &inv).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.closure == Closure::EntryReachability && f.path == "orphan.py"));
    }

    #[test]
    fn missing_entry_points_reports_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "lib.py", "def a():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let findings = check_closures(tmp.path(), &inv).unwrap();
        assert!(findings
            .iter()
            .any(|f| f.closure == Closure::EntryReachability && f.message.contains("没有入口点")));
    }

    #[test]
    fn inventory_roundtrip_json() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.py", "def a():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let path = tmp.path().join("_meta/inventory.json");
        inv.save(&path).unwrap();
        let back = Inventory::load(&path).unwrap();
        assert_eq!(back.entries.len(), inv.entries.len());
        assert_eq!(back.meta.unit_seq, inv.meta.unit_seq);
    }

    #[test]
    fn skips_vendor_and_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "node_modules/x.js", "export const x = 1;\n");
        touch(tmp.path(), ".hidden/y.py", "def y():\n    pass\n");
        touch(tmp.path(), "src/z.py", "def z():\n    pass\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let paths: Vec<&str> = inv.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/z.py"));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
        assert!(!paths.iter().any(|p| p.starts_with(".hidden")));
    }

    #[test]
    fn ts_export_symbols_detected() {
        let tmp = tempfile::tempdir().unwrap();
        touch(
            tmp.path(),
            "index.ts",
            "export function boot() {}\nexport class App {}\nconst internal = 1;\n",
        );
        let inv = build_inventory(tmp.path()).unwrap();
        let names: Vec<&str> = inv
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .map(|e| e.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"boot") && names.contains(&"App"));
        assert!(!names.contains(&"internal"));
    }

    #[test]
    fn rust_pub_symbols_detected() {
        let tmp = tempfile::tempdir().unwrap();
        touch(
            tmp.path(),
            "src/lib.rs",
            "pub fn run() {}\npub struct Cfg;\nfn private() {}\n",
        );
        let inv = build_inventory(tmp.path()).unwrap();
        let names: Vec<&str> = inv
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .map(|e| e.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"run") && names.contains(&"Cfg"));
        assert!(!names.contains(&"private"));
    }

    #[test]
    fn go_exported_symbols_detected() {
        let tmp = tempfile::tempdir().unwrap();
        touch(
            tmp.path(),
            "main.go",
            "package main\n\nfunc Run() {}\nfunc hidden() {}\nfunc (s *Srv) Serve() {}\n",
        );
        let inv = build_inventory(tmp.path()).unwrap();
        let names: Vec<&str> = inv
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .map(|e| e.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"Run") && names.contains(&"Serve"));
        assert!(!names.contains(&"hidden"));
    }
}
