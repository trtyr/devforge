//! 全量代码审计 SOP（P013）：对任意项目整仓审计——非 PR/diff。
//!
//! 设计：`docs/plantree/plans/013-code-review-audit/`（五阶段：盘点→切片→
//! 四阶段审查→核验→汇总分级）。M0 范围：盘点（复用 docgen inventory）+
//! 语义连贯切片器 + 骨架接线（run 走到切片完成即收）。
//!
//! 阶段（roadmap 定稿）：
//! ```text
//! Stage 0 规划（元数据→ReviewPlan）→ Stage 1 批次审查（综合六视角）→
//! Stage 2 跨单元分析 → Stage 3 汇总分级
//! ```

use std::path::Path;

use crate::error::{Context as _, Result};

pub mod finding;
pub mod pipeline;
pub mod report;
pub mod runner;
pub mod verify;

/// 一个审查单元：语义连贯的文件组（≤`MAX_UNIT_FILES`），一次批次审查的消费单位。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AuditUnit {
    /// 单元号（`unit-001` 起顺排）。
    pub id: String,
    /// 单元内源码文件（仓库相对路径）。
    pub files: Vec<String>,
    /// 人类可读标签（聚合依据：inventory 单元名或目录名）。
    pub label: String,
}

/// 切片产物：全部审查单元 + 覆盖对账计数（全仓审计 M6）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AuditPlan {
    pub units: Vec<AuditUnit>,
    /// 盘点到并进入切片的源文件数（去重后）。
    pub source_files: usize,
    /// 未进切片的条目数（Config/Data/Behavior 等非源码面）。
    pub excluded_entries: usize,
}

/// 每单元文件数上限（CodeCrow：批次按文件数限不按 token——≤7 可预测）。
pub const MAX_UNIT_FILES: usize = 7;

/// 每单元源码行数上限（第三方 r2 M3）：文件数上限挡不住同目录多个大文件——
/// 本仓 src/audit 六个源文件（含 1073 行 runner.rs）会整包成一批 ≈2300 行，
/// 超出单会话能可靠审查的量。超限按顺序切批；单文件自身超限独占一批（文件不可再分）。
pub const MAX_UNIT_LINES: usize = 800;

