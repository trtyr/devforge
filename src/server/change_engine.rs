//! `change` SOP 的引擎接线（P009）：门 0 → 三阶段会话 → 报告 → 断点。
//!
//! 这里只做装配：确定性逻辑在 `crate::change::*`，判断在提示词与机械门里。
//! 单测用协议级 mock（见 `server::tests`），不 spawn 真 pi。

use std::path::{Path, PathBuf};

use crate::engine::builtin::BuiltinRpcEngine;
use crate::change::orchestrator::{ChangeRun, run as run_orchestrator};
use crate::change::prepare::prepare;
use crate::change::report::write_report;
use crate::change::session::{ChangeCtx, ProdChangeSession};
use crate::change::state::CHANGE_SOP;
use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunOutcome};
use crate::domain::task::Task;
use crate::error::{Context as _, DevforgeError, Result};
use crate::runstate::{StateKey, StateStore};
use crate::server::RelayUserLink;
use crate::sop::rpc_bridge::{RpcPlanSession, RpcReviewFactory};
use crate::{engine::RpcClient, engine::SpawnSpec};

/// change SOP 需要的六个 profile（三个干活 + 三个审查）。
pub const CHANGE_PROFILES: [&str; 6] = [
    "change-plan-agent",
    "change-plan-review",
    "change-coding-agent",
    "change-coding-review",
    "change-test-agent",
    "change-test-review",
];

/// 需求文件（调用方在 start 前写进黑板）。
pub const REQUIREMENT_FILE: &str = "requirement.md";

/// 收尾结果（报告落盘 + 用量/墙钟）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finalized {
    pub report_path: PathBuf,
    pub usage: String,
    pub wall_secs: u64,
}

/// 收尾：用量从会话账汇总（不是 agent 自述）+ 写 `report.md`。
///
/// 抽出来单独测：真 run 要 pi 子进程，但「报告写没写对」不该只能靠端到端验证。
pub fn finalize(
    outcome: &mut crate::change::orchestrator::ChangeOutcome,
    session_root: &std::path::Path,
    run_id: &str,
    workspace: &std::path::Path,
    started: std::time::Instant,
) -> Result<Finalized> {
    let usage = crate::docgen::usage::from_session_dirs(&session_dirs(session_root, run_id));
    outcome.report.usage = usage.render();
    outcome.report.wall_secs = started.elapsed().as_secs();
    let report_path = write_report(workspace, &outcome.report.change_id, &outcome.report)?;
    Ok(Finalized {
        report_path,
        usage: outcome.report.usage.clone(),
        wall_secs: outcome.report.wall_secs,
    })
}

