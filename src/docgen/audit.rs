//! 确定性审计（P008 · D013/§阶段4）：覆盖对账 + 报告渲染。
//!
//! 分工：结构/实体/置信度/Mermaid 已在 doc-lint（`docgen::lint`）；
//! 本模块补「覆盖」——清单每一条 ↔ 文档是否有落点；并按 `doc_level` 决定要求哪些条目。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::docgen::inventory::{EntryKind, Inventory};
use crate::error::{Context as _, Result};

/// 文档档位（D007：默认 complete）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLevel {
    /// 只要入口点能跑能接
    Essential,
    /// + 模块行为语义、配置项、错误码、数据模型（默认）
    Complete,
    /// + 算法/状态机/权限矩阵（机械覆盖口径同 complete）
    Detailed,
}

impl DocLevel {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "essential" => Self::Essential,
            "detailed" => Self::Detailed,
            _ => Self::Complete,
        }
    }

    fn requires(self, kind: EntryKind) -> bool {
        match self {
            Self::Essential => kind == EntryKind::Entry,
            Self::Complete | Self::Detailed => matches!(
                kind,
                EntryKind::Entry | EntryKind::Module | EntryKind::Symbol
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoverageMiss {
    pub id: String,
    pub kind: EntryKind,
    pub canonical_name: String,
    pub path: String,
    pub line: usize,
    pub unit_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct CoverageReport {
    pub required: usize,
    pub covered: usize,
    pub missing: Vec<CoverageMiss>,
}

impl CoverageReport {
    pub fn passed(&self) -> bool {
        self.missing.is_empty()
    }

    /// 渲染给审查输入（确定性事实，不是建议）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "覆盖审计（引擎）：要求 {} 条 / 已覆盖 {} 条 / 缺失 {} 条\n",
            self.required,
            self.covered,
            self.missing.len()
        );
        for m in &self.missing {
            out.push_str(&format!(
                "- [{}] {} 未在文档中出现（{}:{}，单元 {}）\n",
                kind_str(m.kind),
                m.canonical_name,
                m.path,
                m.line,
                m.unit_id
            ));
        }
        if self.passed() {
            out.push_str("结论：覆盖 100%（该档位要求集合内）。\n");
        } else {
            out.push_str("结论：覆盖不完整——缺一条即零覆盖，必须补文档或登记缺口（D013）。\n");
        }
        out
    }
}

fn kind_str(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Entry => "entry",
        EntryKind::Module => "module",
        EntryKind::Symbol => "symbol",
        EntryKind::Config => "config",
        EntryKind::Data => "data",
        EntryKind::Behavior => "behavior",
    }
}

/// 覆盖对账：按档位取条目集合，逐条查是否在 docs 目录下被提及。
pub fn coverage(inv: &Inventory, docs_dir: &Path, level: DocLevel) -> Result<CoverageReport> {
    let corpus = collect_text(docs_dir)?;
    let mut report = CoverageReport::default();
    for e in &inv.entries {
        if !level.requires(e.kind) {
            continue;
        }
        report.required += 1;
        // 模块：认路径或模块名；符号：认规范名（大小写敏感，避免误判）
        let hit = match e.kind {
            EntryKind::Module => {
                corpus.contains(&e.path) || corpus.contains(&e.canonical_name)
            }
            _ => corpus.contains(&e.canonical_name),
        };
        if hit {
            report.covered += 1;
        } else {
            report.missing.push(CoverageMiss {
                id: e.id.clone(),
                kind: e.kind,
                canonical_name: e.canonical_name.clone(),
                path: e.path.clone(),
                line: e.line,
                unit_id: e.unit_id.clone(),
            });
        }
    }
    Ok(report)
}

/// 单单元覆盖（只算该单元的条目），供逐单元审查使用。
pub fn coverage_for_unit(
    inv: &Inventory,
    docs_dir: &Path,
    level: DocLevel,
    unit: &str,
) -> Result<CoverageReport> {
    let corpus = collect_text(docs_dir)?;
    let mut report = CoverageReport::default();
    for e in inv.entries.iter().filter(|e| e.unit_id == unit) {
        if !level.requires(e.kind) {
            continue;
        }
        report.required += 1;
        let hit = match e.kind {
            EntryKind::Module => corpus.contains(&e.path) || corpus.contains(&e.canonical_name),
            _ => corpus.contains(&e.canonical_name),
        };
        if hit {
            report.covered += 1;
        } else {
            report.missing.push(CoverageMiss {
                id: e.id.clone(),
                kind: e.kind,
                canonical_name: e.canonical_name.clone(),
                path: e.path.clone(),
                line: e.line,
                unit_id: e.unit_id.clone(),
            });
        }
    }
    Ok(report)
}

fn collect_text(dir: &Path) -> Result<String> {
    let mut out = String::new();
    let files = collect_markdown(dir)?;
    for f in files {
        out.push_str(&std::fs::read_to_string(&f).ctx(format!("读文档 {}", f.display()))?);
        out.push('\n');
    }
    Ok(out)
}

fn collect_markdown(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
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
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 抽取 Runbook 里的可执行命令块（bash/sh/console），供审查官“从零 boot”时逐条跑。
pub fn runbook_commands(docs_dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for f in collect_markdown(docs_dir)? {
        let is_runbook = f
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase().contains("runbook"))
            .unwrap_or(false);
        if !is_runbook {
            continue;
        }
        let text = std::fs::read_to_string(&f).ctx(format!("读 {}", f.display()))?;
        let mut cur: Vec<String> = Vec::new();
        let mut in_block = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("```") {
                if in_block {
                    if !cur.is_empty() {
                        out.push(cur.join("\n"));
                    }
                    cur.clear();
                    in_block = false;
                } else {
                    let info = t.trim_start_matches('`').trim();
                    in_block = matches!(info, "bash" | "sh" | "console" | "shell" | "zsh");
                }
                continue;
            }
            if in_block && !t.is_empty() && !t.starts_with('#') {
                cur.push(line.to_string());
            }
        }
    }
    Ok(out)
}

