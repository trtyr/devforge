//! change 编排器（P009 T013）：阶段推进 + coding→plan 反向回退 + 往返上限 3。
//!
//! 与 MVP 编排的区别（不是「单向 vs 带回路」——MVP 每段本来就有 agent⇄review）：
//! - 多一个**反向回退**：coding 发现计划漏了文件 → 申报 → 回 plan 补边界 → 重进 coding；
//! - 多一个**基线锚点**：diff、白名单、回归都对着基线；
//! - 断点由引擎写（这里复用 plan 010 的 `StateStore`，跨会话不丢往返计数）。
//!
//! 本模块只管**编排**：它不懂 prompt，也不碰子进程；每一阶段的真实执行由 [`ChangeSession`]
//! 注入（生产是 pi 子进程，单测是脚本化的假 session）。

use std::path::Path;

use crate::change::boundary::{self, BoundaryVerdict, ImpactGap};
use crate::change::report::{self, AcceptanceSummary, ChangeReport, summarize_acceptance};
use crate::change::state::{ChangePhase, ChangeState, TestFailureContext, load_strict, save};
use crate::error::{DevforgeError, Result};
use crate::runstate::StateStore;
use crate::sop::planning::UserLink;

/// 测试阶段失败后回注 coding 的防御性上限（决策只给了 plan↔coding 的 3 次；这里防止死循环）。
pub const MAX_TEST_REROUTES: usize = 3;

/// plan 阶段审查打回后的修复上限（对齐 MVP 规划的重审节奏；超限 = 计划质量问题，不自动重启）。
pub const MAX_PLAN_FIXES: usize = 3;

/// 阶段转换：写状态 + 落一条里程碑（`phase.change`）+ 把阶段写进观测上下文（心跳随之刷新）。
///
/// 阶段名用 `ChangePhase` 的变体名（plan/coding/testing/done）——与心跳的阶段字段同一套词汇
/// （事后「里程碑说怎么变的」与「心跳说死在哪个阶段」才对得上号）。
fn phase_change(state: &mut ChangeState, to: ChangePhase) {
    let from = state.phase;
    state.phase = to;
    crate::observability::set_phase(to.as_str());
    tracing::info!(
        component = "change-orchestrator",
        event = "phase.change",
        from = from.as_str(),
        to = to.as_str(),
        "change 阶段转换"
    );
}

/// 修复轮：把「正在做什么」写进心跳的步骤字段
/// （心跳要能回答「死在哪个环节」，不只「死在哪个阶段」）。
fn fix_step(state: &ChangeState, kind: &str, round: usize, begin: bool) {
    let step = if begin {
        format!("fix:{kind}:{round}")
    } else {
        state.phase.as_str().to_string()
    };
    crate::observability::set_step(&step);
}

/// 计划阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanTurn {
    Pass,
    Fail { issues: Vec<String> },
}

/// coding 阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingTurn {
    /// agent 的完整回复（含可能的 `<route>plan</route>` + `<impact-gap>`）。
    pub reply: String,
    /// 相对基线的改动文件（`git diff --name-only <baseline>`）。
    pub changed: Vec<String>,
    /// 本阶段消耗的审查轮次（coding review）。
    pub reviews: usize,
}

/// 测试阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestingTurn {
    Pass {
        /// 已对账通过的验收项。
        acceptance_done: Vec<String>,
        /// 本阶段消耗的审查轮次（test review）。
        reviews: usize,
    },
    /// 回归/测试失败 → 回注 coding（带失败明细原文，含 `path:line`）。
    RouteCoding { failures: Vec<String> },
    /// 发现计划层面的问题 → 回 plan。
    RoutePlan { failures: Vec<String> },
}

/// 各阶段的执行器（生产= pi 子进程；单测=脚本化假实现）。
pub trait ChangeSession {
    /// 跑一次 plan（含审查）；`roundtrip` 用于让实现知道这是第几次往返。
    fn run_plan(&mut self, ui: &mut dyn UserLink, roundtrip: usize) -> Result<PlanTurn>;
    /// 带缺口打回 plan：只补边界并重审。
    fn amend_plan(
        &mut self,
        ui: &mut dyn UserLink,
        gaps: &[ImpactGap],
        roundtrip: usize,
    ) -> Result<PlanTurn>;
    /// 审查打回 plan：把问题清单回注，plan agent 重写计划后重审。
    fn fix_plan(
        &mut self,
        ui: &mut dyn UserLink,
        issues: &[String],
        round: usize,
    ) -> Result<PlanTurn>;
    /// 跑 coding（含按步提交）。
    ///
    /// `last_failure` = 上一轮测试失败的上下文（`None` = 无上下文）：回注轮把它送进
    /// coding 开工语（送达面）；`None` 时开工语逐字不变。
    fn run_coding(
        &mut self,
        ui: &mut dyn UserLink,
        roundtrip: usize,
        last_failure: Option<&TestFailureContext>,
    ) -> Result<CodingTurn>;
    /// 跑 testing（回归优先 + 验收对账）。
    fn run_testing(&mut self, ui: &mut dyn UserLink) -> Result<TestingTurn>;

