//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;

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
                // 自审 major：读失败不再静默——基础设施故障伪装成「文件不可达」
                // 的质量结论，可达性审计从此有暗洞。
                tracing::warn!(
                    component = "doc-engine",
                    path = %abs.display(),
                    "闭包 BFS 跳过不可读文件（可达性判定可能失真）"
                );
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

/// 词表导出（D011）：清单即词表，`glossary.md` 只是它的自动视图（不手写）。
pub fn export_glossary(inv: &Inventory) -> String {
    let mut out = String::from("# 词表（由 inventory 自动导出，勿手改）\n\n");
    let mut by_unit: BTreeMap<String, Vec<&InventoryEntry>> = BTreeMap::new();
    for e in &inv.entries {
        if e.kind == EntryKind::Symbol {
            by_unit.entry(e.unit_id.clone()).or_default().push(e);
        }
    }
    for (unit, mut items) in by_unit {
        items.sort_by(|a, b| a.canonical_name.cmp(&b.canonical_name));
        let title = if unit.is_empty() { "未分组" } else { &unit };
        out.push_str(&format!("## {title}\n\n"));
        for e in items {
            let alias = if e.aliases.is_empty() {
                String::new()
            } else {
                format!("（别名：{}）", e.aliases.join(", "))
            };
            out.push_str(&format!(
                "- `{}` — `{}:{}`{alias}\n",
                e.canonical_name, e.path, e.line
            ));
        }
        out.push('\n');
    }
    if !inv.meta.unsupported_languages.is_empty() {
        out.push_str(&format!(
            "## 未支持符号抽取的语言（缺口）\n\n{}\n",
            inv.meta.unsupported_languages.join(", ")
        ));
    }
    out
}
