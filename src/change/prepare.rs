//! change run 的门 0 预检（P009 接线）：一次把「基线 / 文档档位 / change-id / 黑板目录」备好。
//!
//! 顺序有依赖：change-id 决定计划目录 → 计划目录决定 baseline.txt 落点 → 基线又是 coding 的 diff 锚点。
//! 全部是确定性动作，可单测（不碰 agent、不跑 pi）。

use std::path::{Path, PathBuf};

use crate::change::change_id::{assign_change_id, date_segment};
use crate::change::docs::{self, DocAction};
use crate::change::gate0::{ensure_baseline, Baseline, DocLevel};
use crate::error::{DevforgeError, Result};

/// 门 0 准备好的上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePrep {
    pub change_id: String,
    /// 黑板：`plans/<project>/<task>/change`。
    pub blackboard: PathBuf,
    /// 计划目录（台账根 `plantree/<project>/changes/<change-id>`；2026-10-25 起不在目标仓内）。
    pub change_dir: PathBuf,
    pub baseline: Baseline,
    pub doc_level: DocLevel,
    /// 文档动作结论（回调方据此决定是否先跑 doc-from-zero）。
    pub doc_action: DocAction,
}

/// 默认台账根（2026-10-25 反转 D003：plan tree 不进仓）：
/// `DEVFORGE_HOME`（未设时 `$HOME/.devforge`）下的 `plantree/<project>`。
/// 仓内守卫测试用它定位 devforge 自己的台账（旧写法是 `env!("CARGO_MANIFEST_DIR")/docs/plantree/…`）。
pub fn plantree_root(plans_root: &Path, project: &str) -> PathBuf {
    plans_root
        .parent()
        .map(|p| p.join("plantree"))
        .unwrap_or_else(|| PathBuf::from("plantree"))
        .join(project)
}

/// 从环境推默认台账根（无 plans_root 的调用方/测试用）。
pub fn default_plantree_root(project: &str) -> PathBuf {
    let root = std::env::var_os("DEVFORGE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".devforge")))
        .unwrap_or_else(|| PathBuf::from(".devforge"));
    root.join("plantree").join(project)
}

/// 预检：不创建 agent，只把门 0 的确定性部分做完。
///
/// `today` 用 `(年, 月, 日)` 传入，便于测试固定日期。计划目录由 [`plantree_root`]
/// 从 `plans_root` 同源推出（root/plantree/<project>/changes/<id>），不在目标仓内。
pub fn prepare(
    plans_root: &Path,
    repo: &Path,
    project: &str,
    task: &str,
    requirement: &str,
    doc_level: &str,
    today: (i32, u32, u32),
) -> Result<ChangePrep> {
    if !repo.is_dir() {
        return Err(DevforgeError::Config(format!(
            "目标仓不存在：{}",
            repo.display()
        )));
    }
    // 自审 major：公开 API 入口补名字安检（域层已有现成校验但此前未被调用）——
    // project/task 直接拼进 plans_root 路径并建目录写文件。
    crate::domain::store::Store::validate_path_name(project)?;
    crate::domain::store::Store::validate_path_name(task)?;
    let level = DocLevel::parse_strict(doc_level)?;
    let date = date_segment(today.0, today.1, today.2);

    // 1) change-id（幂等）与黑板目录（SOP 级：状态/证据/实施步骤都在这）
    let (change_id, _id_dir) = assign_change_id(plans_root, project, task, requirement, &date)?;
    let blackboard = crate::change::change_id::change_root(plans_root, project, task);
    // 计划目录在**台账根**（2026-10-25 反转 D003：台账不进仓——可审查可回查，
    // 但不进目标仓 diff/提交；旧行为是 repo/docs/plantree/changes/<id>）
    let plan_dir = plantree_root(plans_root, project).join("changes").join(&change_id);
    std::fs::create_dir_all(&plan_dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", plan_dir.display())))?;

    // 2) 文档档位（complete/detailed 需要先跑 doc-from-zero，本层不假装自己写完了文档）
    let doc_action = docs::apply(repo, level)?;
    // 自审 minor（63）：归因文案带真实档位——旧口径把 detailed 也说成 complete
    if let DocAction::FullChainRequired(required) = doc_action {
        return Err(DevforgeError::Quality(format!(
            "doc_level={} 需要先跑 doc-from-zero 生成文档，change SOP 不代跑（调用方决定）",
            required.as_str()
        )));
    }

    // 3) 基线（无 .git 则 init + 全量提交；sha 落到台账根下的计划目录）
    let baseline = ensure_baseline(repo, &plan_dir)?;

    // 4) 黑板骨架（impl-steps / 证据 / 审查报告都在这里，agent 直接写）
    for sub in ["evidence", "testing/reports", "coding/reports"] {
        let dir = blackboard.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", dir.display())))?;
    }

    Ok(ChangePrep {
        change_id,
        blackboard,
        change_dir: plan_dir,
        baseline,
        doc_level: level,
        doc_action,
    })
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
