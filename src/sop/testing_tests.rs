//! （2026-10-26 测试分离：自 src/sop/testing.rs 内联平移，纯移动零改动。）
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
fn review_open_fix_round_and_doc_handoff_are_logged_as_milestones() {
    // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
    // ②打回重写不落 fix.start/fix.end ③文档侧移交不落阶段推进 → 本用例必须变红
    let agent = MockAgent::new(&[
        Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        Turn::ExpectContains("缺超时用例", "补好了。\n<stage-complete>1</stage-complete>"),
    ]);
    let mut factory = MockReviewFactory::new(&[FAIL_TEST_SIDE, PASS_FINISH]);
    let mut user = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        let outcome = TestingSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap();
        let TestingOutcome::Completed(report) = outcome else {
            panic!("应正常收工");
        };
        assert_eq!(report.issues_total, 1);
    });

    // 审查开庭：两轮各一条，角色名与轮次（首审 = 1）
    let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
    assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
    assert!(
        opens[0].contains("\"role\":\"test-review\""),
        "开庭要带角色名：{}",
        opens[0]
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
    assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::testing\""),
        "里程碑的 target 必须是发出它的模块：{}",
        opens[0]
    );
    // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
    for line in text.lines().filter(|l| l.contains("\"event\":")) {
        crate::observability::test_support::assert_milestone_shape(line);
    }

    // 打回重写：一对 fix.start / fix.end（含轮次）
    let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
    let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
    assert_eq!(starts.len(), 1, "打回应落一条 fix.start：{text}");
    assert_eq!(ends.len(), 1, "打回应落一条 fix.end：{text}");
    assert!(starts[0].contains("\"round\":1"), "打回轮次要带上：{}", starts[0]);
    assert!(ends[0].contains("\"round\":1"), "打回轮次要带上：{}", ends[0]);

    // 阶段推进：start → 1
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert_eq!(phases.len(), 1, "只真推进时落一条：{text}");
    assert!(
        phases[0].contains("\"from\":\"start\"") && phases[0].contains("\"to\":\"1\""),
        "首个阶段从 start 推进：{}",
        phases[0]
    );
}

#[test]
fn the_doc_side_handoff_logs_a_phase_change_to_doc() {
    // 错实现形态：文档侧移交不落 phase.change（from=test to=doc）→ 本用例必须变红
    let agent = MockAgent::new(&[Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>")]);
    let mut factory = MockReviewFactory::new(&[FAIL_DOC_SIDE]);
    let mut user = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        let outcome = TestingSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap();
        let TestingOutcome::DocSideHandoff { report, .. } = outcome else {
            panic!("应移交文档侧");
        };
        assert_eq!(report.doc_fix_handoffs, 1);
    });
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert!(
        phases.iter().any(|l| l.contains("\"from\":\"test\"") && l.contains("\"to\":\"doc\"")),
        "移交文档侧要落一条 test → doc 的阶段推进：{text}"
    );
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
    let TestingOutcome::DocSideHandoff { stage, issues, report } = outcome else {
        panic!("应移交 doc-fix");
    };
    assert_eq!(stage, 1);
    assert_eq!(issues, vec!["文档缺分页接口描述"]);
    assert_eq!(report.review_rounds, 1, "移交携带最新账本");

    // resume：原样回传账本，审查轮次递增到 2（工厂断言指令带「第 2 轮」），PASS 后收工
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
    let TestingOutcome::DocSideHandoff { stage, report, .. } = first else {
        panic!("首次应移交");
    };
    let second = loop_
        .resume_after_docfix(&mut user, Path::new("/w"), stage, report)
        .unwrap();
    let TestingOutcome::DocSideHandoff { stage: stage2, report: report2, .. } = second else {
        panic!("恢复后仍可再移交（零上限）");
    };
    assert_eq!(stage2, 1);
    assert_eq!(report2.review_rounds, 2, "账本轮次跨移交累计");
}

#[test]
fn protocol_violation_retries_review_not_agent() {
    // C19-R1：test-review 输出协议违例 → 重跑审查，不回注被测 Agent、不触发 doc-fix
    let agent = MockAgent::new(&[Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>")]);
    let received = agent.received.clone();
    let no_verdict = "我说测试没问题。"; // 无协议块 → 保守 FAIL + protocol_violation
    let mut factory = MockReviewFactory::new(&[no_verdict, PASS_FINISH]);
    let mut user = NoUser;
    let outcome =
        TestingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    let TestingOutcome::Completed(report) = outcome else {
        panic!("违例重试后应正常收工");
    };
    assert_eq!(report.review_rounds, 2, "违例后重跑第二轮审查");
    assert_eq!(report.issues_total, 0, "违例不计入测试问题数");
    assert_eq!(report.doc_fix_handoffs, 0, "违例不触发 doc-fix");
    assert_eq!(received.borrow().len(), 1, "只收到 kickoff，无违例回注");
}

#[test]
fn repeated_protocol_violations_fail_fast() {
    let agent = MockAgent::new(&[Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>")]);
    let mut factory = MockReviewFactory::new(&["缺块1", "缺块2", "缺块3", "缺块4"]);
    let mut user = NoUser;
    let err = TestingSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    assert!(err.to_string().contains("协议违例"), "应报质量失败: {err}");
    assert!(err.to_string().contains("质量失败"), "协议违例上限归质量失败: {err}");
}

/// 全仓审计 #7 回归：审查判 FAIL 却不给 issues 明细 → 拒收 + 追补；
/// 连续超限判质量失败。错实现形态：把空清单直接回注（agent 盲改、可无界空转），
/// 而 change 侧早已有 MAX_EMPTY_ISSUES_NUDGES 护栏——两条链路口径不一。
#[test]
fn fail_without_issues_is_bounded_not_infinite() {
    let agent = MockAgent::new(&[
        Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        Turn::Say("改了点东西。\n<stage-complete>1</stage-complete>"),
        Turn::Say("又改了点。\n<stage-complete>1</stage-complete>"),
        Turn::Say("继续改。\n<stage-complete>1</stage-complete>"),
    ]);
    let received = agent.received.clone();
    // FAIL 但 <issues> 为空——旧实现会把它当可用的失败结论直接回注
    let empty_issues = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues></issues>";
    let mut factory =
        MockReviewFactory::new(&[empty_issues, empty_issues, empty_issues, empty_issues]);
    let mut user = NoUser;
    let err = TestingSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("未给出 issues 明细") && msg.contains("质量失败"),
        "空 issues 应拒收并有界收敛：{msg}"
    );
    assert!(
        received.borrow().iter().any(|m| m.contains("引擎拒收")),
        "应把追补话术发给 agent：{:?}",
        received.borrow()
    );
}

#[test]
fn repeated_missing_marker_fails_fast() {
    // 007 D013：连续 4 次不发标记 → 质量失败（不无限空转）
    let agent = MockAgent::new(&[
        Turn::Say("还在跑"),
        Turn::Say("还在跑"),
        Turn::Say("还在跑"),
        Turn::Say("就是不发标记"),
    ]);
    let mut factory = MockReviewFactory::new(&[]);
    let mut user = NoUser;
    let err = TestingSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("stage-complete"), "{msg}");
    assert!(msg.contains("质量失败"), "{msg}");
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
