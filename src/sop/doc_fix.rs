//! 文档修补循环状态机（P007 · D004 写文档模式 + D008 doc-review 内置审查轮）。
//!
//! ```text
//! DOC_FIX（Doc Fix Agent 单轮工作：读 findings(doc-side) → 逐条修文档 → 回写处置摘要）
//!   → doc-review 内置审查（每轮全新实例，D008：一致性/自洽/Runbook 契约/findings 完整性）
//!   → PASS → 返回（编排层恢复 testing，D004 流回测试环节）
//!   → FAIL → 问题清单回注 Agent → 修订 → 重审（零上限）
//! ```
//!
//! 与 CodingSopLoop 的差异：无阶段推进（doc-fix 是定点修补，不是分阶段开发），
//! 审查裁决只有 PASS/FAIL（无 advance/finish，协议见 testing_protocol::parse_doc_review_report）。

use std::path::Path;

use crate::error::Result;
use crate::sop::coding_protocol::StageVerdict;
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};
use crate::sop::testing_protocol::{parse_doc_review_report, FINDINGS_DIR};

/// Doc Fix Agent 开场指令：黑板（cwd）+ 代码工作区 + findings 位置 + 待补文档位置（D004）。
pub fn docfix_kickoff(workspace: &Path, project: &str, task: &str) -> String {
    format!(
        "你是文档修补 Agent：项目 `{project}` / 任务 `{task}`。\
\n当前目录是共享黑板（任务目录）：测试审查判定文档不足（doc-side），由你按发现补齐文档。\
\n代码工作区（triage 时允许读代码核实事实）：{}\n\
findings 清单在 {}（处置 doc-side 项），待补文档在 coding/docs/。\
逐条处置：修文档、补 Runbook、回写处置摘要。本轮只改文档，不改代码。",
        workspace.display(),
        FINDINGS_DIR
    )
}

/// doc-review 审查指令：全新实例 kickoff（黑板与工作区路径 + 轮次号）。
pub fn doc_review_instruction(workspace: &Path, round: usize) -> String {
    format!(
        "你是文档审查 Agent（第 {round} 轮）。\
\n黑板 = 当前目录（plan tree、coding/docs/ 与 testing/findings/ 都在这）；代码工作区 = {}\n\
按你的提示词执行全部审查面：findings 处置完整性、plan tree 一致性、内部自洽、Runbook 七件套，\
先写 testing/reports/doc-review-{round}.md，再在回复末尾输出协议块（stage-verdict / issues / notes）。",
        workspace.display()
    )
}

/// 打回回注前缀（doc-review FAIL）。
pub const DOC_FIX_REMEDIATION_PREFIX: &str = "文档审查未通过，以下是必须修复的问题：";

/// 协议违例重试上限（C19-R1）：超过即判基础设施故障，快速报错。
pub const MAX_DOC_PROTOCOL_VIOLATIONS: usize = 3;

/// 文档修补 SOP 报告。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocFixReport {
    /// doc-review 审查轮数（含首轮；打回重审累计）。
    pub review_rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
}

