//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;

/// 从磁盘机械构建清单（启发式 v1）。
pub fn build_inventory(root: &Path) -> Result<Inventory> {    let root = root
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
                for (line, name) in config_keys(*lang, &text) {
                    entries.push(InventoryEntry {
                        id: format!("cfg:{rel}:{name}"),
                        kind: EntryKind::Config,
                        path: rel.clone(),
                        line,
                        canonical_name: name,
                        aliases: Vec::new(),
                        status: Coverage::Uncovered,
                        unit_id: String::new(),
                    });
                }
                // T003：测试即可执行规格（只抽测试名与位置）。
                // Rust 的测试通常写在源文件内的 `#[test]` 块里，所以不按文件名卡。
                if (is_test_file(rel) || *lang == Lang::Rust)
                    && let Some(lang) = Lang::from_path(Path::new(rel))
                {
                    for (line, name) in test_specs(lang, &text) {
                        entries.push(InventoryEntry {
                            id: format!("spec:{rel}:{name}"),
                            kind: EntryKind::Behavior,
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

    for rel in scan_support_files(&root)? {
        let abs = root.join(&rel);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            continue;
        };
        let file = Path::new(&rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let mut push = |kind: EntryKind, line: usize, name: String, prefix: &str| {
            entries.push(InventoryEntry {
                id: format!("{prefix}:{rel}:{name}"),
                kind,
                path: rel.clone(),
                line,
                canonical_name: name,
                aliases: Vec::new(),
                status: Coverage::Uncovered,
                unit_id: String::new(),
            });
        };
        if file.ends_with(".env.example") || file.ends_with(".env.sample") {
            for (line, name) in env_file_keys(&text) {
                push(EntryKind::Config, line, name, "cfg");
            }
        }
        if file.ends_with(".sql") {
            for (line, name) in sql_objects(&text) {
                push(EntryKind::Data, line, name, "data");
            }
        }
        if file == "Makefile" || file == "makefile" {
            for (line, name) in make_targets(&text) {
                push(EntryKind::Behavior, line, name, "cmd");
            }
        }
        if file == "package.json" {
            for (line, name) in package_scripts(&text) {
                push(EntryKind::Behavior, line, name, "cmd");
            }
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
    // assets/ 下是随技能发布的模板与样例（vendored），不是产品代码；
    // 算进分母会把文档任务拖成写别人代码的说明书（2026-10-20 自测实践）
    "assets",
];

/// 扫描时跳过的目录（隐藏目录 + 依赖/产物目录 + 文档目录）。
pub(super) fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name) || name.ends_with(".venv")
}

/// 扫描源文件（跳过隐藏目录与构建/依赖目录），返回相对路径 + 语言。
pub fn scan_source_files(root: &Path) -> Result<Vec<(String, Lang)>> {
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
                if is_skipped_dir(&name) {
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

/// 单元粒度上限：一个单元最多包含多少模块。
///
/// 为什么需要：单元原来按顶层目录分组，大仓的 `src/` 再大也只是一个单元——
/// 滚动、检查点、逐单元审查全部退化成“一次大活”，跟不分单元的差别只剩文件名。
pub(super) const MAX_UNIT_MODULES: usize = 8;

/// 单元编号预分配（D020）：按顶层目录分组（过大的组再按路径切片），编号一次分配、重复调用不漂移。
pub fn assign_units(inv: &mut Inventory) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, e) in inv.entries.iter().enumerate() {
        let grouped = matches!(
            e.kind,
            EntryKind::Module | EntryKind::Config | EntryKind::Data | EntryKind::Behavior
        );
        if !grouped {
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
        // 大组切片：排序后每 MAX_UNIT_MODULES 个一组（确定性切分，同一批文件多次调用结果一致）
        let mut members: Vec<usize> = idxs.clone();
        members.sort_by(|a, b| inv.entries[*a].path.cmp(&inv.entries[*b].path));
        let chunks: Vec<Vec<usize>> = members.chunks(MAX_UNIT_MODULES).map(|c| c.to_vec()).collect();
        for chunk in chunks {
            let existing = chunk
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
            for i in chunk {
                inv.entries[i].unit_id = unit.clone();
            }
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
        if !matches!(e.kind, EntryKind::Module)
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
