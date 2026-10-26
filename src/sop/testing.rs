//! 测试循环状态机（P007 · decisions/D001-D012）。
//!
//! ```text
//! TESTING（Test Agent 长驻：读文档→自判 N 阶段→逐阶段写测/自起自跑/落报告+矩阵）
//!   → <stage-complete>N</stage-complete> → 本轮审查
//!   → 无有效标记 → 回注「继续」重推（零上限，沿 coding decisions/003 家族）
//! REVIEWING（每轮全新 Test Review 实例，从头查）
//!   → FAIL + reroute=test-side → 问题清单回注活着的 Agent → 修完重发标记 → 重审
//!   → FAIL + reroute=doc-side  → 返回 DocSideHandoff（编排层触发 doc-fix 子 SOP，
//!                                 完成后 resume_after_docfix 对同阶段重审）
//!   → PASS+advance → 消化 <notes> → 进入阶段 N+1（同一会话继续）
//!   → PASS+finish  → 文档全覆盖 → 收工
//! ```
//!
//! 归因权在审查侧（D001），测试 Agent 只交中性发现；reroute 由协议标签驱动（D003），
//! 保守缺省 test-side（宁退回不滥升级）。零硬限制：阶段数、重推、打回轮次均无上限。

use std::path::Path;

use crate::error::Result;
use crate::sop::coding_protocol::{parse_stage_complete, NextAction};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};
use crate::sop::review_gate::{review_with_gate, ReviewBudget, ReviewOutcome};
use crate::sop::testing_protocol::{parse_test_review_report, Reroute};

/// Test Agent 开场指令：黑板（cwd）与代码工作区双路径告知 + 文档指针（D006）。
pub fn testing_kickoff(workspace: &Path, project: &str, task: &str) -> String {
    format!(
        "你是测试 Agent：项目 `{project}` / 任务 `{task}`。\
\n当前目录是共享黑板（任务目录）：读这里的 plan tree 与 coding/docs/ 伴写文档，按你的提示词开工。\
\n代码工作区（被测系统与 tests/ 落点，decisions/D010）：{}\n\
先通读文档、再自划分测试阶段（写 testing/stages.md），然后逐阶段写测、自起被测系统、自跑自收。\
findings 写 testing/findings/，追溯矩阵写 testing/coverage-matrix.md。",
        workspace.display()
    )
}

/// 审查指令：全新 Test Review 实例的 kickoff（黑板与工作区路径 + 轮次号）。
pub fn testing_review_instruction(workspace: &Path, round: usize) -> String {
    format!(
        "你是测试审查 Agent（第 {round} 轮）。\
\n黑板 = 当前目录（plan tree、coding/docs/ 与 testing/ 子树都在这）；代码工作区 = {}\n\
按你的提示词执行全部审查面：真读测试、真跑测试、审计追溯矩阵，先写 testing/reports/review-{round}.md，\
再在回复末尾输出协议块（stage-verdict / next / FAIL 时 reroute + issues / PASS 时 notes）。",
        workspace.display()
    )
}

/// 无有效标记时的重推话术（有界，007 D013：连续 3 次不发标记 → 第 4 次判质量失败）。
pub const TESTING_CONTINUE_NUDGE: &str = "\
继续当前阶段。阶段 todo 全部完成、测试全绿（或发现全部已记录）、追溯矩阵同步后，\
在回复末尾单独一行输出 <stage-complete>N</stage-complete>（N 为阶段号）。";

/// 无标记重推上限：超过即质量失败（不自动重启，plan 010 D004）。
pub const MAX_TEST_CONTINUE_NUDGES: usize = 3;

/// 打回回注前缀（FAIL + reroute=test-side）。
pub const TESTING_REMEDIATION_PREFIX: &str = "审查未通过（test-side），以下是必须修复的问题：";

/// PASS+advance 的推进话术（附非阻塞建议先消化）。
fn testing_advance_message(stage: u64, notes: &[String]) -> String {
    if notes.is_empty() {
        return format!("阶段 {stage} 审查通过，进入阶段 {}。", stage + 1);
    }
    let mut msg = format!(
        "阶段 {stage} 审查通过。先消化以下非阻塞建议再开始阶段 {}：",
        stage + 1
    );
    for n in notes {
        msg.push_str(&format!("\n- {n}"));
    }
    msg
}

/// 协议违例重试上限（C19-R1）：审查输出不合规时重跑审查而不是回注被测 Agent。
/// 单一事实源在 [`crate::sop::review_gate`]（D026），此处只做别名。
pub const MAX_TEST_PROTOCOL_VIOLATIONS: usize = crate::sop::review_gate::MAX_PROTOCOL_VIOLATIONS;