impl BuiltinRpcEngine {
    /// 跑一次 change run（门 0 → plan/coding/testing → 报告）。
    pub fn run_change(
        &mut self,
        task: &Task,
        run: &Run,
        control: &RunControl,
    ) -> Result<RunOutcome> {
        if task.workspace.as_os_str().is_empty() {
            return Err(DevforgeError::Store(format!(
                "任务 `{}/{}` 无代码工作区，change SOP 需要项目地址",
                task.project, task.id
            )));
        }
        // P012 M1 · T1.2：worktree 隔离——契约带 worktree 时，本 run 的全部读写落在
        // 独立 worktree（create → spawn cwd = worktree → freeze 候选 → remove，分支保留）。
        // guard 负责 Ok/Err 两路都清理；冻结 sha 在 Ok 分支落黑板供 diff 归因。
        struct WsCleanup<'a> {
            ws: Option<crate::workspace::Workspace>,
            repo: &'a std::path::Path,
        }
        impl Drop for WsCleanup<'_> {
            fn drop(&mut self) {
                if let Some(ws) = self.ws.take() {
                    // change 批次 #5：清理失败不得静默——残留的脏 worktree 会被下次 run 的
                    // create/ensure 复用，表现为「新 run 一开局就带着上一轮的未提交改动」。
                    if let Err(e) = ws.remove(self.repo) {
                        tracing::error!(
                            component = "change-engine",
                            event = "workspace.cleanup_failed",
                            repo = %self.repo.display(),
                            error = %e,
                            "worktree 清理失败：残留脏 worktree 可能被下次 run 复用"
                        );
                    }
                }
            }
        }
        let ws_guard = {
            let want = task
                .contract
                .as_ref()
                .map(|c| matches!(c.isolation, crate::domain::contract::Isolation::Worktree))
                .unwrap_or(false);
            if want {
                let base = task
                    .contract
                    .as_ref()
                    .and_then(|c| c.base_rev.clone())
                    .unwrap_or_else(|| "HEAD".into());
                let ws = crate::workspace::Workspace::ensure(&crate::workspace::WorkspaceSpec {
                    repo: task.workspace.clone(),
                    task_id: task.id.clone(),
                    base_rev: base,
                })?;
                tracing::info!(
                    component = "change-engine",
                    path = %ws.path.display(),
                    "worktree 工作区已创建（本 run 全部读写落在其中）"
                );
                WsCleanup {
                    ws: Some(ws),
                    repo: &task.workspace,
                }
            } else {
                WsCleanup { ws: None, repo: &task.workspace }
            }
        };
        let run_ws = match &ws_guard.ws {
            Some(ws) => ws.path.clone(),
            None => task.workspace.clone(),
        };
        for p in CHANGE_PROFILES {
            self.require_profile(p)?;
        }
        // change-plan-agent 被要求 codegraph-first：索引必须先建（与其他代码类 SOP 同口径）
        crate::engine::builtin::preflight_codegraph(&run_ws, &self.codegraph_bin);

        let task_root = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&task_root).ctx("建任务黑板目录")?;
        // 需求原文：调用方开跑前写进任务黑板（不给需求就没法规划）
        let requirement_path = task_root.join(REQUIREMENT_FILE);
        let requirement = std::fs::read_to_string(&requirement_path).map_err(|e| {
            DevforgeError::Quality(format!(
                "缺少需求文件 {}（change SOP 开跑前必须写清楚要改什么）：{e}",
                requirement_path.display()
            ))
        })?;
        if requirement.trim().is_empty() {
            return Err(DevforgeError::Quality(format!(
                "需求文件 {} 是空的",
                requirement_path.display()
            )));
        }

        let doc_level = std::env::var("DEVFORGE_DOC_LEVEL").unwrap_or_else(|_| "none".into());
        let today = today_parts();
        let prep = prepare(
            &self.plans_root,
            &run_ws,
            &task.project,
            &task.id,
            &requirement,
            &doc_level,
            today,
        )?;
        tracing::info!(
            component = "change-engine",
            change_id = %prep.change_id,
            baseline = %prep.baseline.sha,
            doc_level = prep.doc_level.as_str(),
            "change 门 0 完成"
        );

        // 会话的工作目录与证据根 = SOP 级黑板（门 0 已建好骨架）
        let blackboard = prep.blackboard.clone();

        // 计划事实（续跑时计划文件已存在，先回读）
        let (criteria, whitelist) = match std::fs::read_to_string(prep.change_dir.join("change-plan.md"))
        {
            Ok(text) => crate::change::plan_audit::plan_facts(&text),
            Err(_) => (Vec::new(), Vec::new()),
        };

        let ctx = build_ctx(&prep, &run_ws, &requirement, task.contract.clone());

        // 会话装配：三个干活（长驻）+ 三个审查（每轮新实例，用完即弃）
        let run_id = run.id.0.clone();
        // 引擎批次 #4：change 的六个角色会话也按 run-id 隔离
        let sessions = self.session_root.join(&run_id);
        let plan_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-plan-agent"),
                "change-plan-agent",
                run_id.clone(),
                sessions.join("change-plan"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let plan_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-plan-review"),
            "change-plan-review",
            sessions.join("change-plan-reviews"),
            blackboard.clone(),
        );
        let coding_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-coding-agent"),
                "change-coding-agent",
                run_id.clone(),
                sessions.join("change-coding"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let coding_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-coding-review"),
            "change-coding-review",
            sessions.join("change-coding-reviews"),
            blackboard.clone(),
        );
        let test_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-test-agent"),
                "change-test-agent",
                run_id.clone(),
                sessions.join("change-testing"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let test_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-test-review"),
            "change-test-review",
            sessions.join("change-test-reviews"),
            blackboard.clone(),
        );

        let mut session = ProdChangeSession {
            plan_agent: Box::new(plan_agent),
            plan_review: Box::new(plan_review),
            coding_agent: Box::new(coding_agent),
            coding_review: Box::new(coding_review),
            test_agent: Box::new(test_agent),
            test_review: Box::new(test_review),
            ctx: ctx.clone(),
            plan_review_instances: 0,
        };
        let store = StateStore::new(
            self.plans_root.clone(),
            StateKey::new(&task.project, &task.id, CHANGE_SOP),
        );
        let started = std::time::Instant::now();
        let session_root = self.session_root.clone();
        let ui = self.ui.clone();
        let workspace = run_ws.clone();
        let change_id = prep.change_id.clone();
        let baseline = prep.baseline.sha.clone();
        let watch_dirs = session_dirs(&self.session_root, &run_id).to_vec();

        // 取消/超时/失活与其他 SOP 同口径：跑在 watchdog 里，不然 cancel 端点停不下来
        let sop_result = super::run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            watch_dirs,
            || {
                run_orchestrator(
                    ChangeRun {
                        store: &store,
                        repo: &workspace,
                        change_dir: &prep.change_dir,
                        change_id: &change_id,
                        baseline_sha: &baseline,
                        criteria,
                        whitelist,
                        usage: String::new(),
                        run_id: run_id.clone(),
                        wall_secs: 0,
                    },
                    &mut session,
                    &mut RelayUserLink { relay: ui },
                )
            },
        );

        match sop_result {
            Ok(mut outcome) => {
                let finalized = finalize(&mut outcome, &session_root, &run_id, &workspace, started)?;
                // P012 M1 · T1.1：候选冻结 commit 落黑板（diff 归因锚点）；随后 guard 清 worktree（分支保留）
                if let Some(ws) = &ws_guard.ws {
                    let frozen = ws.freeze()?;
                    let frozen_path = self.plans_root.join(&task.project).join(&task.id).join("frozen-commit.txt");
                    std::fs::write(&frozen_path, format!("{frozen}\n"))
                        .ctx("写冻结 commit 记录")?;
                    tracing::info!(
                        component = "change-engine",
                        frozen = %frozen,
                        branch = %ws.branch,
                        "候选已冻结（worktree 即将清理，分支保留）"
                    );
                }
                tracing::info!(
                    component = "change-engine",
                    change_id = %change_id,
                    report = %finalized.report_path.display(),
                    rounds = outcome.report.review_rounds,
                    roundtrips = outcome.report.roundtrips,
                    "change SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => crate::engine::policy::map_intervention(control, "change-engine", e),
        }
    }
}

