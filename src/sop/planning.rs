//! 规划循环状态机（P001 · decisions/008 零校验流转 + decisions/004 Review 无状态）。
//!
//! 状态流转：
//! ```text
//! PLAN_EDITING（Plan Agent 与用户对话，强制 ask-user-question 确认完成）
//!   → 完成标记 → PLAN_HOLD（本抽象中 Plan 会话保活，跨轮次复用）
//! REVIEWING（每轮全新 Review 实例，从头查）
//!   → PASS → 阶段完成（decisions/008 零校验，编排器信任 verdict）
//!   → FAIL → Review 实例即弃 → 问题清单回注活着的 Plan Agent → 自动重查（无上限）
//! ```
//!
//! Review 永不碰用户（P004 边界）；用户交互只在 Plan 环节内（ask-user-question）。

use crate::error::Result;
use crate::sop::output::{is_complete, parse_verdict, Verdict};
use crate::sop::review_gate::{review_with_gate, ReviewBudget, ReviewOutcome};

/// Plan Agent 开场指令（角色 + 强制完成确认 + 完成标记约定）。
pub const PLAN_KICKOFF: &str = "\
你是规划 Agent：读取 plan-tree skill，与用户沟通，产出 plan tree 到当前目录（任务目录即 plan root）。\n\
规则：\n\
1. 规划过程中用户确认类问题必须用 ask-user-question 询问。\n\
2. 结束前【必须】用 ask-user-question 问用户是否完成；用户确认后才算完成。\n\
3. 完成后，在你的最终回复末尾单独一行输出：<plan-complete>";

/// Review 检查指令（每轮全新实例都收到同一份）。
pub const REVIEW_INSTRUCTION: &str = "\
你是规划检查 Agent：读取 plan tree 与 plan-tree skill，只检查语义是否通顺、前后是否矛盾。\n\
不评设计好坏、不找潜在坑、不与用户交互。\n\
在回复末尾输出 verdict 块：\n\
通过：<verdict>PASS</verdict>\n\
不通过：<verdict>FAIL</verdict> 加 <issues> 清单（每行一条，`- ` 开头）。";

/// 打回回注前缀。
pub const REMEDIATION_PREFIX: &str = "Review 发现以下语义问题，请修订 plan tree：";

/// 用户链路抽象：Plan Agent 的 ask-user-question 经此抵达用户并带回回答。
/// 真身 = P003 API 中继（前端/MVP 裸 HTTP）；测试 = 脚本回答。
pub trait UserLink {
    fn ask(&mut self, question: &str, options: &[&str]) -> Result<String>;
}

/// Plan Agent 会话（干活者：跨轮次复用，上下文连续——decisions/004）。
pub trait PlanAgentSession {
    /// 与 Plan Agent 一轮对话。UI 问答（ask-user-question）在实现内部经 `ui` 发生。
    fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String>;
}

impl<P: PlanAgentSession + ?Sized> PlanAgentSession for &mut P {
    fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String> {
        (**self).converse(message, ui)
    }
}

/// Review 会话（检查者：用完即弃，decisions/004）。
pub trait ReviewSession {
    /// 丢检查指令，等结果，返回 Review 最终文本。
    fn review(&mut self, instruction: &str) -> Result<String>;

    /// 带墙钟的 review（第三方 r3 M4）：默认忽略墙钟（脚本替身无阻塞语义）；
    /// 真 RPC 会话覆写成「到点返回 Quality」。
    fn review_with_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        let _ = deadline;
        self.review(instruction)
    }
}

/// Review 会话工厂（每轮全新实例，不带前轮记忆）。
pub trait ReviewSessionFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>>;
}

impl<F: ReviewSessionFactory + ?Sized> ReviewSessionFactory for &mut F {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        (**self).open()
    }
}

/// 阶段报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    /// Review 轮数。
    pub rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
}