/// 测试 SOP 报告 / 进度账本（移交恢复时由编排层持有并回传）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestingReport {
    /// 完成并通过审查的阶段号（升序）。
    pub stages_completed: Vec<u64>,
    /// Review 轮数（全局计数，跨移交累计）。
    pub review_rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
    /// doc-side 移交次数（触发 doc-fix 子 SOP 的次数）。
    pub doc_fix_handoffs: usize,
}

/// 循环出口：正常收工，或需要编排层跑 doc-fix 后恢复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestingOutcome {
    /// 文档全覆盖，收工。
    Completed(TestingReport),
    /// 审查判定 doc-side：编排层应触发 doc-fix 子 SOP，完成后调
    /// [`TestingSopLoop::resume_after_docfix`] 对同阶段重审（Agent 会话保持存活）。
    /// `report` 是最新累计账本（轮次/问题数全局计数），编排层必须原样传回 resume。
    DocSideHandoff {
        /// 未通过审查的阶段号。
        stage: u64,
        /// 审查官列出的问题（doc-fix 的处置输入）。
        issues: Vec<String>,
        /// 最新累计账本（编排层回传 resume 的凭据）。
        report: TestingReport,
    },
}

enum Step {
    /// 与 Agent 对话（期待 stage-complete）。
    Agent(String),
    /// 对该阶段跑一轮全新审查。
    Review(u64),
}