/// 把门 0 产出装配成会话上下文（抽出来单测：真实 run 不需要 pi 也能验路径）。
pub fn build_ctx(
    prep: &crate::change::prepare::ChangePrep,
    repo: &Path,
    requirement: &str,
    contract: Option<crate::domain::contract::WorkItemContract>,
) -> ChangeCtx {
    ChangeCtx {
        repo: repo.to_path_buf(),
        blackboard: prep.blackboard.clone(),
        change_dir: prep.change_dir.clone(),
        change_id: prep.change_id.clone(),
        baseline: prep.baseline.sha.clone(),
        requirement: requirement.to_string(),
        doc_level: prep.doc_level,
        contract,
    }
}

/// 今天的 `(年, 月, 日)`（`DEVFORGE_TODAY` 可覆盖）。
fn today_parts() -> (i32, u32, u32) {
    let seg = crate::change::change_id::date_segment_now();
    let year: i32 = seg[0..4].parse().unwrap_or(1970);
    let month: u32 = seg[4..6].parse().unwrap_or(1);
    let day: u32 = seg[6..8].parse().unwrap_or(1);
    (year, month, day)
}

/// 供测试/调用方构造会话目录（避免路径拼写在两处漂移）。
/// 引擎批次 #4：会话按 run-id 隔离，所以目录根是 `sessions/<run-id>/`。
pub fn session_dirs(session_root: &std::path::Path, run_id: &str) -> [PathBuf; 6] {
    let base = session_root.join(run_id);
    [
        base.join("change-plan"),
        base.join("change-plan-reviews"),
        base.join("change-coding"),
        base.join("change-coding-reviews"),
        base.join("change-testing"),
        base.join("change-test-reviews"),
    ]
}

#[cfg(test)]
mod tests {
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
}
