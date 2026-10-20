//! `change` SOP 的引擎接线（P009）：门 0 → 三阶段会话 → 报告 → 断点。
//!
//! 这里只做装配：确定性逻辑在 `crate::change::*`，判断在提示词与机械门里。
//! 单测用协议级 mock（见 `server::tests`），不 spawn 真 pi。

use std::path::PathBuf;

use super::BuiltinRpcEngine;
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
    workspace: &std::path::Path,
    started: std::time::Instant,
) -> Result<Finalized> {
    let usage = crate::docgen::usage::from_session_dirs(&session_dirs(session_root));
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
        for p in CHANGE_PROFILES {
            self.require_profile(p)?;
        }

        let blackboard = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&blackboard).ctx("建黑板目录")?;
        // 需求原文：调用方开跑前写进黑板（不给需求就没法规划）
        let requirement_path = blackboard.join(REQUIREMENT_FILE);
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
            &task.workspace,
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

        // 计划事实（续跑时计划文件已存在，先回读）
        let (criteria, whitelist) = match std::fs::read_to_string(prep.change_dir.join("change-plan.md"))
        {
            Ok(text) => crate::change::plan_audit::plan_facts(&text),
            Err(_) => (Vec::new(), Vec::new()),
        };

        let ctx = ChangeCtx {
            repo: task.workspace.clone(),
            blackboard: blackboard.clone(),
            change_dir: prep.change_dir.clone(),
            change_id: prep.change_id.clone(),
            baseline: prep.baseline.sha.clone(),
            requirement: requirement.clone(),
            doc_level: prep.doc_level,
        };

        // 会话装配：三个干活（长驻）+ 三个审查（每轮新实例，用完即弃）
        let run_id = run.id.0.clone();
        let plan_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-plan-agent"),
                "change-plan-agent",
                run_id.clone(),
                self.session_root.join("change-plan"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let plan_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-plan-review"),
            "change-plan-review",
            self.session_root.join("change-plan-reviews"),
            blackboard.clone(),
        );
        let coding_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-coding-agent"),
                "change-coding-agent",
                run_id.clone(),
                self.session_root.join("change-coding"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let coding_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-coding-review"),
            "change-coding-review",
            self.session_root.join("change-coding-reviews"),
            blackboard.clone(),
        );
        let test_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-test-agent"),
                "change-test-agent",
                run_id.clone(),
                self.session_root.join("change-testing"),
            )
            .with_working_dir(blackboard.clone()),
        )?);
        let test_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-test-review"),
            "change-test-review",
            self.session_root.join("change-test-reviews"),
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
        };
        let store = StateStore::new(
            self.plans_root.clone(),
            StateKey::new(&task.project, &task.id, CHANGE_SOP),
        );
        let started = std::time::Instant::now();
        let session_root = self.session_root.clone();
        let ui = self.ui.clone();
        let workspace = task.workspace.clone();
        let change_id = prep.change_id.clone();
        let baseline = prep.baseline.sha.clone();
        let watch_dirs = session_dirs(&self.session_root).to_vec();

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
                let finalized = finalize(&mut outcome, &session_root, &workspace, started)?;
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
            Err(e) => self.map_intervention(control, "change-engine", e),
        }
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
pub fn session_dirs(session_root: &std::path::Path) -> [PathBuf; 6] {
    [
        session_root.join("change-plan"),
        session_root.join("change-plan-reviews"),
        session_root.join("change-coding"),
        session_root.join("change-coding-reviews"),
        session_root.join("change-testing"),
        session_root.join("change-test-reviews"),
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
    fn finalize_writes_report_with_usage_and_wall_clock() {
        let tmp = tempfile::tempdir().unwrap();
        let mut out = outcome();
        let f = finalize(&mut out, &tmp.path().join("sessions"), tmp.path(), std::time::Instant::now())
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
        let dirs = session_dirs(std::path::Path::new("/s"));
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
    }
}
