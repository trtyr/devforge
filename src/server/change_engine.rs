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
        let project = task.project.clone();
        let task_id = task.id.clone();
        let workspace = task.workspace.clone();
        let change_id = prep.change_id.clone();
        let baseline = prep.baseline.sha.clone();

        let result = run_orchestrator(
            ChangeRun {
                store: &store,
                repo: &workspace,
                change_id: &change_id,
                baseline_sha: &baseline,
                criteria,
                whitelist,
                usage: String::new(),
                run_id,
                wall_secs: 0,
            },
            &mut session,
            &mut RelayUserLink { relay: ui },
        );

        match result {
            Ok(mut outcome) => {
                // 用量从会话账汇总（不是 agent 自述）
                let usage = crate::docgen::usage::from_session_dirs(&[
                    session_root.join("change-plan"),
                    session_root.join("change-coding"),
                    session_root.join("change-testing"),
                ]);
                outcome.report.usage = usage.render();
                outcome.report.wall_secs = started.elapsed().as_secs();
                let report_path = write_report(&workspace, &change_id, &outcome.report)?;
                tracing::info!(
                    component = "change-engine",
                    change_id = %change_id,
                    report = %report_path.display(),
                    rounds = outcome.report.review_rounds,
                    roundtrips = outcome.report.roundtrips,
                    "change SOP 通过"
                );
                let _ = (project, task_id, control);
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
