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
        // 自审 minor（195）：profile 门卫必须排在 worktree 创建**之前**——最常见的配置
        // 错误（profile 未渲染）旧口径会先建后删 worktree，白跑一轮 git。
        for p in CHANGE_PROFILES {
            self.require_profile(p)?;
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
                    event = "workspace.created",
                    path = %ws.path.display(),
                    base_sha = %ws.base_rev,
                    branch = %ws.branch,
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
        // change-plan-agent 被要求 codegraph-first：索引必须先建（与其他代码类 SOP 同口径）
        crate::engine::builtin::preflight_codegraph(&run_ws, &self.codegraph_bin);

        let task_root = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&task_root).ctx("建任务黑板目录")?;
        // 需求原文：调用方开跑前写进任务黑板（不给需求就没法规划）
        let requirement_path = task_root.join(REQUIREMENT_FILE);
        // 自审 minor（196）：需求缺失/为空是**装配/配置错误**（调用方没把前置条件备好），
        // 不是质量失败——旧口径归 Quality 会让重试策略按「内容不合格」处理。
        let requirement = std::fs::read_to_string(&requirement_path).map_err(|e| {
            DevforgeError::Config(format!(
                "缺少需求文件 {}（change SOP 开跑前必须写清楚要改什么）：{e}",
                requirement_path.display()
            ))
        })?;
        if requirement.trim().is_empty() {
            return Err(DevforgeError::Config(format!(
                "需求文件 {} 是空的（装配错误：调用方必须写入非空需求）",
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
            event = "change.gate0_done",
            change_id = %prep.change_id,
            baseline = %prep.baseline.sha,
            doc_level = prep.doc_level.as_str(),
            "change 门 0 完成"
        );

        // 会话的工作目录与证据根 = SOP 级黑板（门 0 已建好骨架）
        let blackboard = prep.blackboard.clone();

        // 计划事实（续跑时计划文件已存在，先回读）
        // 自审 minor（197）：旧口径把**全部** IO 错误吞成空 vec 且不记日志——
        // 「首跑没有计划文件（正常）」与「续跑读失败/损坏（异常）」两种语义被混同，
        // 续跑会带着空验收/空白名单继续跑，越界声明全被当成 scope creep。
        let plan_path = prep.change_dir.join("change-plan.md");
        let (criteria, whitelist) = match std::fs::read_to_string(&plan_path) {
            Ok(text) => crate::change::plan_audit::plan_facts(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
            Err(e) => {
                tracing::warn!(
                    component = "change-engine",
                    event = "change.plan_read_failed",
                    path = %plan_path.display(),
                    error = %e,
                    "计划文件存在但读不出（验收标准/白名单为空，可能把合法改动判成越界）"
                );
                (Vec::new(), Vec::new())
            }
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
            .with_working_dir(blackboard.clone())
            // 自审 major：台账必须落 project 层——working_dir（change 黑板）在
            // task 层，涌现推导会让熔断与启动对账对六个角色全部失明。
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?);
        let plan_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-plan-review"),
            "change-plan-review",
            sessions.join("change-plan-reviews"),
            blackboard.clone(),
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
        );
        let coding_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-coding-agent"),
                "change-coding-agent",
                run_id.clone(),
                sessions.join("change-coding"),
            )
            .with_working_dir(blackboard.clone())
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?);
        let coding_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-coding-review"),
            "change-coding-review",
            sessions.join("change-coding-reviews"),
            blackboard.clone(),
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
        );
        let test_agent = RpcPlanSession::new(RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("change-test-agent"),
                "change-test-agent",
                run_id.clone(),
                sessions.join("change-testing"),
            )
            .with_working_dir(blackboard.clone())
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?);
        let test_review = RpcReviewFactory::per_round_in(
            self.profile_root.join("change-test-review"),
            "change-test-review",
            sessions.join("change-test-reviews"),
            blackboard.clone(),
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
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
        // 自审 minor（175）：台账路径进看门狗（强杀走精确路径，扫盘只做降级）
        let ledger = self.plans_root.join(&task.project).join("spawn-ledger.json");
        let sop_result = super::run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            watch_dirs,
            Some(ledger),
            Some(self.unblock_hook()),
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
                // 自审 critical：报告必须落在目标主仓，不是 worktree——worktree 隔离下
                // 旧实现把 report.md 写进 worktree，随后 ws_guard 的 Drop 连目录一起删
                // （git worktree remove --force），run 报 Completed 但 report_path 指向
                // 不存在的文件。非 worktree 模式两者同路径，行为不变。
                let finalized =
                    finalize(&mut outcome, &session_root, &run_id, &task.workspace, started)?;
                // P012 M1 · T1.1：候选冻结 commit 落黑板（diff 归因锚点）；随后 guard 清 worktree（分支保留）
                if let Some(ws) = &ws_guard.ws {
                    let frozen = ws.freeze()?;
                    // 自审 minor（198）：冻结锚点旧口径零校验——HEAD 未前进时 frozen == 基线，
                    // 仍被写盘并被调度器入队当候选（「候选」= 空改动，下游归因/合入全错）。
                    if frozen == baseline {
                        tracing::warn!(
                            component = "change-engine",
                            event = "change.frozen_at_baseline",
                            frozen = %frozen,
                            branch = %ws.branch,
                            "候选分支停在基点（没有任何提交）——不写冻结锚点、不作为候选入队"
                        );
                    } else if frozen.trim().len() < 7 {
                        tracing::warn!(
                            component = "change-engine",
                            event = "change.frozen_shape_unexpected",
                            frozen = %frozen,
                            "冻结锚点不是 commit sha 形状——不写盘（下游会把它当锚点）"
                        );
                    } else {
                    let frozen_path = self.plans_root.join(&task.project).join(&task.id).join("frozen-commit.txt");
                    std::fs::write(&frozen_path, format!("{frozen}\n"))
                        .ctx("写冻结 commit 记录")?;
                    tracing::info!(
                        component = "change-engine",
                        event = "change.frozen",
                        frozen = %frozen,
                        branch = %ws.branch,
                        "候选已冻结（worktree 即将清理，分支保留）"
                    );
                    }
                }
                tracing::info!(
                    component = "change-engine",
                    event = "change.passed",
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
///
/// 自审 minor（44）：旧口径直接 `seg[0..4]` 字节切片——段长不足或含非 ASCII 时
/// **panic**（切片越界 / 非字符边界），解析失败又会静默落成 1970-01-01（日期错标）。
/// 现在按「长度 + 全数字」显式校验，非法即回落到系统时钟并留痕。
fn today_parts() -> (i32, u32, u32) {
    let seg = crate::change::change_id::date_segment_now();
    let parsed = seg
        .get(0..8)
        .filter(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| {
            Some((
                s[0..4].parse::<i32>().ok()?,
                s[4..6].parse::<u32>().ok()?,
                s[6..8].parse::<u32>().ok()?,
            ))
        });
    match parsed {
        Some(t) => t,
        None => {
            tracing::warn!(
                component = "change-engine",
                event = "change.bad_today_segment",
                segment = %seg,
                "日期段不是 8 位数字（DEVFORGE_TODAY 写错了？）——回落到系统时钟"
            );
            let (y, m, d) = crate::change::change_id::today_ymd();
            (y, m, d)
        }
    }
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
#[path = "change_engine_tests.rs"]
mod tests;