fn line_count(workspace: &Path, rel: &str) -> usize {
    std::fs::read_to_string(workspace.join(rel))
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

/// 按双预算顺序切批（不重排：同批文件语义仍连贯）。
fn split_by_budget(workspace: &Path, files: &[String]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_lines = 0usize;
    for f in files {
        let n = line_count(workspace, f);
        let full = cur.len() >= MAX_UNIT_FILES
            || (!cur.is_empty() && cur_lines + n > MAX_UNIT_LINES);
        if full {
            out.push(std::mem::take(&mut cur));
            cur_lines = 0;
        }
        cur.push(f.clone());
        cur_lines += n;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 盘点 + 切片（M0 核心，确定性零 LLM）：inventory 盘仓 → 按既有 unit_id /
/// 目录聚类 → 超限单元再二分。
pub fn build_plan(workspace: &Path) -> Result<AuditPlan> {
    let inventory = crate::docgen::inventory::build_inventory(workspace)?;

    // 聚类键：inventory 预分配的 unit_id 优先（门 0 已按模块分好），无则按父目录。
    let mut groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // 全仓审计 M6：非源码面条目（Config/Data/Behavior）不再静默丢弃——计数进报告
    let mut excluded_entries = 0usize;
    for e in &inventory.entries {
        if e.kind != crate::docgen::inventory::EntryKind::Module
            && e.kind != crate::docgen::inventory::EntryKind::Symbol
        {
            excluded_entries += 1;
            continue; // 只切源码面（Config/Data/Behavior 条目不是切片对象）
        }
        if !seen.insert(e.path.clone()) {
            continue;
        }
        let key = if e.unit_id.is_empty() {
            let dir = Path::new(&e.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());
            format!("dir:{dir}")
        } else {
            // unit 内再按目录分：docgen 的小仓 unit 启发式会把整个 src 给同一 unit，
            // 直接用会跨目录混批（审计批次要语义连贯 = 同目录/同模块）
            let dir = Path::new(&e.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());
            format!("unit:{}:{dir}", e.unit_id)
        };
        groups.entry(key).or_default().push(e.path.clone());
    }

    // 超限切分：文件数（MAX_UNIT_FILES）+ 行数（MAX_UNIT_LINES）双预算——
    // 只按文件数挡不住「同目录多个大文件」（第三方 r2 M3）。
    let mut units: Vec<AuditUnit> = Vec::new();
    let mut seq = 0usize;
    for (key, mut files) in groups {
        files.sort();
        let label = key
            .split_once(':')
            .map(|(_, v)| v.to_string())
            .unwrap_or(key.clone());
        // 审计修复 #10：分片标签用组内局部序号（全局 seq 当分子会出现「分片 4/2」）
        let chunks = split_by_budget(workspace, &files);
        let n_chunks = chunks.len();
        for (chunk_i, chunk) in chunks.into_iter().enumerate() {
            seq += 1;
            units.push(AuditUnit {
                id: format!("unit-{seq:03}"),
                files: chunk,
                label: format!(
                    "{label}{}",
                    if n_chunks > 1 {
                        format!("（分片 {}/{})", chunk_i + 1, n_chunks)
                    } else {
                        String::new()
                    }
                ),
            });
        }
    }
    units.sort_by(|a, b| a.id.cmp(&b.id));
    // 重排 id 保证连续（二分插入后序号乱）
    for (i, u) in units.iter_mut().enumerate() {
        u.id = format!("unit-{:03}", i + 1);
    }
    Ok(AuditPlan {
        units,
        source_files: seen.len(),
        excluded_entries,
    })
}

/// 批次文件清单落黑板（第三方 r2 m7）：报告只有 coverage 计数，没有「哪批审了哪些文件」，
/// 分母不可重建。run_audit 在切片后调用（旧的 run_skeleton 只在 M0 骨架期用过，生产无调用方，已删）。
pub fn write_units_file(blackboard: &Path, plan: &AuditPlan) -> Result<()> {
    let path = blackboard.join("audit-units.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(plan).ctx("序列化审计单元")?,
    )
    .ctx(format!("写 {}", path.display()))?;
    tracing::info!(
        component = "audit-engine",
        units = plan.units.len(),
        files = plan.units.iter().map(|u| u.files.len()).sum::<usize>(),
        "全量审计切片完成"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具：8 个源码文件分两个目录（模拟两个模块）。
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (i, dir) in ["src/alpha", "src/beta"].iter().enumerate() {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
            for j in 1..=4 {
                std::fs::write(
                    tmp.path().join(format!("{dir}/m{j}.rs")),
                    format!("// module {i}-{j}\npub fn f{i}_{j}() {{}}\n"),
                )
                .unwrap();
            }
        }
        tmp
    }

    #[test]
    fn slice_groups_by_directory_and_caps_unit_size() {
        let r = repo();
        let plan = build_plan(r.path()).unwrap();
        // 8 文件 / 上限 7 → 至少 2 个单元；alpha 与 beta 不混在同一个 7 文件单元里
        assert!(plan.units.len() >= 2, "应至少切成两单元：{plan:?}");
        let all: Vec<&String> = plan.units.iter().flat_map(|u| u.files.iter()).collect();
        assert_eq!(all.len(), 8, "全部源码文件都应被切片覆盖");
        for u in &plan.units {
            assert!(u.files.len() <= MAX_UNIT_FILES, "单元超限：{u:?}");
            let dirs: std::collections::BTreeSet<String> = u.files.iter().map(|f| {
                Path::new(f).parent().unwrap().to_string_lossy().to_string()
            }).collect();
            assert_eq!(dirs.len(), 1, "同单元不应跨目录（M0 目录聚合语义）：{u:?}");
        }
    }

    /// 第三方 r2 M3：同目录大文件不得整包进同一批——行数预算必须生效。
    #[test]
    fn line_budget_splits_same_directory_big_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/audit")).unwrap();
        // 三个各 600 行的同目录文件：总 1800 行 > MAX_UNIT_LINES(800) → 必切 ≥3 批
        for i in 1..=3 {
            let body: String = (0..600).map(|n| format!("// line {n}\n")).collect();
            std::fs::write(
                tmp.path().join(format!("src/audit/m{i}.rs")),
                format!("{body}pub fn f{i}() {{}}\n"),
            )
            .unwrap();
        }
        let plan = build_plan(tmp.path()).unwrap();
        assert!(
            plan.units.len() >= 3,
            "行数预算未生效（应 ≥3 批）：{:?}",
            plan.units
                .iter()
                .map(|u| u.files.clone())
                .collect::<Vec<_>>()
        );
        for u in &plan.units {
            let lines: usize = u.files.iter().map(|f| line_count(tmp.path(), f)).sum();
            assert!(
                lines <= MAX_UNIT_LINES || u.files.len() == 1,
                "单元行数超限且非单文件：{} 行 {u:?}",
                lines
            );
        }
    }

    /// 第三方 r3 M3：V5 依赖 codegraph——必须强制带 projectPath（本机多仓索引共存时
    /// 不带路径的 explore 可能返回别的仓；官方设计就是每调用传 projectPath）。
    /// 同时钉住 C1 的「只报读过的代码」（它是审查官侧唯一的行为约束）。
    #[test]
    fn prompt_pins_codegraph_project_path_and_read_first() {
        let prompt = include_str!("../../assets/prompts/code-review-agent.md");
        assert!(
            prompt.contains("projectPath"),
            "V5 必须要求 codegraph 调用带 projectPath"
        );
        assert!(
            prompt.contains("没打开过文件就不报"),
            "「只报读过的代码」约束不得被删"
        );
        assert!(
            prompt.contains("代码原文"),
            "证据必须引原文（核验器逐字比对）也要写进提示词"
        );
    }

    #[test]
    fn slice_is_deterministic() {
        let r = repo();
        let a = build_plan(r.path()).unwrap();
        let b = build_plan(r.path()).unwrap();
        assert_eq!(a, b, "同输入切片必须确定性一致");
    }

    #[test]
    fn non_source_entries_are_not_sliced() {
        // Behavior/Config 条目（docgen 也会收）不进切片面
        let r = repo();
        let plan = build_plan(r.path()).unwrap();
        let all: Vec<&String> = plan.units.iter().flat_map(|u| u.files.iter()).collect();
        assert!(
            all.iter().all(|f| f.ends_with(".rs")),
            "切片只含源码：{all:?}"
        );
    }
}
