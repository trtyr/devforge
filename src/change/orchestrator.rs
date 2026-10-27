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

/// 编码审查打回后的回注修复上限（change #4：旧实现编码审查 FAIL 直接判 Quality，
/// 与 MVP 的 FAIL→回注→重审节奏不一致；超限才判质量失败，不自动重启）。
pub const MAX_CODING_FIXES: usize = 3;

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
    /// 编码审查的失败明细（change #4）：非空 = 审查 FAIL，编排层走有界回注循环；
    /// 空 = 审查通过（或本轮没走到审查，如 `<route>plan</route>`）。
    pub review_issues: Vec<String>,
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
    /// `reason` 是 coding 的回退散文理由（可能为空）。
    fn amend_plan(
        &mut self,
        ui: &mut dyn UserLink,
        gaps: &[ImpactGap],
        roundtrip: usize,
        reason: &str,
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

    /// **本次调用**新开的 plan 审查实例数（不是累计轮次——编排层自己按轮累加；
    /// 自审 minor 18：旧文案写「消耗的审查轮次」，实现若改成累计值就会双计）。
    /// 默认 0（脚本化 session 不审）。
    fn plan_reviews(&self) -> usize {
        0
    }

    /// 编码审查打回后的回注：把审查失败明细交给 coding agent 逐条修复。
    ///
    /// 默认实现**显式失败**（自审 minor 71：旧口径静默 no-op 返回 Ok，编排层却把这当作
    /// 「修复已送达」落 fix.end 并消耗持久化额度——额度烧完了问题根本没送给 agent）。
    /// 脚本化 session 不产 review issues，因此不会走到这里。
    fn fix_coding(&mut self, ui: &mut dyn UserLink, issues: &[String], round: usize) -> Result<()> {
        let _ = (ui, issues, round);
        Err(DevforgeError::Agent {
            role: "change-coding".into(),
            message: "该会话未实现 fix_coding（编码审查回注无法送达）".into(),
        })
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


/// 判终态（自审 minor 72/74）：把原因写进断点（续跑立即失败）+ 落结构化事件，
/// 再返回质量失败。所有「判死」路径都走它——事件流才能以终态结论收尾，
/// 而不是以 fix.end/phase.change 结尾、失败原因只活在返回值里。
fn terminate<S: ChangeSession>(
    run: &ChangeRun<'_>,
    state: &mut crate::change::state::ChangeState,
    reason: String,
) -> crate::error::Result<ChangeOutcome> {
    let _ = std::marker::PhantomData::<S>;
    state.terminal_reason = Some(reason.clone());
    if let Err(e) = save(run.store, state) {
        tracing::error!(
            component = "change-orchestrator",
            change_id = %run.change_id,
            error = %e,
            "终态原因落盘失败（续跑可能重跑一整段后再次撞同一上限）"
        );
    }
    tracing::error!(
        component = "change-orchestrator",
        event = "change.terminal",
        change_id = %run.change_id,
        reason = %reason,
        "change 判终态"
    );
    Err(DevforgeError::Quality(reason))
}

/// 运行入口（外层包一层终态日志：任何出口都留一条结论事件）。
pub fn run_with_terminal_log<S: ChangeSession>(
    input: ChangeRun<'_>,
    session: &mut S,
    ui: &mut dyn UserLink,
) -> Result<ChangeOutcome> {
    let change_id_for_log = input.change_id.to_string();
    let out = run(input, session, ui);
    match &out {
        Ok(o) => tracing::info!(
            component = "change-orchestrator",
            event = "change.outcome",
            change_id = %change_id_for_log,
            phase = %o.state.phase.as_str(),
            "change 收尾（成功出口）"
        ),
        Err(e) => {
            let root = e.root_cause();
            let kind = if root.is_quality_failure() {
                "quality"
            } else if root.is_agent_failure() {
                "agent"
            } else {
                "infra"
            };
            tracing::error!(
                component = "change-orchestrator",
                event = "change.failed",
                change_id = %change_id_for_log,
                kind,
                error = %e,
                "change 收尾（失败出口）"
            );
        }
    }
    out
}

/// 跑完整条 change 流程（从断点继续）。
pub fn run<S: ChangeSession>(
    run: ChangeRun<'_>,
    session: &mut S,
    ui: &mut dyn UserLink,
) -> Result<ChangeOutcome> {
    let mut state = load_strict(run.store, run.change_id, run.baseline_sha)?;
    if state.baseline_sha.is_empty() {
        state.baseline_sha = run.baseline_sha.to_string();
    }
    // 自审 minor（74）：已判终态的变更不得再重跑一整段（旧口径只有往返上限会先落盘比较，
    // 其余上限撞线时什么都不写 → 续跑重跑 coding 再撞同一个上限）。
    if let Some(reason) = &state.terminal_reason {
        tracing::error!(
            component = "change-orchestrator",
            event = "change.terminal",
            change_id = %run.change_id,
            reason = %reason,
            "该变更已判终态，拒绝继续续跑（确需重跑请带 fresh:true）"
        );
        return Err(DevforgeError::Quality(format!(
            "该变更已判终态（{reason}）——拒绝重跑；确需重跑请带 fresh:true"
        )));
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
            let reason = format!(
                "计划修复轮已达上限 {MAX_PLAN_FIXES} 轮（断点 plan_fix_rounds={fix_round}），拒绝继续修复：{}",
                issues.join("；")
            );
            return terminate::<S>(&run, &mut state, reason);
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
                        let reason = format!(
                            "计划审查连续 {fix_round} 轮未通过（上限 {MAX_PLAN_FIXES} 轮修复）：{}",
                            next.join("；")
                        );
                        issues = next;
                        return terminate::<S>(&run, &mut state, reason);
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
            // change #4：编码审查 FAIL 走有界回注循环（旧实现直接 `Err(Quality)`，
            // 与 MVP 的 FAIL→回注→重审节奏不一致，一次小毛病就判死整个变更）。
            let review_issues = turn.review_issues.clone();
            if !review_issues.is_empty() {
                let next = state.coding_fix_rounds + 1;
                state.coding_fix_rounds = next;
                save(run.store, &state)?;
                if next > MAX_CODING_FIXES {
                    let reason = format!(
                        "编码审查连续 {next} 轮未通过（上限 {MAX_CODING_FIXES} 轮修复）：{}",
                        review_issues.join("；")
                    );
                    return terminate::<S>(&run, &mut state, reason);
                }
                fix_step(&state, "coding", next, true);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.start",
                    round = next,
                    "编码审查回注开始"
                );
                session.fix_coding(ui, &review_issues, next)?;
                fix_step(&state, "coding", next, false);
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.end",
                    round = next,
                    "编码审查回注结束"
                );
                continue;
            }
            match boundary::classify(&turn.changed, &whitelist, &turn.reply) {
                BoundaryVerdict::InBoundary => {
                    // 自审 major：边界门是五道机械门里唯一不落结构化事件的——
                    // 三种裁决在 runlog 里都不可重建。三种裁决各落一条。
                    tracing::info!(
                        component = "change-orchestrator",
                        event = "boundary.verdict",
                        verdict = "in_boundary",
                        "边界门：改动全在白名单内"
                    );
                    // 消费即清：coding 一旦过门，上一轮失败上下文就过期了
                    state.clear_test_failure();
                    phase_change(&mut state, ChangePhase::Testing);
                    save(run.store, &state)?;
                    break;
                }
                BoundaryVerdict::OutOfBoundary { extra } => {
                    tracing::info!(
                        component = "change-orchestrator",
                        event = "boundary.verdict",
                        verdict = "out_of_boundary",
                        extra = ?extra,
                        "边界门：越界未申报（范围膨胀）"
                    );
                    let reason = format!("越界改动未申报（范围膨胀）：{}", extra.join("、"));
                    return terminate::<S>(&run, &mut state, reason);
                }
                BoundaryVerdict::RoutePlan { gaps, reason } => {
                    tracing::info!(
                        component = "change-orchestrator",
                        event = "boundary.verdict",
                        verdict = "route_plan",
                        gaps = gaps.len(),
                        "边界门：越界已申报，回 plan 补边界"
                    );
                    let next = state.roundtrips + 1;
                    boundary::check_roundtrip(next)?;
                    state.bump_roundtrip();
                    // 自审 major：先持久化计数再写历史——旧行序里历史写失败会让
                    // Err 抢在 save 之前上抛，往返计数丢失，MAX_ROUNDTRIPS 上限
                    // 可被自动重启绕过。计数落盘后历史写失败仍 Err（审计链不静默）。
                    save(run.store, &state)?;
                    // 每次回退都落历史（T013：事后能看出边界是怎么被撑大的）
                    crate::change::history::append_roundtrip(
                        run.change_dir,
                        run.change_id,
                        state.roundtrips,
                        &gaps,
                    )?;
                    // 自审 minor（16）：旧口径在这里 merge 白名单，但 amend 成功后整份白名单被
                    // 新计划覆盖（`whitelist = w`）——merge 的产出被唯一消费方丢弃，「补完边界后
                    // 重新校验」在生产上不成立。现在合并挪到「拿到新计划之后」，并按归一化路径
                    // 校验申报缺口是否真的被写进新白名单（漏了要留痕）。
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
                    let amend = session.amend_plan(ui, &gaps, state.roundtrips, &reason)?;
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
                                // 申报过的文件必须进新白名单（缺 = 计划没补全边界，留痕后仍然
                                // 合并进去：coding 已经在这些文件上动过，回退成本高于放行）。
                                for g in &gaps {
                                    let norm = g.file.trim_start_matches("./");
                                    if !w.iter().any(|x| x.trim_start_matches("./") == norm) {
                                        tracing::warn!(
                                            component = "change-orchestrator",
                                            event = "boundary.gap_not_in_new_whitelist",
                                            file = g.file.as_str(),
                                            "补边界后的新计划白名单仍缺申报文件——已合并兜底"
                                        );
                                    }
                                }
                                whitelist = w;
                            }
                            boundary::merge_gaps_into_whitelist(&mut whitelist, &gaps);
                            save(run.store, &state)?;
                            continue;
                        }
                        PlanTurn::Fail { issues } => {
                            let reason = format!("补边界后计划审查仍未过：{}", issues.join("；"));
                            return terminate::<S>(&run, &mut state, reason);
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
                    return terminate::<S>(
                        &run,
                        &mut state,
                        "计划里没有可对账的验收标准——plan_facts 只认「验收标准」块里的列表项，\
                         用表格/段落写法会抽不到，请改用列表并在 plan 阶段重新过审"
                            .into(),
                    );
                }
                let summary = summarize_acceptance(&criteria, &acceptance_done);
                if !summary.passed() {
                    let reason = format!(
                        "验收对账缺 {} 条：{}",
                        summary.missing.len(),
                        summary.missing.join("；")
                    );
                    return terminate::<S>(&run, &mut state, reason);
                }
                phase_change(&mut state, ChangePhase::Done);
                // 终态清：收尾不留过期状态
                state.clear_test_failure();
                save(run.store, &state)?;
            }
            TestingTurn::RouteCoding { failures } => {
                state.bump_test_reroute();
                if state.test_reroutes > MAX_TEST_REROUTES {
                    let reason = format!(
                        "测试→coding 已回注 {} 次（上限 {MAX_TEST_REROUTES}），停止：{}",
                        state.test_reroutes,
                        failures.join("；")
                    );
                    return terminate::<S>(&run, &mut state, reason);
                }
                // 回注带失败明细原文（含 path:line）——明细同源：coding 拿到的与日志面/失败原因文本一致
                let msg = crate::change::testing_gate::route_back_message("coding", &failures);
                tracing::warn!(
                    component = "change-orchestrator",
                    event = "fix.reinject",
                    stage = "testing→coding",
                    failures = ?failures,
                    %msg,
                    "测试失败回注 coding"
                );
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
                // 自审 minor（76）：testing→plan 与 testing→coding 同级的回退，旧口径既没有
                // fix.start/fix.end 事件对，也没有失败明细日志——事件流无法重建回退原因。
                tracing::warn!(
                    component = "change-orchestrator",
                    event = "fix.start",
                    stage = "testing→plan",
                    round = state.plan_fix_rounds + 1,
                    failures = ?failures,
                    "测试发现计划问题，回注 plan 开始"
                );
                // 引擎审计修复 #5：testing→plan 回退与 plan 审查打回同一条额度管线——
                // issues 持久化进 state（resume 后 plan 循环读它走 fix_plan 而非重新
                // kickoff），计数复用 plan_fix_rounds（MAX_PLAN_FIXES cap 生效，
                // 不再无界循环）。
                state.last_plan_issues = failures.clone();
                state.plan_fix_rounds += 1;
                phase_change(&mut state, ChangePhase::Plan);
                save(run.store, &state)?;
                tracing::info!(
                    component = "change-orchestrator",
                    event = "fix.end",
                    stage = "testing→plan",
                    round = state.plan_fix_rounds,
                    "测试发现计划问题，回注 plan 结束（本次 run 到此收束）"
                );
                if state.plan_fix_rounds > MAX_PLAN_FIXES {
                    let reason = format!(
                        "testing→plan 回退后计划修复已达上限 {MAX_PLAN_FIXES} 轮：{}",
                        failures.join("；")
                    );
                    return terminate::<S>(&run, &mut state, reason);
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
    // 自审 minor（77）：报告里的「基线」字段与 diff 锚点必须**同源**——旧口径 diff 用
    // run.baseline_sha、报告字段用 state.baseline_sha，两者背离时报告所述基线与实际 diff
    // 口径不符。统一用断点里的基线（门 0 记下后全程不动）。
    let diff = report::diff_stats(run.repo, &state.baseline_sha)?;
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
#[path = "orchestrator_tests.rs"]
mod orchestrator_tests;
