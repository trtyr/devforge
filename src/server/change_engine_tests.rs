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
        &tmp.path().join("ledger").join("change-20261020-x"),
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

// ── 2026-10-27 自审 change_engine 批回归 ──

fn test_git(repo: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 自审 minor（102/198）：worktree 隔离链路的冻结锚点语义必须可测——
/// HEAD 未前进时 `freeze()` == 基点（此时不得写盘/当候选入队），有提交后才与基点不同。
#[test]
fn freeze_equals_baseline_until_the_agent_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    test_git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
    test_git(&repo, &["add", "-A"]);
    test_git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    let base = String::from_utf8(
        std::process::Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let ws = crate::workspace::Workspace::ensure(&crate::workspace::WorkspaceSpec {
        repo: repo.clone(),
        task_id: "freeze-probe".into(),
        base_rev: base.clone(),
    })
    .unwrap();
    assert_eq!(
        ws.freeze().unwrap(),
        base,
        "未提交时应冻结在基点（这正是「空候选不得入队」的判据）"
    );

    // agent 提交之后锚点才前进
    std::fs::write(ws.path.join("a.rs"), "fn a() { let _x = 1; }\n").unwrap();
    test_git(&ws.path, &["add", "-A"]);
    test_git(
        &ws.path,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "impl",
        ],
    );
    let frozen = ws.freeze().unwrap();
    assert_ne!(frozen, base, "有提交后冻结锚点必须前进");
    assert_eq!(frozen.len(), 40, "冻结锚点必须是完整 commit sha：{frozen}");
    ws.remove(&repo).unwrap();
}

/// 外部审计 #2（2026-10-28）+ 自审 critical（102 前案）：报告必须落在**台账根的
/// change_dir**——写 worktree 会被 remove --force 连目录删（102 前案：run 报
/// Completed 但 report_path 指向不存在的文件）；写主仓则把台账产物塞进目标仓，
/// 违反「台账不进仓」（2026-10-25 反转 D003）。
#[test]
fn finalize_writes_report_into_the_ledger_change_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let worktree = tmp.path().join("repo-worktrees").join("t1");
    std::fs::create_dir_all(&worktree).unwrap();
    let change_dir = tmp
        .path()
        .join("ledger")
        .join("proj")
        .join("change-20261020-x");
    let sessions = worktree.join("sessions"); // 会话目录在 worktree 侧
    let mut out = outcome();
    let f = finalize(
        &mut out,
        &sessions,
        "run-w",
        &change_dir,
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(
        f.report_path.starts_with(&change_dir),
        "报告必须落在台账 change_dir：{}",
        f.report_path.display()
    );
    assert!(
        !f.report_path.starts_with(&worktree),
        "报告不得落在会被 remove --force 删掉的 worktree 里"
    );
    assert!(
        !f.report_path.starts_with(&repo),
        "报告也不得写进目标主仓（台账不进仓）：{}",
        f.report_path.display()
    );
    assert!(f.report_path.is_file());
}