pub struct PlanningLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    pub plan: P,
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> PlanningLoop<P, F> {
    pub fn new(plan: P, review_factory: F) -> Self {
        Self { plan, review_factory }
    }

    /// 跑完规划阶段直至 Review 通过。
    ///
    /// 零硬限制（decisions/003）：打回轮次无上限；零校验（decisions/008）：
    /// Plan Agent 修复后直接自动重查，不验修复质量、不再问用户。
    pub fn run(&mut self, ui: &mut dyn UserLink) -> Result<StageReport> {
        // ---- PLAN 阶段：循环直至完成标记（完成判定在 Agent 环节内）----
        let mut message = PLAN_KICKOFF.to_string();
        loop {
            let text = self.plan.converse(&message, ui)?;
            if is_complete(&text) {
                break;
            }
            message = "继续。完成前记得 ask-user-question 向用户确认，并在最终回复输出完成标记。".into();
        }
        // 阶段推进：规划阶段交卷 → 进入审查阶段（里程碑：阶段怎么变的）
        tracing::info!(
            component = "planning-engine",
            event = "phase.change",
            from = "plan",
            to = "review",
            "MVP planning 阶段推进"
        );

        // ---- REVIEW 循环：每轮全新实例；FAIL 打回后零校验自动重查 ----
        let mut rounds = 0usize;
        let mut issues_total = 0usize;
        // 预算的生命周期 = 整个 review 循环（违例重审与实例失败重试都从这里计数）
        let mut budget = ReviewBudget::default();
        loop {
            // 协议违例 → 换全新实例重审（≤3，超限质量失败，绝不把违例当 issue 回注规划 agent）；
            // 判负（FAIL + 非空 issues）→ 照旧回注活着的规划 agent（零校验自动重查）。
            // 审查轮次口径（Q6）：`rounds` 是**已开过的实例数** → 本轮序号 = rounds + 1（首审 = 1）。
            let review_round = rounds + 1;
            // 「审查开庭」里程碑：在**发起**那一刻落（不是等审完）
            tracing::info!(
                component = "planning-engine",
                event = "review.open",
                role = "plan-review",
                round = review_round,
                "审查开庭"
            );
            let (verdict, opened) = review_with_gate(
                &mut self.review_factory,
                rounds,
                |_| REVIEW_INSTRUCTION.to_string(),
                &mut budget,
                |text| {
                    let v = parse_verdict(text)?;
                    let violation = v.violation_reason();
                    Ok(ReviewOutcome {
                        report: v,
                        violation,
                    })
                },
            )?;
            rounds += opened;
            match verdict {
                Verdict::Pass => {
                    return Ok(StageReport { rounds, issues_total });
                }
                Verdict::Fail { issues } => {
                    issues_total += issues.len();
                    let remediation = format!(
                        "{REMEDIATION_PREFIX}\n{}",
                        issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    // 判负回注：fix 起止一对（含轮次）——回注活着的 Plan Agent（它的上下文连续）；
                    // 修完零校验自动重查
                    tracing::info!(
                        component = "planning-engine",
                        event = "fix.start",
                        round = review_round,
                        "判负回注开始"
                    );
                    let _ = self.plan.converse(&remediation, ui)?;
                    tracing::info!(
                        component = "planning-engine",
                        event = "fix.end",
                        round = review_round,
                        "判负回注结束"
                    );
                }
                Verdict::ProtocolViolation { reason } => {
                    // 门已把违例收进「换新实例重审」通道：走到这里 = 门与解析器形状不一致
                    return Err(crate::error::DevforgeError::System(format!(
                        "审查协议违例未进门：{reason}"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::output::COMPLETION_TAG;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    // ---- 用户脚本 ----
    struct MockUser {
        answers: VecDeque<&'static str>,
        asked: Vec<String>,
    }
    impl MockUser {
        fn new(answers: &[&'static str]) -> Self {
            Self {
                answers: answers.iter().copied().collect(),
                asked: Vec::new(),
            }
        }
    }
    impl UserLink for MockUser {
        fn ask(&mut self, question: &str, _options: &[&str]) -> Result<String> {
            self.asked.push(question.into());
            self.answers
                .pop_front()
                .map(String::from)
                .ok_or_else(|| crate::error::DevforgeError::System("无更多用户回答".into()))
        }
    }

    // ---- Plan Agent 脚本 ----
    enum PlanTurn {
        /// 发起 ask-user-question；用户答「完成」→ 回复带完成标记，否则回复继续文本
        AskDone,
        /// 断言收到的消息包含片段（打回回注校验），然后回复
        ExpectContains(&'static str, &'static str),
        /// 断言不再有第二轮对话（协议违例不得回注规划 agent）
        NoSecondTurn,
    }
    struct MockPlan {
        turns: VecDeque<PlanTurn>,
        received: Vec<String>,
    }
    impl MockPlan {
        fn new(turns: &[PlanTurn]) -> Self {
            Self {
                turns: turns.iter().cloned().collect(),
                received: Vec::new(),
            }
        }
    }
    impl Clone for PlanTurn {
        fn clone(&self) -> Self {
            match self {
                PlanTurn::AskDone => PlanTurn::AskDone,
                PlanTurn::ExpectContains(a, b) => PlanTurn::ExpectContains(a, b),
                PlanTurn::NoSecondTurn => PlanTurn::NoSecondTurn,
            }
        }
    }
    impl PlanAgentSession for MockPlan {
        fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String> {
            self.received.push(message.into());
            match self.turns.pop_front().expect("MockPlan 轮次耗尽") {
                PlanTurn::AskDone => {
                    let answer = ui.ask("规划完成了吗？", &["完成", "还没"])?;
                    if answer == "完成" {
                        Ok(format!("好，全部落盘。\n{COMPLETION_TAG}"))
                    } else {
                        Ok("收到，继续修改 plan tree。".into())
                    }
                }
                PlanTurn::ExpectContains(needle, reply) => {
                    assert!(
                        message.contains(needle),
                        "应收到含 `{needle}` 的消息，实际: {message}"
                    );
                    Ok(reply.into())
                }
                PlanTurn::NoSecondTurn => {
                    panic!("规划 agent 不该收到第二轮回注，实际收到: {message}")
                }
            }
        }
    }

    // ---- Review 工厂脚本 ----
    struct MockReview {
        reply: &'static str,
        closed: Rc<Cell<bool>>,
    }
    impl ReviewSession for MockReview {
        fn review(&mut self, instruction: &str) -> Result<String> {
            assert_eq!(instruction, REVIEW_INSTRUCTION, "每轮应收到同一检查指令");
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
            Ok(Box::new(MockReview {
                reply: self.replies.pop_front().expect("Review 回复耗尽"),
                closed,
            }))
        }
    }

    const FAIL_ONE: &str = "有矛盾。\n<verdict>FAIL</verdict>\n<issues>\n- 规划说 Rust 又说 Python\n</issues>";
    const PASS: &str = "通顺。\n<verdict>PASS</verdict>";

    #[test]
    fn approved_first_round() {
        let plan = MockPlan::new(&[PlanTurn::AskDone]);
        let mut factory = MockReviewFactory::new(&[PASS]);
        let mut user = MockUser::new(&["完成"]);
        let report = PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        assert_eq!(report, StageReport { rounds: 1, issues_total: 0 });
        // Review 用完即弃
        assert!(factory.opened[0].get());
        // 完成确认发生且仅一次（在 Plan 环节内）
        assert_eq!(user.asked.len(), 1);
    }

    #[test]
    fn fail_then_remediation_then_pass() {
        let plan = MockPlan::new(&[
            PlanTurn::AskDone,
            PlanTurn::ExpectContains("规划说 Rust 又说 Python", "已修订。\n<plan-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_ONE, PASS]);
        let mut user = MockUser::new(&["完成"]);
        let report = PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        assert_eq!(report, StageReport { rounds: 2, issues_total: 1 });
        // 两轮都是全新实例且已弃
        assert_eq!(factory.opened.len(), 2);
        assert!(factory.opened.iter().all(|f| f.get()));
    }

    #[test]
    fn zero_validation_flow_no_user_ask_during_loop() {
        // 打回与重查之间没有任何用户确认——零校验流转（decisions/008）
        let plan = MockPlan::new(&[
            PlanTurn::AskDone,
            PlanTurn::ExpectContains(REMEDIATION_PREFIX, "修好了。<plan-complete>"),
            PlanTurn::ExpectContains(REMEDIATION_PREFIX, "又修好了。<plan-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_ONE, FAIL_ONE, PASS]);
        let mut user = MockUser::new(&["完成"]);
        let report = PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        assert_eq!(report.rounds, 3);
        assert_eq!(report.issues_total, 2);
        // 用户只被问过一次（初次完成确认）；两轮打回全程零打扰
        assert_eq!(user.asked.len(), 1);
    }

    #[test]
    fn review_open_and_fix_round_are_logged_as_milestones() {
        // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
        // ②判负回注不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
        let plan = MockPlan::new(&[
            PlanTurn::AskDone,
            PlanTurn::ExpectContains("规划说 Rust 又说 Python", "已修订。\n<plan-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_ONE, PASS]);
        let mut user = MockUser::new(&["完成"]);
        let text = crate::observability::test_support::capture_jsonl(|| {
            PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        });

        // 审查开庭：两轮各一条，角色名与轮次（首审 = 1）
        let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
        assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
        assert!(
            opens[0].contains("\"role\":\"plan-review\""),
            "开庭要带角色名：{}",
            opens[0]
        );
        assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
        assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
        assert!(
            opens[0].contains("\"target\":\"devforge::sop::planning\""),
            "里程碑的 target 必须是发出它的模块：{}",
            opens[0]
        );
        // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
        for line in text.lines().filter(|l| l.contains("\"event\":")) {
            crate::observability::test_support::assert_milestone_shape(line);
        }

        // 判负回注：一对 fix.start / fix.end（含轮次）
        let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
        let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
        assert_eq!(starts.len(), 1, "判负回注应落一条 fix.start：{text}");
        assert_eq!(ends.len(), 1, "判负回注应落一条 fix.end：{text}");
        assert!(starts[0].contains("\"round\":1"), "回注轮次要带上：{}", starts[0]);
        assert!(ends[0].contains("\"round\":1"), "回注轮次要带上：{}", ends[0]);

        // 阶段推进：plan → review
        let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
        assert_eq!(phases.len(), 1, "规划阶段推进应落一条：{text}");
        assert!(
            phases[0].contains("\"from\":\"plan\"") && phases[0].contains("\"to\":\"review\""),
            "阶段推进要从 plan 到 review：{}",
            phases[0]
        );
    }

    #[test]
    fn user_can_refuse_completion() {
        let plan = MockPlan::new(&[PlanTurn::AskDone, PlanTurn::AskDone]);
        let mut factory = MockReviewFactory::new(&[PASS]);
        let mut user = MockUser::new(&["还没", "完成"]);
        let report = PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        assert_eq!(report.rounds, 1);
        assert_eq!(user.asked.len(), 2);
    }

    #[test]
    fn missing_verdict_is_a_violation_retried_with_a_new_instance() {
        // 错实现形态：把协议违例当 issue 回注规划 agent（规划 agent 会收到「<verdict> 协议块」字样）
        // → 本用例必须变红
        let plan = MockPlan::new(&[PlanTurn::AskDone, PlanTurn::NoSecondTurn]);
        let mut factory = MockReviewFactory::new(&["我忘了输出块", PASS]);
        let mut user = MockUser::new(&["完成"]);
        let report = PlanningLoop::new(plan, &mut factory).run(&mut user).unwrap();
        assert_eq!(report.rounds, 2, "违例后换新实例重审");
        assert_eq!(report.issues_total, 0, "违例不计入被审对象的问题数");
        assert_eq!(factory.opened.len(), 2, "两个审查实例都开过且均为全新实例");
    }

    #[test]
    fn repeated_protocol_violations_fail_fast() {
        // 错实现形态：违例超限判成基础设施故障（会被自动重启）或无限重审 → 本用例必须变红
        let plan = MockPlan::new(&[PlanTurn::AskDone, PlanTurn::NoSecondTurn]);
        let mut factory = MockReviewFactory::new(&["缺块1", "缺块2", "缺块3", "缺块4"]);
        let mut user = MockUser::new(&["完成"]);
        let err = PlanningLoop::new(plan, &mut factory)
            .run(&mut user)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("协议违例"), "{msg}");
        assert!(
            err.is_quality_failure(),
            "违例超限是质量失败（不自动重启）: {msg}"
        );
    }
}
