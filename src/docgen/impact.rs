//! 增量模式的影响传播（008 · T023 / D015 / D021）。
//!
//! 只做机械可判定的事：从 git 变更集出发，算出「这次提交影响哪些单元」。
//! 语义精确度不做承诺——**宁可多算，不可漏算**（漏算 = 文档说假话）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Context as _, DevforgeError, Result};

use crate::docgen::inventory::{Inventory, build_inventory};

/// 一次变更里的单个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// `A` / `M` / `D` / `R`（重命名按新路径记，旧路径在 `old_path`）。
    pub kind: char,
    /// 相对仓库根的路径（统一 `/` 分隔）。
    pub path: String,
    /// 重命名前的路径。
    pub old_path: Option<String>,
}

/// 影响传播结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Impact {
    /// 需要重写的单元 id（已排序、去重）。
    pub units: Vec<String>,
    /// 需要复核的汇编页（文档文件名，如 `README.md`）。
    pub assembly_review: Vec<String>,
    /// 过程说明（降级原因、重建清单等），进日志与报告。
    pub notes: Vec<String>,
}

/// 汇编页（D016）：这几页汇总全仓，叶子一变就可能陈旧。
pub const ASSEMBLY_PAGES: [&str; 4] = [
    "README.md",
    "01-architecture.md",
    "02-dataflow.md",
    "10-runbook.md",
];

/// 扫描时跳过的目录（不是产品源码）。
const SKIP_DIRS: [&str; 8] = [
    "docs",
    ".git",
    "target",
    ".codegraph",
    "node_modules",
    "dist",
    "build",
    ".venv",
];

