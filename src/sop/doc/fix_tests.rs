//! （2026-10-26 测试分离：自 src/sop/doc/fix.rs 内联平移，纯移动零改动。）
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
    deadlines: Rc<RefCell<Vec<Option<std::time::Instant>>>>,
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
    fn review_with_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        self.deadlines.borrow_mut().push(deadline);
        self.review(instruction)
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
    deadlines: Rc<RefCell<Vec<Option<std::time::Instant>>>>,
}
impl MockReviewFactory {
    fn new(replies: &[&'static str]) -> Self {
        Self {
            replies: replies.iter().copied().collect(),
            opened: Vec::new(),
            deadlines: Default::default(),
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
            deadlines: self.deadlines.clone(),
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

/// 自审 critical 回归：doc-fix 审查必须走墙钟——裸 review() 在 doc SOP
/// （默认无失活收割）下挂死即永挂并占住串行槽（server #6 同款）。
#[test]
fn review_carries_a_wall_clock() {
    let agent = MockAgent::new(&["补完文档了。"]);
    let mut factory = MockReviewFactory::new(&[REVIEW_PASS]);
    let deadlines = factory.deadlines.clone();
    let mut user = NoUser;
    DocFixSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap();
    let d = deadlines.borrow();
    assert_eq!(d.len(), 1);
    assert!(d[0].is_some(), "审查必须带 deadline（Some）");
}

/// 自审 critical 回归：FAIL 不带 <issues> → 回注空清单，无上限 = run 无限空转
/// （每圈真烧两次 LLM）。修复后连续 FAIL 有上限，Quality 收场。
#[test]
fn fail_without_issues_is_capped_not_infinite() {
    const FAIL_NO_ISSUES: &str = "<stage-verdict>FAIL</stage-verdict>";
    let agent = MockAgent::new(&["改", "改", "改", "改"]);
    let mut factory = MockReviewFactory::new(&[
        FAIL_NO_ISSUES,
        FAIL_NO_ISSUES,
        FAIL_NO_ISSUES,
        FAIL_NO_ISSUES,
    ]);
    let mut user = NoUser;
    let err = DocFixSopLoop::new(agent, &mut factory)
        .run(&mut user, Path::new("/w"), "p", "t")
        .unwrap_err();
    assert!(err.is_quality_failure(), "FAIL 空转是质量失败：{err}");
    assert!(format!("{err}").contains("上限"), "{err}");
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
fn review_open_and_fix_round_are_logged_as_milestones() {
    // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
    // ②打回重写不落 fix.start/fix.end → 本用例必须变红
    let agent = MockAgent::new(&["补了一版。", "又补了一版。"]);
    let mut factory = MockReviewFactory::new(&[REVIEW_FAIL_MISSING_SEED, REVIEW_PASS]);
    let mut user = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        DocFixSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap();
    });

    // 审查开庭：两轮各一条，角色名与轮次
    // 轮次口径（Q6/R22）：doc-fix 是**先自增再开实例** → 落 report.review_rounds，**不加 1**（首审 = 1）。
    let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
    assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
    assert!(
        opens[0].contains("\"role\":\"doc-review\""),
        "开庭要带角色名：{}",
        opens[0]
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
    assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc::fix\""),
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