/// 未被任何文档提及的单元（供引擎在终审前定位返工范围）。
pub fn uncovered_units(report: &CoverageReport) -> BTreeSet<String> {
    report
        .missing
        .iter()
        .map(|m| m.unit_id.clone())
        .filter(|u| !u.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docgen::inventory::build_inventory;

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/main.py"),
            "from core import create_note\ncreate_note()\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("src/core.py"),
            "def create_note(title):\n    return title\n\ndef orphan_fn():\n    pass\n",
        )
        .unwrap();
        tmp
    }

    fn write_doc(repo: &Path, name: &str, body: &str) {
        let dir = repo.join("docs/contexts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn coverage_marks_missing_entries() {
        let tmp = fixture();
        write_doc(tmp.path(), "core.md", "# core\n\n`create_note` 写笔记。\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
        assert!(!rep.passed());
        assert!(rep.missing.iter().any(|m| m.canonical_name == "orphan_fn"));
        assert!(rep.render().contains("orphan_fn"));
    }

    #[test]
    fn full_coverage_requires_modules_and_symbols() {
        let tmp = fixture();
        write_doc(
            tmp.path(),
            "all.md",
            "# all\n\n`src/main.py` / `src/core.py`：`create_note` 与 `orphan_fn`。\n",
        );
        let inv = build_inventory(tmp.path()).unwrap();
        let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
        assert!(rep.passed(), "{:?}", rep.missing);
        assert!(rep.render().contains("覆盖 100%"));
    }

    #[test]
    fn essential_level_only_requires_entry_points() {
        let tmp = fixture();
        write_doc(tmp.path(), "entry.md", "# entry\n\n`src/main.py`\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Essential).unwrap();
        assert!(rep.passed(), "{:?}", rep.missing);
    }

    #[test]
    fn unit_scope_only_counts_own_entries() {
        let tmp = fixture();
        write_doc(tmp.path(), "core.md", "# core\n\n`create_note`\n");
        let inv = build_inventory(tmp.path()).unwrap();
        let unit = inv
            .entries
            .iter()
            .find(|e| e.canonical_name == "create_note")
            .map(|e| e.unit_id.clone())
            .unwrap();
        let rep =
            coverage_for_unit(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete, &unit)
                .unwrap();
        assert!(rep.required > 0);
        assert!(rep.missing.iter().all(|m| m.unit_id == unit));
    }

    #[test]
    fn uncovered_units_groups_misses() {
        let tmp = fixture();
        let inv = build_inventory(tmp.path()).unwrap();
        let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
        let units = uncovered_units(&rep);
        assert!(!units.is_empty());
        assert!(units.iter().all(|u| u.starts_with("unit-")));
    }

    #[test]
    fn runbook_commands_extracted_from_bash_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        write_doc(
            tmp.path(),
            "10-runbook.md",
            "# Runbook\n\n```bash\npip install -r requirements.txt\npython -m app\n```\n\n```text\n不是命令\n```\n",
        );
        let cmds = runbook_commands(&tmp.path().join("docs/contexts")).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].contains("pip install"));
        assert!(cmds[0].contains("python -m app"));
    }

    #[test]
    fn level_parse_defaults_to_complete() {
        assert_eq!(DocLevel::parse("whatever"), DocLevel::Complete);
        assert_eq!(DocLevel::parse("essential"), DocLevel::Essential);
        assert_eq!(DocLevel::parse("detailed"), DocLevel::Detailed);
    }
}