/// 解析 `git diff --name-status` 输出。
///
/// 形如：`M\tsrc/a.rs`、`A\tsrc/b.rs`、`D\tsrc/c.rs`、`R100\told.rs\tnew.rs`。
pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(kind_raw) = parts.next() else { continue };
        let kind = kind_raw.chars().next().unwrap_or(' ');
        match kind {
            'A' | 'M' | 'D' | 'T' => {
                if let Some(p) = parts.next() {
                    out.push(ChangedFile {
                        kind,
                        path: p.to_string(),
                        old_path: None,
                    });
                }
            }
            'R' | 'C' => {
                if let (Some(old), Some(new)) = (parts.next(), parts.next()) {
                    out.push(ChangedFile {
                        kind: 'R',
                        path: new.to_string(),
                        old_path: Some(old.to_string()),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// 解析 `git status --porcelain` 输出（未提交改动也算改了）。
pub fn parse_porcelain(text: &str) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }
        // 自审 major：diff --name-status 的重命名行是 `R100\told\tnew`（状态长度可变 +
        // TAB 分隔），旧实现按 porcelain 的 `XY old -> new` 硬切——重命名条目从来
        // 解析不出来（old_path 恒丢）。现在按首个空白切状态，rename 按两种分隔符都认。
        let (status, path) = match line.split_once('\t') {
            Some((s, p)) => (s.to_string(), p.to_string()),
            None => (line[..2].to_string(), line[3..].trim().to_string()),
        };
        if path.is_empty() {
            continue;
        }
        // 重命名/复制：diff 形态 `R100\told\tnew`；porcelain 形态 `R  old -> new`
        if (status.contains('R') || status.contains('C'))
            && let Some((old, new)) = path
                .split_once('\t')
                .or_else(|| path.split_once(" -> "))
        {
            out.push(ChangedFile {
                kind: 'R',
                path: new.to_string(),
                old_path: Some(old.to_string()),
            });
            continue;
        }
        let kind = if status.contains('A') || status.contains('?') {
            'A'
        } else if status.contains('D') {
            'D'
        } else {
            'M'
        };
        out.push(ChangedFile {
            kind,
            path: path.to_string(),
            old_path: None,
        });
    }
    out
}

/// 取 `base..HEAD` 的变更集，并并入未提交改动。
pub fn changed_since(repo: &Path, base: &str) -> Result<Vec<ChangedFile>> {
    let diff = run_git(
        repo,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-status",
            base,
            "HEAD",
        ],
    )?;
    // 自审 major：未加 -uall 时新目录里的未跟踪文件被折叠成 `?? dir/`，
    // 新文件的单元永远不会进影响面。展开到每个文件。
    let status = run_git(
        repo,
        &[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain",
            "-uall",
        ],
    )?;
    let mut seen: BTreeMap<String, ChangedFile> = BTreeMap::new();
    // 自审 major：porcelain 条目不再整体覆盖 diff 条目——同名时只补 old_path，
    // 否则「已提交的重命名 + 未提交的再修改」会把 R 的 old_path 丢掉。
    for f in parse_name_status(&diff).into_iter().chain(parse_porcelain(&status)) {
        match seen.get_mut(&f.path) {
            Some(existing) => {
                if existing.old_path.is_none() {
                    existing.old_path = f.old_path.clone();
                }
            }
            None => {
                seen.insert(f.path.clone(), f);
            }
        }
    }
    Ok(seen.into_values().collect())
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .ctx(format!("跑 git {} 失败（{}）", args.join(" "), repo.display()))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git {} 退出码非 0：{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 变更文件命中的单元。
pub fn units_touching(inv: &Inventory, files: &[ChangedFile]) -> BTreeSet<String> {
    let paths: BTreeSet<&str> = files
        .iter()
        .flat_map(|f| std::iter::once(f.path.as_str()).chain(f.old_path.as_deref()))
        .collect();
    inv.entries
        .iter()
        .filter(|e| paths.contains(e.path.as_str()))
        .map(|e| e.unit_id.clone())
        .collect()
}

/// 变更文件上「旧清单有、新清单没有」的符号名（删除或改名）。
pub fn vanished_names(old: &Inventory, new: &Inventory, files: &[ChangedFile]) -> BTreeSet<String> {
    let paths: BTreeSet<&str> = files
        .iter()
        .flat_map(|f| std::iter::once(f.path.as_str()).chain(f.old_path.as_deref()))
        .collect();
    let before: BTreeSet<&str> = old
        .entries
        .iter()
        .filter(|e| paths.contains(e.path.as_str()))
        .map(|e| e.canonical_name.as_str())
        .collect();
    let after: BTreeSet<&str> = new
        .entries
        .iter()
        .filter(|e| paths.contains(e.path.as_str()))
        .map(|e| e.canonical_name.as_str())
        .collect();
    before
        .difference(&after)
        .filter(|n| n.len() >= 2) // 单字符名误命中率太高，不含信息量
        .map(|n| n.to_string())
        .collect()
}

/// 全仓源码里引用了这些名字的文件（词边界匹配，跳过生成目录）。
pub fn reference_files(repo: &Path, names: &BTreeSet<String>) -> Result<Vec<String>> {
    let mut hits = BTreeSet::new();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    walk(repo, repo, &mut |rel, text| {
        let hit = names.iter().any(|n| contains_word(text, n));
        if hit {
            hits.insert(rel);
        }
        Ok(())
    })?;
    Ok(hits.into_iter().collect())
}

fn contains_word(hay: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let abs = start + pos;
        let before_ok = hay[..abs]
            .chars()
            .next_back()
            .map(|c| !is_word_char(c))
            .unwrap_or(true);
        let after = abs + needle.len();
        let after_ok = hay[after..]
            .chars()
            .next()
            .map(|c| !is_word_char(c))
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        start = abs + needle.len().max(1);
        if start >= hay.len() {
            break;
        }
    }
    false
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 遍历仓库里的文本源码文件。
fn walk(
    root: &Path,
    dir: &Path,
    f: &mut impl FnMut(String, &str) -> Result<()>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ctx(format!("读目录 {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') && name != "." {
            continue;
        }
        if p.is_dir() {
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            walk(root, &p, f)?;
            continue;
        }
        let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !is_source_ext(ext) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            // 自审 major：读失败/非 UTF-8 不再静默——漏算零线索比多算一条日志更贵。
            tracing::warn!(
                component = "doc-engine",
                path = %p.display(),
                "影响面扫描跳过不可读/非 UTF-8 文件"
            );
            continue;
        };
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        f(rel, &text)?;
    }
    Ok(())
}

fn is_source_ext(ext: &str) -> bool {
    // 自审 major：扩展名单以前在本文件与 inventory 双份维护且已漂移——
    // 现在单一事实源 = inventory::SOURCE_EXTENSIONS，扫描覆盖与清单覆盖天然对齐
    //（语义变化：inventory 不支持的语言不再进影响面扫描，它们本就进不了清单）。
    crate::docgen::inventory::SOURCE_EXTENSIONS
        .iter()
        .any(|(e, _)| *e == ext)
}

/// 汇总影响面。
pub fn impacted(
    repo: &Path,
    old_inv: &Inventory,
    new_inv: &Inventory,
    files: &[ChangedFile],
    docs_root: &Path,
) -> Result<Impact> {
    let mut notes = Vec::new();
    let mut units = units_touching(new_inv, files);
    // 自审 major：旧清单兜底改为**并集**而非「非空才回退」——混合变更下
    // 被删除文件的单元只存在于旧清单，「新命中非空」会把它整段漏掉。
    units.extend(units_touching(old_inv, files));

    let names = vanished_names(old_inv, new_inv, files);
    for rel in reference_files(repo, &names)? {
        let hit: BTreeSet<String> = new_inv
            .entries
            .iter()
            .filter(|e| e.path == rel)
            .map(|e| e.unit_id.clone())
            .collect();
        if hit.is_empty() {
            notes.push(format!("引用方 `{rel}` 不在清单里（可能是新文件或未支持语言）"));
        }
        units.extend(hit);
    }

    let assembly_review = assembly_pages_mentioning(docs_root, &units, &names);
    if !names.is_empty() {
        notes.push(format!("删除/改名的符号：{}", names.into_iter().collect::<Vec<_>>().join(", ")));
    }

    Ok(Impact {
        units: units.into_iter().collect(),
        assembly_review,
        notes,
    })
}

/// 汇编页里提到了受影响单元或消失符号 → 需要复核。
fn assembly_pages_mentioning(
    docs_root: &Path,
    units: &BTreeSet<String>,
    names: &BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    // 注意：单元 → 汇编页引用 token 的真实约定尚未锚定（docs 汇编页还未有真实产物），
    // 当前 slug 展开形态（`unit-src-01` → `src/01`）是占位实现；等 doc-from-zero 首次
    // 全量跑通后按真实页面引用形态重写（gaps.md 已登记）。
    let unit_tokens: Vec<String> = units
        .iter()
        .map(|u| u.trim_start_matches("unit-").replace('-', "/"))
        .collect();
    for page in ASSEMBLY_PAGES {
        let path = docs_root.join(page);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mentions_unit = unit_tokens.iter().any(|t| !t.is_empty() && text.contains(t));
        let mentions_name = names.iter().any(|n| contains_word(&text, n));
        if mentions_unit || mentions_name {
            out.push(page.to_string());
        }
    }
    out
}

/// 便捷入口：读旧清单（`docs/contexts/_meta/inventory.json`）并与现仓重建清单对照。
pub fn plan_impact(repo: &Path, docs_root: &Path, changes: &[ChangedFile]) -> Result<Impact> {
    let old_path = docs_root.join("_meta/inventory.json");
    let old = Inventory::load(&old_path)
        .ctx(format!("读旧清单 {} 失败（增量需要上次的清单做对照）", old_path.display()))?;
    let new = build_inventory(repo)?;
    let mut impact = impacted(repo, &old, &new, changes, docs_root)?;
    if impact.units.is_empty() && !changes.is_empty() {
        impact
            .notes
            .push("变更文件没有命中任何单元（可能是纯文档/配置改动），不派发单元".into());
    }
    Ok(impact)
}

#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;