pub struct DocFixSopLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    /// Doc Fix Agent 会话（打回重审在同一会话继续——保活语义沿 coding 家族）。
    pub agent: P,
    /// doc-review 实例工厂（每轮全新，用完即弃）。
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> DocFixSopLoop<P, F> {
    pub fn new(agent: P, review_factory: F) -> Self {
        Self {
            agent,
            review_factory,
        }
    }

    /// 跑完文档修补 SOP 直至 doc-review 判 PASS（零上限：打回轮次无上限）。
    pub fn run(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        project: &str,
        task: &str,
    ) -> Result<DocFixReport> {
        let mut report = DocFixReport::default();

        // 单轮工作：按 findings 修文档（完成即止，无阶段协议）
        let message = docfix_kickoff(workspace, project, task);
        self.agent.converse(&message, ui)?;

        // 审查循环：PASS 出口，FAIL 打回（回注问题清单）后重审
        let mut violations = 0usize;
        loop {
            report.review_rounds += 1;
            let mut session = self.review_factory.open()?;
            let review_text =
                session.review(&doc_review_instruction(workspace, report.review_rounds))?;
            drop(session);

            let parsed = parse_doc_review_report(&review_text);
            // C19-R1：审查输出本身不合规 → 重跑审查，不回注 doc-fix Agent
            if parsed.protocol_violation {
                violations += 1;
                if violations > MAX_DOC_PROTOCOL_VIOLATIONS {
                    return Err(crate::error::DevforgeError::Quality(format!(
                        "文档审查连续 {} 轮输出协议违例（缺 stage-verdict），判定为质量失败（不自动重启），停止 run：{}",
                        violations,
                        parsed.issues.join("；")
                    )));
                }
                continue;
            }
            match parsed.verdict {
                StageVerdict::Pass => return Ok(report),
                StageVerdict::Fail => {
                    report.issues_total += parsed.issues.len();
                    let remediation = format!(
                        "{DOC_FIX_REMEDIATION_PREFIX}\n{}",
                        parsed
                            .issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    self.agent.converse(&remediation, ui)?;
                }
            }
        }
    }
}

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

    struct MockAgent {
        turns: VecDeque<&'static str>,
        received: Rc<RefCell<Vec<String>>>,
    }
    impl MockAgent {
        fn new(turns: &[&'static str]) -> Self {
            Self {
                turns: turns.iter().copied().collect(),
                received: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for MockAgent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.received.borrow_mut().push(message.into());
            Ok(self.turns.pop_front().expect("MockAgent 轮次耗尽").into())
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

    const REVIEW_PASS: &str =
        "齐了。\n<stage-verdict>PASS</stage-verdict>\n<notes>- 建议补端口说明</notes>";
    const REVIEW_FAIL_MISSING_SEED: &str = "<stage-verdict>FAIL</stage-verdict>\n\
        <issues>\n- Runbook 缺种子数据节\n</issues>";
    const NO_VERDICT: &str = "我觉得差不多了。"; // 无协议块 → 保守 FAIL

    #[test]
    fn kickoff_carries_paths_and_findings_pointers() {
        let ws = Path::new("/tmp/ws-x");
        let k = docfix_kickoff(ws, "demo", "t1");
        assert!(k.starts_with("你是文档修补 Agent：项目 `demo` / 任务 `t1`"));
        assert!(k.contains("/tmp/ws-x"));
        assert!(k.contains(FINDINGS_DIR));
        assert!(k.contains("coding/docs/"));
        assert!(k.contains("只改文档，不改代码"));
        let r = doc_review_instruction(ws, 2);
        assert!(r.contains("第 2 轮"));
        assert!(r.contains("/tmp/ws-x"));
    }

    #[test]
    fn pass_on_first_review_round() {
        let agent = MockAgent::new(&["补完文档了。"]);
        let mut factory = MockReviewFactory::new(&[REVIEW_PASS]);
        let mut user = NoUser;
        let report =
            DocFixSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.review_rounds, 1);
        assert_eq!(report.issues_total, 0);
        assert!(factory.opened.iter().all(|c| c.get()));
    }

    #[test]
    fn review_reject_reinjects_issues_then_passes() {
        // doc-review 打回路径：问题清单回注 Agent → 修订 → 重审 PASS
        let agent = MockAgent::new(&["补了一版。", "又补了一版。"]);
        let received = agent.received.clone();
        let mut factory = MockReviewFactory::new(&[REVIEW_FAIL_MISSING_SEED, REVIEW_PASS]);
        let mut user = NoUser;
        let report =
            DocFixSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.issues_total, 1);
        // 第二条消息是打回回注，含问题原文与前缀
        let received = received.borrow();
        assert_eq!(received.len(), 2);
        assert!(received[1].contains(DOC_FIX_REMEDIATION_PREFIX));
        assert!(received[1].contains("Runbook 缺种子数据节"));
    }

    #[test]
    fn protocol_violation_retries_review_not_agent() {
        // C19-R1：doc-review 输出协议块缺失 → 重跑审查，不回注 doc-fix Agent
        let agent = MockAgent::new(&["改完了。"]);
        let received = agent.received.clone();
        let mut factory = MockReviewFactory::new(&[NO_VERDICT, REVIEW_PASS]);
        let mut user = NoUser;
        let report =
            DocFixSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.review_rounds, 2, "违例后重跑第二轮审查");
        assert_eq!(report.issues_total, 0, "违例不计入文档问题数");
        let received = received.borrow();
        assert_eq!(received.len(), 1, "只收到 kickoff，没收到违例回注");
    }

    #[test]
    fn repeated_protocol_violations_fail_fast() {
        // 连续违例超上限 → 基础设施故障报错，不无限重试
        let agent = MockAgent::new(&["改完了。"]);
        let mut factory = MockReviewFactory::new(&[NO_VERDICT, NO_VERDICT, NO_VERDICT, NO_VERDICT]);
        let mut user = NoUser;
        let err = DocFixSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap_err();
        assert!(err.to_string().contains("协议违例"), "应报基础设施故障: {err}");
    }

    #[test]
    fn repeated_rejects_zero_cap() {
        // 零上限：连打两轮仍继续，第三轮 PASS 收工
        let agent = MockAgent::new(&["v1", "v2", "v3"]);
        let mut factory = MockReviewFactory::new(&[
            REVIEW_FAIL_MISSING_SEED,
            REVIEW_FAIL_MISSING_SEED,
            REVIEW_PASS,
        ]);
        let mut user = NoUser;
        let report =
            DocFixSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.review_rounds, 3);
        assert_eq!(report.issues_total, 2);
    }
}
