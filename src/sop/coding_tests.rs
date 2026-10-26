//! （2026-10-26 测试分离：自 src/sop/coding.rs 内联平移，纯移动零改动。）
use super::*;
use crate::sop::planning::ReviewSession;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

// ---- 无用户交互：编码 SOP 里用户只经 ask-user-question（会话内部），测试不给用户脚本 ----
struct NoUser;
impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
        Err(crate::error::DevforgeError::System(format!(
            "测试不应有用户交互: {q}"
        )))
    }
}

// ---- Coding Agent 脚本 ----
enum Turn {
    /// 回复文本
    Say(&'static str),
    /// 断言收到的消息包含片段，然后回复
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

// ---- Review 工厂脚本 ----
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
        // 轮次号 = 即将开始的第 N 轮（全局计数，不是实例内计数）
        let round = self.opened.len(); // push 后取长度 = 第 N 轮
        Ok(Box::new(MockReview {
            reply: self.replies.pop_front().expect("Review 回复耗尽"),
            closed,
            expect_round: round,
        }))
    }
}

const PASS_ADVANCE: &str =
    "合格。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
const PASS_FINISH: &str = "全部达成。\n<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
const FAIL_FIX_ONE: &str = "有问题。\n<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 登录没处理过期 token\n</issues>";

fn kickoff_starts_with(ws: &Path, first: &str) -> bool {
    let k = coding_kickoff(ws, "demo", "t1");
    k.starts_with(first) && k.contains(ws.display().to_string().as_str())
}

#[test]
fn kickoff_carries_dual_paths() {
    // decisions/001：双路径告知（黑板 cwd + 代码工作区注入）
    let ws = Path::new("/tmp/ws-x");
    assert!(kickoff_starts_with(ws, "你是编码 Agent：项目 `demo` / 任务 `t1`"));
    assert!(coding_review_instruction(ws, 3).contains("第 3 轮"));
    assert!(coding_review_instruction(ws, 3).contains("/tmp/ws-x"));
}

#[test]
fn two_stages_advance_then_finish() {
    // 阶段推进：s1 完成→PASS+advance→s2 完成→PASS+finish 收工
    let agent = MockAgent::new(&[
        Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        Turn::ExpectContains(
            "进入阶段 2",
            "阶段 2 完成。\n<stage-complete>2</stage-complete>",
        ),
    ]);
    let mut factory = MockReviewFactory::new(&[PASS_ADVANCE, PASS_FINISH]);
    let mut user = NoUser;
    let report =
        CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    assert_eq!(report.stages_completed, vec![1, 2]);
    assert_eq!(report.review_rounds, 2);
    assert_eq!(report.issues_total, 0);
    // 每轮审查都是全新实例且已弃
    assert!(factory.opened.iter().all(|f| f.get()));
}

#[test]
fn fail_then_fix_then_reemit_marker_then_advance() {
    // 打回重审：FAIL+fix → 问题回注 → Agent 修复后重发标记 → 重审通过
    let agent = MockAgent::new(&[
        Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        Turn::ExpectContains(
            "登录没处理过期 token",
            "修好了。\n<stage-complete>1</stage-complete>",
        ),
        Turn::Say("好了。\n<stage-complete>2</stage-complete>"),
    ]);
    let mut factory = MockReviewFactory::new(&[FAIL_FIX_ONE, PASS_ADVANCE, PASS_FINISH]);
    let mut user = NoUser;
    let report =
        CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    assert_eq!(report.stages_completed, vec![1, 2]);
    assert_eq!(report.review_rounds, 3);
    assert_eq!(report.issues_total, 1);
}

#[test]
fn review_open_and_fix_round_are_logged_as_milestones() {
    // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
    // ②打回重写不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
    let agent = MockAgent::new(&[
        Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
        Turn::ExpectContains(
            "登录没处理过期 token",
            "修好了。\n<stage-complete>1</stage-complete>",
        ),
    ]);
    let mut factory = MockReviewFactory::new(&[FAIL_FIX_ONE, PASS_FINISH]);
    let mut user = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        CodingSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap();
    });

    // 审查开庭：两轮各一条，角色名与轮次（首审 = 1）
    let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
    assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
    assert!(
        opens[0].contains("\"role\":\"coding-review\""),
        "开庭要带角色名：{}",
        opens[0]
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
    assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::coding\""),
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

    // 阶段推进：start → 1（重发同一阶段号不是推进 → 不重复落）
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert_eq!(phases.len(), 1, "只真推进时落一条：{text}");
    assert!(
        phases[0].contains("\"from\":\"start\"") && phases[0].contains("\"to\":\"1\""),
        "首个阶段从 start 推进：{}",
        phases[0]
    );
}

