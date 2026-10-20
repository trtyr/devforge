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
    /// 计划目录（目标仓）：`docs/plantree/changes/<change-id>`。
    pub change_dir: PathBuf,
    pub baseline: Baseline,
    pub doc_level: DocLevel,
    /// 文档动作结论（回调方据此决定是否先跑 doc-from-zero）。
    pub doc_action: DocAction,
}

/// 预检：不创建 agent，只把门 0 的确定性部分做完。
///
/// `today` 用 `(年, 月, 日)` 传入，便于测试固定日期。
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
    let level = DocLevel::parse(doc_level)?;
    let date = date_segment(today.0, today.1, today.2);

    // 1) change-id 与黑板目录（幂等：同一任务重复触发复用）
    let (change_id, change_dir) =
        assign_change_id(plans_root, project, task, requirement, &date)?;
    let blackboard = crate::change::change_id::change_root(plans_root, project, task);

    // 2) 文档档位（complete 需要先跑 doc-from-zero，本层不假装自己写完了文档）
    let doc_action = docs::apply(repo, level)?;
    if doc_action == DocAction::FullChainRequired {
        return Err(DevforgeError::Quality(
            "doc_level=complete 需要先跑 doc-from-zero 生成文档，change SOP 不代跑（调用方决定）".into(),
        ));
    }

    // 3) 基线（无 .git 则 init + 全量提交；sha 落到计划目录）
    let plan_dir = repo.join("docs/plantree/changes").join(&change_id);
    std::fs::create_dir_all(&plan_dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", plan_dir.display())))?;
    let baseline = ensure_baseline(repo, &plan_dir)?;

    // 4) 黑板骨架（impl-steps 与证据都在这里，agent 直接写）
    for sub in ["evidence", "testing", "coding/reports"] {
        let dir = blackboard.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", dir.display())))?;
    }

    Ok(ChangePrep {
        change_id,
        blackboard,
        change_dir: if change_dir.is_dir() {
            change_dir
        } else {
            plan_dir
        },
        baseline,
        doc_level: level,
        doc_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_with_code() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "// a\n").unwrap();
        tmp
    }

    #[test]
    fn bare_repo_gets_baseline_and_blackboard_skeleton() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        let plans = tmp.path().join("plans");

        let prep = prepare(
            &plans,
            repo.path(),
            "proj",
            "task1",
            "add book api",
            "none",
            (2026, 10, 20),
        )
        .unwrap();

        assert_eq!(prep.change_id, "change-20261020-add-book-api");
        let baseline_txt = repo
            .path()
            .join("docs/plantree/changes")
            .join(&prep.change_id)
            .join("baseline.txt");
        assert!(baseline_txt.is_file(), "baseline.txt 要落在计划目录");
        assert_eq!(
            std::fs::read_to_string(&baseline_txt).unwrap().trim(),
            prep.baseline.sha
        );
        assert!(prep.baseline.created_repo, "无 .git 时应新建仓库");
        for sub in ["evidence", "testing", "coding/reports"] {
            assert!(
                prep.blackboard.join(sub).is_dir(),
                "黑板缺子目录 {sub}：{}",
                prep.blackboard.display()
            );
        }
    }

    #[test]
    fn none_level_does_not_touch_docs() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        prepare(
            &tmp.path().join("plans"),
            repo.path(),
            "p",
            "t",
            "x",
            "none",
            (2026, 10, 20),
        )
        .unwrap();
        assert!(!repo.path().join("docs/contexts").exists());
    }

    #[test]
    fn essential_level_emits_inventory_then_proceeds() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        let prep = prepare(
            &tmp.path().join("plans"),
            repo.path(),
            "p",
            "t",
            "x",
            "essential",
            (2026, 10, 20),
        )
        .unwrap();
        assert_eq!(prep.doc_action, DocAction::EssentialDone);
        assert!(repo.path().join("docs/contexts/_meta/inventory.json").is_file());
    }

    #[test]
    fn complete_level_is_refused_with_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        let err = prepare(
            &tmp.path().join("plans"),
            repo.path(),
            "p",
            "t",
            "x",
            "complete",
            (2026, 10, 20),
        )
        .unwrap_err();
        assert!(err.to_string().contains("doc-from-zero"), "{err}");
    }

    #[test]
    fn repeated_prepare_reuses_change_id() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        let plans = tmp.path().join("plans");
        let first = prepare(&plans, repo.path(), "p", "t", "add book", "none", (2026, 10, 20)).unwrap();
        let second =
            prepare(&plans, repo.path(), "p", "t", "totally different words", "none", (2026, 10, 21))
                .unwrap();
        assert_eq!(first.change_id, second.change_id, "同一任务必须复用 change-id");
        assert_eq!(first.change_dir, second.change_dir);
        // 已有基线不重建仓库
        assert!(!second.baseline.created_repo);
        assert_eq!(first.baseline.sha, second.baseline.sha);
    }

    #[test]
    fn missing_repo_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let err = prepare(
            tmp.path(),
            &tmp.path().join("nope"),
            "p",
            "t",
            "x",
            "none",
            (2026, 10, 20),
        )
        .unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
    }

    #[test]
    fn bad_doc_level_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_with_code();
        let err = prepare(
            tmp.path(),
            repo.path(),
            "p",
            "t",
            "x",
            "full",
            (2026, 10, 20),
        )
        .unwrap_err();
        assert!(err.to_string().contains("doc_level"), "{err}");
    }
}
