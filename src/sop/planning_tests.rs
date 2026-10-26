//! （2026-10-26 测试分离：自 src/sop/planning.rs 内联平移，纯移动零改动。）
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