#[test]
fn no_marker_gets_nudged() {
    // 无标记重推（零上限）：停转但没发标记 → CONTINUE_NUDGE 回注
    let agent = MockAgent::new(&[
        Turn::Say("我停下来想想"),
        Turn::ExpectContains(
            "stage-complete",
            "阶段 1 完成。\n<stage-complete>1</stage-complete>",
        ),
    ]);
    let rx = std::rc::Rc::clone(&agent.received);
    let mut factory = MockReviewFactory::new(&[PASS_FINISH]);
    let mut user = NoUser;
    let report =
        CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    assert_eq!(report.stages_completed, vec![1]);
    // 收到的第二条消息就是重推话术
    assert_eq!(rx.borrow()[1], CONTINUE_NUDGE);
}

#[test]
fn repeated_missing_marker_fails_fast() {
    // 005/015：连续 4 次不发标记 → 质量失败（不无限空转）
    let agent = MockAgent::new(&[
        Turn::Say("还在写"),
        Turn::Say("还在写"),
        Turn::Say("还在写"),
        Turn::Say("就是不发标记"),
    ]);
    let mut factory = MockReviewFactory::new(&[]);
    let mut user = NoUser;
    let err = CodingSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("stage-complete"), "{msg}");
    assert!(msg.contains("质量失败"), "{msg}");
}

#[test]
fn advance_digests_notes_before_next_stage() {
    // PASS+advance 附非阻塞建议 → 回注消息含建议（先消化再开工）
    let adv_with_notes: &'static str =
        "合格，有建议。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>\n<notes>\n- 补个边界测试\n- 命名收敛\n</notes>";
    let agent = MockAgent::new(&[
        Turn::Say("<stage-complete>1</stage-complete>"),
        Turn::ExpectContains(
            "补个边界测试",
            "阶段 2 完成。\n<stage-complete>2</stage-complete>",
        ),
        Turn::ExpectContains(
            "进入阶段 3",
            "<stage-complete>3</stage-complete>",
        ),
    ]);
    let mut factory = MockReviewFactory::new(&[adv_with_notes, PASS_ADVANCE, PASS_FINISH]);
    let mut user = NoUser;
    let report =
        CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    assert_eq!(report.stages_completed, vec![1, 2, 3]);
}

#[test]
fn protocol_violation_retries_review_not_agent() {
    // C19-R1：审查输出协议违例（缺块）→ **重跑审查**（新 fresh 实例），
    // 不把「审查官输出格式问题」回注给被审 Agent；issues_total 也不应累计。
    let agent = MockAgent::new(&[Turn::Say("<stage-complete>1</stage-complete>")]);
    let mut factory = MockReviewFactory::new(&["我忘了输出协议块", PASS_FINISH]);
    let mut user = NoUser;
    let report =
        CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
    assert_eq!(report.stages_completed, vec![1]);
    assert_eq!(report.review_rounds, 2, "违例后重跑了第二轮审查");
    assert_eq!(report.issues_total, 0, "违例不计入代码问题数");
    assert_eq!(factory.opened.len(), 2, "两个审查实例都开过且均被丢弃");
}

#[test]
fn repeated_protocol_violations_fail_fast() {
    // 连续违例超上限（3）→ 基础设施故障报错，不无限重试、不回注
    let agent = MockAgent::new(&[Turn::Say("<stage-complete>1</stage-complete>")]);
    let mut factory = MockReviewFactory::new(&[
        "缺块1", "缺块2", "缺块3", "缺块4",
    ]);
    let mut user = NoUser;
    let err = CodingSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    assert!(
        err.to_string().contains("协议违例"),
        "应报协议违例基础设施故障: {err}"
    );
}