    /// 计划过审后回读计划事实（验收标准 / 白名单）——编排层据此进 coding/testing。
    fn plan_facts(&mut self) -> Result<(Vec<String>, Vec<String>)> {
        Ok((Vec::new(), Vec::new()))
    }

    /// plan 阶段消耗的审查轮次（计入报告；默认 0，脚本化 session 不审）。
    fn plan_reviews(&self) -> usize {
        0
    }
}

/// 编排输入。
pub struct ChangeRun<'a> {
    pub store: &'a StateStore,
    /// 目标仓（diff、报告都落这里）。
    pub repo: &'a Path,
    /// 计划目录（台账根下 `changes/<change-id>`；**不在目标仓内**，2026-10-25 反转 D003）。
    pub change_dir: &'a Path,
    pub change_id: &'a str,
    pub baseline_sha: &'a str,
    /// 计划的验收标准（对账用）。
    pub criteria: Vec<String>,
    /// 计划的白名单（初始）。
    pub whitelist: Vec<String>,
    /// 用量行（会话账汇总；空串则写「未记录」）。
    pub usage: String,
    pub run_id: String,
    /// 墙钟（调用方计时；单测给固定值）。
    pub wall_secs: u64,
}

/// 编排结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeOutcome {
    pub state: ChangeState,
    pub report: ChangeReport,
}