pub struct TestingSopLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    /// Test Agent 会话（长驻，跨全部阶段/审查轮/doc-fix 移交——D006 保活语义）。
    pub agent: P,
    /// 审查实例工厂（每轮全新，用完即弃）。
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> TestingSopLoop<P, F> {
    pub fn new(agent: P, review_factory: F) -> Self {
        Self {
            agent,
            review_factory,
        }
    }

    /// 跑完测试 SOP 直至审查判定 finish（文档全覆盖）。
    pub fn run(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        project: &str,
        task: &str,
    ) -> Result<TestingOutcome> {
        let message = testing_kickoff(workspace, project, task);
        self.drive(ui, workspace, Step::Agent(message), TestingReport::default())
    }
    /// doc-fix 子 SOP 完成后恢复：对同一阶段发起全新审查（轮次全局递增）。
    ///
    /// 编排层（引擎）持有 [`TestingReport`] 账本，移交-恢复可循环多次
    /// （恢复后再判 doc-side → 再移交 → 再恢复，零上限）。
    pub fn resume_after_docfix(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        stage: u64,
        report: TestingReport,
    ) -> Result<TestingOutcome> {
        self.drive(ui, workspace, Step::Review(stage), report)
    }

    fn drive(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        mut step: Step,
        mut report: TestingReport,
    ) -> Result<TestingOutcome> {
        // 审查预算的生命周期 = 一次 drive（与既有违例计数同口径）
        let mut budget = ReviewBudget::default();
        let mut nudges = 0usize;
        // 全仓审计 #7：FAIL 但 issues 为空的追补计数（与 change 测试回注同口径）
        let mut empty_issue_nudges = 0usize;
        // 阶段推进记录：上一个阶段号（首个阶段的前驱 = "start"）
        let mut last_stage: Option<u64> = None;
        // 被打回后待收尾的修复轮（回注在下一轮 agent 回合才发生 → fix.end 落在那一回合返回处）
        let mut pending_fix: Option<u32> = None;
        loop {
            match step {
                Step::Agent(message) => {
                    let text = self.agent.converse(&message, ui)?;
                    // 上一轮回注已随本回合交卷 → 修复轮收尾
                    if let Some(round) = pending_fix.take() {
                        tracing::info!(
                            component = "testing-engine",
                            event = "fix.end",
                            round,
                            "打回重写结束"
                        );
                    }
                    let Some(stage) = parse_stage_complete(&text) else {
                        // 无有效标记 → 有界重推（007 D013）
                        nudges += 1;
                        if nudges > MAX_TEST_CONTINUE_NUDGES {
                            return Err(crate::error::DevforgeError::Quality(format!(
                                "测试 Agent 连续 {nudges} 次未输出 <stage-complete>，判定为质量失败（不自动重启）；最近一次回复尾巴：{}",
                                crate::sop::coding::tail(&text)
                            )));
                        }
                        step = Step::Agent(TESTING_CONTINUE_NUDGE.to_string());
                        continue;
                    };
                    nudges = 0;
                    // 阶段推进：agent 报出的新阶段号（前一个阶段 → 当前阶段）。
                    // 重发同一阶段号（打回后重交）**不是**推进 → 不落里程碑（from/to 同值 = 噪声）。
                    if last_stage != Some(stage) {
                        tracing::info!(
                            component = "testing-engine",
                            event = "phase.change",
                            from = last_stage.map(|s| s.to_string()).unwrap_or_else(|| "start".into()),
                            to = stage.to_string(),
                            "MVP testing 阶段推进"
                        );
                        last_stage = Some(stage);
                    }
                    step = Step::Review(stage);
                }
                Step::Review(stage) => {
                    // C19-R1 + D026：协议违例（审查输出本身不合规）→ 换新实例重审，不回注被测 Agent；
                    // 实例失败（空回复/早退/通道错误）→ 换新实例重试。
                    // 审查轮次口径（Q6）：`report.review_rounds` 是**已开过的实例数** → 本轮序号 = report.review_rounds + 1（首审 = 1）。
                    let review_round = (report.review_rounds + 1) as u32;
                    // 「审查开庭」里程碑：在**发起**那一刻落（不是等审完）
                    tracing::info!(
                        component = "testing-engine",
                        event = "review.open",
                        role = "test-review",
                        round = review_round,
                        "审查开庭"
                    );
                    let (parsed, opened) = review_with_gate(
                        &mut self.review_factory,
                        report.review_rounds,
                        |round| testing_review_instruction(workspace, round),
                        &mut budget,
                        |text| {
                            let r = parse_test_review_report(text);
                            let reason = if r.protocol_violation {
                                Some(r.issues.join("；"))
                            } else {
                                None
                            };
                            Ok(ReviewOutcome {
                                report: r,
                                violation: reason,
                            })
                        },
                    )?;
                    report.review_rounds += opened;
                    match (parsed.verdict, parsed.next) {
                        (StageVerdictRe::Pass, NextAction::Finish) => {
                            report.stages_completed.push(stage);
                            return Ok(TestingOutcome::Completed(report));
                        }
                        (StageVerdictRe::Pass, NextAction::Advance) => {
                            report.stages_completed.push(stage);
                            step = Step::Agent(testing_advance_message(stage, &parsed.notes));
                        }
                        // (Fail, Fix)——其余组合已被解析器保守归一为 (Fail, Fix)
                        _ => {
                            report.issues_total += parsed.issues.len();
                            // 全仓审计 #7：FAIL 但不给明细 = 空箱回注（agent 盲改、可无界空转）。
                            // 拒收 + 追补，连续超限判质量失败——与 change 测试回注同一口径。
                            if parsed.issues.is_empty() {
                                empty_issue_nudges += 1;
                                if empty_issue_nudges
                                    > crate::sop::review_gate::MAX_EMPTY_ISSUES_NUDGES
                                {
                                    return Err(crate::error::DevforgeError::Quality(format!(
                                        "测试审查连续 {empty_issue_nudges} 次判定 FAIL 但未给出 issues 明细，判定为质量失败（不自动重启）"
                                    )));
                                }
                                step = Step::Agent(
                                    crate::sop::review_gate::ISSUES_NUDGE.to_string(),
                                );
                                continue;
                            }
                            empty_issue_nudges = 0;
                            if parsed.reroute == Reroute::DocSide {
                                report.doc_fix_handoffs += 1;
                                // 阶段推进：测试侧 → 文档侧（移交 doc-fix）
                                tracing::info!(
                                    component = "testing-engine",
                                    event = "phase.change",
                                    from = "test",
                                    to = "doc",
                                    "MVP testing 移交文档侧"
                                );
                                return Ok(TestingOutcome::DocSideHandoff {
                                    stage,
                                    issues: parsed.issues,
                                    report,
                                });
                            }
                            // 打回回注：fix 起止一对（含轮次）——回注在下一轮 agent 回合发生
                            tracing::info!(
                                component = "testing-engine",
                                event = "fix.start",
                                round = review_round,
                                "打回重写开始"
                            );
                            pending_fix = Some(review_round);
                            step = Step::Agent(format!(
                                "{TESTING_REMEDIATION_PREFIX}\n{}",
                                parsed
                                    .issues
                                    .iter()
                                    .map(|i| format!("- {i}"))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ));
                        }
                    }
                }
            }
        }
    }
}

// StageVerdict 在 match 中以 re-export 别名引用，避免与本文件其他符号混淆
use crate::sop::coding_protocol::StageVerdict as StageVerdictRe;

#[cfg(test)]
#[path = "testing_tests.rs"]
mod tests;
