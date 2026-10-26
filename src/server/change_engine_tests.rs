//! （2026-10-26 测试分离：自 src/server/change_engine.rs 内联平移，纯移动零改动。）
use super::*;
use crate::change::report::{AcceptanceSummary, ChangeReport, DiffStats};
use crate::change::state::{ChangePhase, ChangeState};
use crate::change::orchestrator::ChangeOutcome;

fn outcome() -> ChangeOutcome {
    ChangeOutcome {
        state: ChangeState::new("change-1", "base"),
        report: ChangeReport {
            change_id: "change-1".into(),
            run_id: "run-1".into(),
            baseline_sha: "base".into(),
            wall_secs: 0,
            diff: DiffStats {
                files: 1,
                insertions: 2,
                deletions: 0,
            },
            review_rounds: 2,
            roundtrips: 1,
            usage: String::new(),
            acceptance: AcceptanceSummary {
                total: 1,
                done: 1,
                missing: vec![],
            },
        },
    }
}

#[test]
fn ctx_paths_point_at_repo_plan_dir_and_sop_blackboard() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
    let prep = crate::change::prepare::prepare(
        &tmp.path().join("plans"),
        &repo,
        "p",
        "t",
        "add x",
        "none",
        (2026, 10, 20),
    )
    .unwrap();
    let ctx = build_ctx(&prep, &repo, "add x", None);
    assert_eq!(
        ctx.plan_path(),
        crate::change::prepare::plantree_root(&tmp.path().join("plans"), "p")
            .join("changes")
            .join(&prep.change_id)
            .join("change-plan.md"),
        "计划路径必须在台账根（2026-10-25 反转 D003：台账不进仓）"
    );
    assert!(!ctx.plan_path().starts_with(&repo), "计划不得落进目标仓");
    assert!(
        ctx.blackboard.ends_with("p/t/change"),
        "证据根必须是 SOP 级黑板：{}",
        ctx.blackboard.display()
    );
    assert!(ctx.blackboard.join("evidence").is_dir(), "门 0 骨架要在会话用的黑板上");
    assert!(ctx.blackboard.join("testing/reports").is_dir());
    assert_eq!(ctx.baseline, prep.baseline.sha);
}

#[test]
fn finalize_writes_report_with_usage_and_wall_clock() {
    let tmp = tempfile::tempdir().unwrap();
    let mut out = outcome();
    let f = finalize(
        &mut out,
        &tmp.path().join("sessions"),
        "run-t",
        tmp.path(),
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(f.report_path.is_file());
    let text = std::fs::read_to_string(&f.report_path).unwrap();
    assert!(text.contains("用量:"), "{text}");
    assert!(text.contains("验收对账: 通过"), "{text}");
    assert!(text.contains("审查轮次: 2"), "{text}");
    assert!(text.contains("往返: 1"), "{text}");
    assert_eq!(out.state.phase, ChangePhase::Plan, "收尾不得改阶段");
}

#[test]
fn session_dirs_cover_all_six_profiles() {
    let dirs = session_dirs(std::path::Path::new("/s"), "run-x");
    let names: Vec<String> = dirs
        .iter()
        .map(|d| d.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), CHANGE_PROFILES.len());
    for n in [
        "change-plan",
        "change-plan-reviews",
        "change-coding",
        "change-coding-reviews",
        "change-testing",
        "change-test-reviews",
    ] {
        assert!(names.contains(&n.to_string()), "缺会话目录 {n}");
    }
    // 引擎批次 #4：会话必须先落在 sessions/<run-id>/ 下
    assert!(
        dirs.iter().all(|d| d.parent().unwrap().file_name().unwrap() == "run-x"),
        "会话目录必须按 run-id 隔离：{dirs:?}"
    );
}