/// 跑完整条 change 流程（从断点继续）。
pub fn run<S: ChangeSession>(
    run: ChangeRun<'_>,
    session: &mut S,
    ui: &mut dyn UserLink,
) -> Result<ChangeOutcome> {
    let mut state = crate::change::state::load_strict(run.store, run.change_id, run.baseline_sha)?;
    if state.baseline_sha.is_empty() {
        state.baseline_sha = run.baseline_sha.to_string();
    }
    let mut whitelist = run.whitelist.clone();
    let mut criteria = run.criteria.clone();

    // ---- plan ----
    // 打回-修复循环（对齐 MVP 规划）：审查 FAIL 回注重写重审，有界；超限才 Quality
    if matches!(state.phase, ChangePhase::Plan) {
        // 引擎审计修复 #5：issues 初始值从断点读——testing→plan 回退写入的
        // last_plan_issues 在 resume 后走 fix_plan(issues) 而非重新 kickoff。
        let mut issues: Vec<String> = state.last_plan_issues.clone();
        // 复验 #9：修复轮计数住断点——重启后不得重新获得修复额度。
        let mut fix_round = state.plan_fix_rounds;
        // change 批次 #1：入口守卫——testing→plan 回退是「先落盘再比较上限」，超限断点仍停在
        // Plan；旧实现从入口直接开跑，再跑一轮 fix_plan 就能 Pass 进 coding，额度被绕过。
        if fix_round > MAX_PLAN_FIXES {
            return Err(DevforgeError::Quality(format!(
                "计划修复轮已达上限 {MAX_PLAN_FIXES} 轮（断点 plan_fix_rounds={fix_round}），拒绝继续修复：{}",
                issues.join("；")
            )));
        }
        loop {
            let turn = if fix_round == 0 {
                session.run_plan(ui, state.roundtrips)?
            } else {
                // 计划修复轮：fix.start → 重写计划 → fix.end（含修复轮次）
                fix_step(&state, "plan", fix_round, true);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.start",
                    round = fix_round,
                    "计划修复轮开始"
                );
                let turn = session.fix_plan(ui, &issues, fix_round)?;
                fix_step(&state, "plan", fix_round, false);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.end",
                    round = fix_round,
                    "计划修复轮结束"
                );
                turn
            };
            state.review_rounds += session.plan_reviews();
            match turn {
                PlanTurn::Pass => {
                    let (c, w) = session.plan_facts()?;
                    if !c.is_empty() {
                        criteria = c;
                    }
                    if !w.is_empty() {
                        whitelist = w;
                    }
                    phase_change(&mut state, ChangePhase::Coding);
                    save(run.store, &state)?;
                    break;
                }
                PlanTurn::Fail { issues: next } => {
                    fix_round += 1;
                    // 失败路径也落盘（复验 #9）：重启不得重置修复额度。
                    state.plan_fix_rounds = fix_round;
                    save(run.store, &state)?;
                    if fix_round > MAX_PLAN_FIXES {
                        return Err(DevforgeError::Quality(format!(
                            "计划审查连续 {fix_round} 轮未通过（上限 {MAX_PLAN_FIXES} 轮修复）：{}",
                            next.join("；")
                        )));
                    }
                    issues = next;
                }
            }
        }
    }

    // ---- coding ⇄ plan ----
    if matches!(state.phase, ChangePhase::Coding) {
        loop {
            let turn = session.run_coding(ui, state.roundtrips, state.last_test_failure.as_ref())?;
            state.review_rounds += turn.reviews;
            match boundary::classify(&turn.changed, &whitelist, &turn.reply) {
                BoundaryVerdict::InBoundary => {
                    // 消费即清：coding 一旦过门，上一轮失败上下文就过期了
                    state.clear_test_failure();
                    phase_change(&mut state, ChangePhase::Testing);
                    save(run.store, &state)?;
                    break;
                }
                BoundaryVerdict::OutOfBoundary { extra } => {
                    return Err(DevforgeError::Quality(format!(
                        "越界改动未申报（范围膨胀）：{}",
                        extra.join("、")
                    )));
                }
                BoundaryVerdict::RoutePlan { gaps } => {
                    let next = state.roundtrips + 1;
                    boundary::check_roundtrip(next)?;
                    state.bump_roundtrip();
                    // 每次回退都落历史（T013：事后能看出边界是怎么被撑大的）
                    crate::change::history::append_roundtrip(
                        run.change_dir,
                        run.change_id,
                        state.roundtrips,
                        &gaps,
                    )?;
                    boundary::merge_gaps_into_whitelist(&mut whitelist, &gaps);
                    // 补边界往返：fix.start → 补计划 → fix.end（含往返轮次）
                    fix_step(&state, "boundary", state.roundtrips, true);
                    tracing::info!(
                        component = "change-orchestrator",
                        event = "fix.start",
                        round = state.roundtrips,
                        "补边界往返开始"
                    );
                    // 复验 #9（中）：bump+append 后立即落盘——amend 的 Err/Fail 路径
                    // 不再丢计数（原实现只有 Pass 分支 save，重启后 roundtrips 回退、
                    // 上限 3 可被反复绕过、history 往返编号重复）。
                    save(run.store, &state)?;
                    let amend = session.amend_plan(ui, &gaps, state.roundtrips)?;
                    fix_step(&state, "boundary", state.roundtrips, false);
                    tracing::info!(
                        component = "change-orchestrator",
                        event = "fix.end",
                        round = state.roundtrips,
                        "补边界往返结束"
                    );
                    match amend {
                        PlanTurn::Pass => {
                            state.review_rounds += session.plan_reviews();
                            let (c, w) = session.plan_facts()?;
                            if !c.is_empty() {
                                criteria = c;
                            }
                            if !w.is_empty() {
                                whitelist = w;
                            }
                            save(run.store, &state)?;
                            continue;
                        }
                        PlanTurn::Fail { issues } => {
                            return Err(DevforgeError::Quality(format!(
                                "补边界后计划审查仍未过：{}",
                                issues.join("；")
                            )));
                        }
                    }
                }
            }
        }
    }

    // ---- testing ----
    // 失败会以 Quality 结束本次调用，并把阶段回推到 coding/plan；下次 start 从断点继续
    // （回注次数记在断点里，跳会话累计，不会被重启清零）。
    let mut acceptance_done: Vec<String> = state.acceptance_done.clone();
    if matches!(state.phase, ChangePhase::Testing) {
        match session.run_testing(ui)? {
            TestingTurn::Pass {
                acceptance_done: done,
                reviews,
            } => {
                state.review_rounds += reviews;
                for c in done {
                    state.mark_acceptance_done(&c);
                }
                acceptance_done = state.acceptance_done.clone();
                // 复验 N3：criteria 为空（计划用表格写验收标准，plan_facts 只认列表项）时
                // 独立报错，不再产生「缺 0 条」的误导性对账输出。
                if criteria.is_empty() {
                    return Err(DevforgeError::Quality(
                        "计划里没有可对账的验收标准——plan_facts 只认「验收标准」块里的列表项，\
                         用表格/段落写法会抽不到，请改用列表并在 plan 阶段重新过审"
                            .into(),
                    ));
                }
                let summary = summarize_acceptance(&criteria, &acceptance_done);
                if !summary.passed() {
                    return Err(DevforgeError::Quality(format!(
                        "验收对账缺 {} 条：{}",
                        summary.missing.len(),
                        summary.missing.join("；")
                    )));
                }
                phase_change(&mut state, ChangePhase::Done);
                // 终态清：收尾不留过期状态
                state.clear_test_failure();
                save(run.store, &state)?;
            }
            TestingTurn::RouteCoding { failures } => {
                state.bump_test_reroute();
                if state.test_reroutes > MAX_TEST_REROUTES {
                    save(run.store, &state)?;
                    return Err(DevforgeError::Quality(format!(
                        "测试→coding 已回注 {} 次（上限 {MAX_TEST_REROUTES}），停止：{}",
                        state.test_reroutes,
                        failures.join("；")
                    )));
                }
                // 回注带失败明细原文（含 path:line）——明细同源：coding 拿到的与日志面/失败原因文本一致
                let msg = crate::change::testing_gate::route_back_message("coding", &failures);
                tracing::warn!(component = "change-orchestrator", %msg, "测试失败回注 coding");
                // 写入失败上下文（下一轮 coding 的送达面）；超限分支不写——那里没有下一次 coding 轮
                let round = state.test_reroutes;
                // 测试→coding 回注：fix.start → 写入送达面 → fix.end（含回注轮次）
                fix_step(&state, "test-reroute", round, true);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.start",
                    round,
                    "测试回注 coding 开始"
                );
                state.set_test_failure(round, failures.clone());
                phase_change(&mut state, ChangePhase::Coding);
                save(run.store, &state)?;
                fix_step(&state, "test-reroute", round, false);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.end",
                    round,
                    "测试回注 coding 结束"
                );
                return Err(DevforgeError::Quality(format!(
                    "测试未通过，已回注 coding。{msg}"
                )));
            }
            TestingTurn::RoutePlan { failures } => {
                let msg = crate::change::testing_gate::route_back_message("plan", &failures);
                // 引擎审计修复 #5：testing→plan 回退与 plan 审查打回同一条额度管线——
                // issues 持久化进 state（resume 后 plan 循环读它走 fix_plan 而非重新
                // kickoff），计数复用 plan_fix_rounds（MAX_PLAN_FIXES cap 生效，
                // 不再无界循环）。
                state.last_plan_issues = failures.clone();
                state.plan_fix_rounds += 1;
                phase_change(&mut state, ChangePhase::Plan);
                save(run.store, &state)?;
                if state.plan_fix_rounds > MAX_PLAN_FIXES {
                    return Err(DevforgeError::Quality(format!(
                        "testing→plan 回退后计划修复已达上限 {MAX_PLAN_FIXES} 轮：{}",
                        failures.join("；")
                    )));
                }
                return Err(DevforgeError::Quality(format!(
                    "测试发现计划问题，已回注 plan。{msg}"
                )));
            }
        }
    }

    // ---- 报告（引擎写）----
    // 引擎审计修复 #7：diff 失败必须 Err（reverify #1 同类）——吞成空统计会让
    // 报告看起来像一次 no-op 改动，事后无从察觉 git 究竟坏在哪。
    let diff = report::diff_stats(run.repo, run.baseline_sha)?;
    let acceptance: AcceptanceSummary = summarize_acceptance(&criteria, &acceptance_done);
    let timeline = ChangeReport {
        change_id: run.change_id.to_string(),
        run_id: run.run_id.clone(),
        baseline_sha: state.baseline_sha.clone(),
        wall_secs: run.wall_secs,
        diff,
        review_rounds: state.review_rounds,
        roundtrips: state.roundtrips,
        usage: if run.usage.trim().is_empty() {
            "未记录".to_string()
        } else {
            run.usage.clone()
        },
        acceptance,
    };
    Ok(ChangeOutcome {
        state,
        report: timeline,
    })
}

#[cfg(test)]
mod orchestrator_tests;
