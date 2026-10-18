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

/// 无有效标记时的重推话术（零上限）。
pub const TESTING_CONTINUE_NUDGE: &str = "\
继续当前阶段。阶段 todo 全部完成、测试全绿（或发现全部已记录）、追溯矩阵同步后，\
在回复末尾单独一行输出 <stage-complete>N</stage-complete>（N 为阶段号）。";

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
    DocSideHandoff {
        /// 未通过审查的阶段号。
        stage: u64,
        /// 审查官列出的问题（doc-fix 的处置输入）。
        issues: Vec<String>,
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
        loop {
            match step {
                Step::Agent(message) => {
                    let text = self.agent.converse(&message, ui)?;
                    let Some(stage) = parse_stage_complete(&text) else {
                        // 无有效标记 → 重推（零上限）
                        step = Step::Agent(TESTING_CONTINUE_NUDGE.to_string());
                        continue;
                    };
                    step = Step::Review(stage);
                }
                Step::Review(stage) => {
                    report.review_rounds += 1;
                    let mut session = self.review_factory.open()?;
                    let review_text =
                        session.review(&testing_review_instruction(workspace, report.review_rounds))?;
                    drop(session);

                    let parsed = parse_test_review_report(&review_text);
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
                            if parsed.reroute == Reroute::DocSide {
                                report.doc_fix_handoffs += 1;
                                return Ok(TestingOutcome::DocSideHandoff {
                                    stage,
                                    issues: parsed.issues,
                                });
                            }
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
mod tests {
    use super::*;
    use crate::sop::planning::ReviewSession;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    struct NoUser;
    impl UserLink for NoUser {
        fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
            Err(crate::error::DevforgeError::System(format!(
                "测试不应有用户交互: {q}"
            )))
        }
    }

    enum Turn {
        Say(&'static str),
        ExpectContains(&'static str, &'static str),
    }
    impl Clone for Turn {
        fn clone(&self) -> Self {
            match self {
                Turn::Say(a) => Turn::Say(a),
                Turn::ExpectContains(a, b) => Turn::ExpectContains(a, b),
            }
        }
    }
    struct MockAgent {
        turns: VecDeque<Turn>,
        received: Rc<RefCell<Vec<String>>>,
    }
    impl MockAgent {
        fn new(turns: &[Turn]) -> Self {
            Self {
                turns: turns.iter().cloned().collect(),
                received: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for MockAgent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.received.borrow_mut().push(message.into());
            match self.turns.pop_front().expect("MockAgent 轮次耗尽") {
                Turn::Say(t) => Ok(t.into()),
                Turn::ExpectContains(needle, reply) => {
                    assert!(
                        message.contains(needle),
                        "应收到含 `{needle}` 的消息，实际: {message}"
                    );
                    Ok(reply.into())
                }
            }
        }
    }

    struct MockReview {
        reply: &'static str,
        closed: Rc<Cell<bool>>,
        expect_round: usize,
    }
    impl ReviewSession for MockReview {
        fn review(&mut self, instruction: &str) -> Result<String> {
            assert!(
                instruction.contains(&format!("第 {} 轮", self.expect_round)),
                "审查指令应带轮次号 {}: {instruction}",
                self.expect_round
            );
            Ok(self.reply.into())
        }
    }
    impl Drop for MockReview {
        fn drop(&mut self) {
            self.closed.set(true);
        }
    }
    #[derive(Default)]
    struct MockReviewFactory {
        replies: VecDeque<&'static str>,
        opened: Vec<Rc<Cell<bool>>>,
    }
    impl MockReviewFactory {
        fn new(replies: &[&'static str]) -> Self {
            Self {
                replies: replies.iter().copied().collect(),
                opened: Vec::new(),
            }
        }
    }
    impl ReviewSessionFactory for MockReviewFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            let closed = Rc::new(Cell::new(false));
            self.opened.push(closed.clone());
            let round = self.opened.len();
            Ok(Box::new(MockReview {
                reply: self.replies.pop_front().expect("Review 回复耗尽"),
                closed,
                expect_round: round,
            }))
        }
    }

    const PASS_ADVANCE: &str =
        "合格。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
    const PASS_FINISH: &str = "全覆盖。\n<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
    const FAIL_TEST_SIDE: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
        <reroute>test-side</reroute>\n<issues>\n- 缺超时用例\n</issues>";
    const FAIL_DOC_SIDE: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
        <reroute>doc-side</reroute>\n<issues>\n- 文档缺分页接口描述\n</issues>";

    #[test]
    fn kickoff_carries_dual_paths_and_doc_pointers() {
        let ws = Path::new("/tmp/ws-x");
        let k = testing_kickoff(ws, "demo", "t1");
        assert!(k.starts_with("你是测试 Agent：项目 `demo` / 任务 `t1`"));
        assert!(k.contains("/tmp/ws-x"));
        assert!(k.contains("coding/docs/"));
        assert!(k.contains("coverage-matrix"));
        assert!(testing_review_instruction(ws, 3).contains("第 3 轮"));
        assert!(testing_review_instruction(ws, 3).contains("/tmp/ws-x"));
    }

    #[test]
    fn two_stages_advance_then_finish() {
        // 正常推进：s1 完成→PASS+advance→s2 完成→PASS+finish 收工
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "进入阶段 2",
                "阶段 2 完成。\n<stage-complete>2</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[PASS_ADVANCE, PASS_FINISH]);
        let mut user = NoUser;
        let outcome =
            TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("应正常收工");
        };
        assert_eq!(report.stages_completed, vec![1, 2]);
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.issues_total, 0);
        assert_eq!(report.doc_fix_handoffs, 0);
        // 审查实例用完即弃
        assert!(factory.opened.iter().all(|c| c.get()));
    }

    #[test]
    fn missing_marker_gets_nudged() {
        // 无标记 → 回注重推（零上限）→ 补标记后 finish
        let agent = MockAgent::new(&[
            Turn::Say("还在写测试……"),
            Turn::ExpectContains(
                "<stage-complete>",
                "好了。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[PASS_FINISH]);
        let mut user = NoUser;
        let outcome =
            TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert!(matches!(outcome, TestingOutcome::Completed(_)));
    }

    #[test]
    fn fail_test_side_returns_to_agent() {
        // test-side 退回：问题清单回注活着的 Agent → 修复 → 重审 PASS
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "缺超时用例",
                "修好了。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_TEST_SIDE, PASS_FINISH]);
        let mut user = NoUser;
        let outcome =
            TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("应正常收工");
        };
        assert_eq!(report.stages_completed, vec![1]);
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.issues_total, 1);
        assert_eq!(report.doc_fix_handoffs, 0);
    }

    #[test]
    fn fail_doc_side_hands_off_and_resume_reviews_same_stage() {
        // doc-side 转移交 → 编排层跑 doc-fix → resume 对同阶段重审（轮次全局递增）
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_DOC_SIDE, PASS_FINISH]);
        let mut user = NoUser;
        let mut loop_ = TestingSopLoop::new(agent, &mut factory);
        let outcome = loop_.run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::DocSideHandoff { stage, issues } = outcome else {
            panic!("应移交 doc-fix");
        };
        assert_eq!(stage, 1);
        assert_eq!(issues, vec!["文档缺分页接口描述"]);

        // resume：审查轮次递增到 2（工厂断言指令带「第 2 轮」），PASS 后收工
        let report = TestingReport {
            stages_completed: vec![],
            review_rounds: 1,
            issues_total: 1,
            doc_fix_handoffs: 1,
        };
        let outcome = loop_
            .resume_after_docfix(&mut user, Path::new("/w"), stage, report)
            .unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("恢复后应收工");
        };
        assert_eq!(report.stages_completed, vec![1]);
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.doc_fix_handoffs, 1);
    }

    #[test]
    fn doc_side_can_repeat_after_resume() {
        // 恢复后再判 doc-side → 再移交（编排层可循环，零上限）
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_DOC_SIDE, FAIL_DOC_SIDE]);
        let mut user = NoUser;
        let mut loop_ = TestingSopLoop::new(agent, &mut factory);
        let first = loop_.run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::DocSideHandoff { stage, .. } = first else {
            panic!("首次应移交");
        };
        let second = loop_
            .resume_after_docfix(&mut user, Path::new("/w"), stage, TestingReport { review_rounds: 1, doc_fix_handoffs: 1, ..Default::default() })
            .unwrap();
        let TestingOutcome::DocSideHandoff { stage: stage2, .. } = second else {
            panic!("恢复后仍可再移交（零上限）");
        };
        assert_eq!(stage2, 1);
    }

    #[test]
    fn conservative_violations_route_to_test_side_not_handoff() {
        // 保守违规路由：协议块缺失/未知 reroute → 归一 test-side 退回，绝不触发移交
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "协议问题",
                "补协议了。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let no_verdict = "我说测试没问题。"; // 无协议块 → 保守 FAIL/TestSide
        let mut factory = MockReviewFactory::new(&[no_verdict, PASS_FINISH]);
        let mut user = NoUser;
        let outcome =
            TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("保守违规应走 test-side 退回而非移交");
        };
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.doc_fix_handoffs, 0);
    }

    #[test]
    fn unknown_reroute_value_defaults_test_side() {
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "缺超时用例",
                "修好了。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let junk_reroute = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
            <reroute>user-side</reroute>\n<issues>\n- 缺超时用例\n</issues>";
        let mut factory = MockReviewFactory::new(&[junk_reroute, PASS_FINISH]);
        let mut user = NoUser;
        let outcome =
            TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("未知 reroute 应保守归 test-side 退回");
        };
        assert_eq!(report.doc_fix_handoffs, 0);
    }
}
