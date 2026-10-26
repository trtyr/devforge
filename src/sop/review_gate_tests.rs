//! （2026-10-26 测试分离：自 src/sop/review_gate.rs 内联平移，纯移动零改动。）
use super::*;
use crate::sop::planning::ReviewSession;
use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

/// 审查脚本：每次 open 弹一条回复。
struct ScriptedReview {
    replies: VecDeque<Result<String>>,
    opened: Rc<Cell<usize>>,
}

impl ReviewSession for ScriptedReview {
    fn review(&mut self, _instruction: &str) -> Result<String> {
        self.opened.set(self.opened.get() + 1);
        self.replies.pop_front().expect("Review 脚本耗尽")
    }
}

/// 工厂：每次 open 弹一条回复（或一条错误）。
#[derive(Default)]
struct ScriptedFactory {
    replies: VecDeque<Result<String>>,
    opened: Rc<Cell<usize>>,
}

impl ScriptedFactory {
    fn new(replies: Vec<Result<String>>) -> Self {
        Self {
            replies: replies.into(),
            opened: Rc::new(Cell::new(0)),
        }
    }
}

impl ReviewSessionFactory for ScriptedFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        let reply = self.replies.pop_front().expect("Review 脚本耗尽");
        Ok(Box::new(ScriptedReview {
            replies: VecDeque::from([reply]),
            opened: self.opened.clone(),
        }))
    }
}

/// 判负解析器：`violation` 标记为真即违例通道。
fn parse_violation(text: &str) -> Result<ReviewOutcome<String>> {
    if text.starts_with("BAD") {
        Ok(ReviewOutcome::violation(text.to_string(), "审查输出不合规"))
    } else {
        Ok(ReviewOutcome::verdict(text.to_string()))
    }
}

#[test]
fn verdict_passes_through_and_counts_instances() {
    // 错实现形态：把实例数报成 1（少算重审）→ 本用例必须变红
    let mut factory = ScriptedFactory::new(vec![Ok("GOOD".into())]);
    let mut budget = ReviewBudget::default();
    let (report, opened) = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        parse_violation,
    )
    .unwrap();
    assert_eq!(report, "GOOD");
    assert_eq!(opened, 1, "开过的实例数要如实报出");
}

#[test]
fn violation_retries_with_a_new_instance() {
    // 错实现形态：把协议违例当判负（直接交出）→ 本用例必须变红
    let mut factory = ScriptedFactory::new(vec![Ok("BAD".into()), Ok("GOOD".into())]);
    let mut budget = ReviewBudget::default();
    let (report, opened) = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        parse_violation,
    )
    .unwrap();
    assert_eq!(report, "GOOD");
    assert_eq!(opened, 2, "违例后必须换新实例重审");
    assert_eq!(budget.violations, 1);
}

#[test]
fn violation_budget_exhausted_is_quality_failure() {
    // 错实现形态：违例超限判成 System（会被自动重启）→ 本用例必须变红
    let mut factory = ScriptedFactory::new(vec![
        Ok("BAD".into()),
        Ok("BAD".into()),
        Ok("BAD".into()),
        Ok("BAD".into()),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        parse_violation,
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("协议违例"), "{msg}");
    assert!(err.is_quality_failure(), "违例超限是质量失败，不该自动重启");
}

#[test]
fn instance_failures_are_not_verdicts_and_exhaust_to_system() {
    // 错实现形态：空回复 / 通道错误被当成判负（不换实例）→ 本用例必须变红
    let mut factory = ScriptedFactory::new(vec![
        Ok(String::new()),
        Err(DevforgeError::System("通道 500".into())),
        Ok(String::new()),
        Err(DevforgeError::System("通道 500".into())),
        Ok(String::new()),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        parse_violation,
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("基础设施故障"), "{msg}");
    assert!(!err.is_quality_failure(), "实例失败耗尽是基础设施故障（可重启）");
    assert_eq!(budget.instance_failures, 4, "空回复与通道错误同属实例失败");
}

#[test]
fn instance_failure_is_retried_before_exhaustion() {
    // 错实现形态：第一次实例失败就判死 run → 本用例必须变红
    let mut factory = ScriptedFactory::new(vec![Ok(String::new()), Ok("GOOD".into())]);
    let mut budget = ReviewBudget::default();
    let (report, opened) = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        parse_violation,
    )
    .unwrap();
    assert_eq!(report, "GOOD");
    assert_eq!(opened, 2, "实例失败后必须换新实例重试");
}

/// 每轮审查必须走 `review_with_deadline` 并带上墙钟（server #6：旧实现只调裸
/// `review()`，轮询链路根本没有墙钟——只靠 15 分钟失活收割）。探针在 `review()`
/// 里直接 panic：一旦调用方退回裸 review，本用例立刻红。
#[test]
fn review_rounds_carry_a_wall_clock() {
    struct Probe(std::sync::Arc<std::sync::Mutex<Vec<Option<std::time::Instant>>>>);
    impl crate::sop::planning::ReviewSession for Probe {
        fn review(&mut self, _i: &str) -> Result<String> {
            panic!("审查轮必须走 review_with_deadline（带墙钟），不得退回裸 review");
        }
        fn review_with_deadline(
            &mut self,
            _i: &str,
            deadline: Option<std::time::Instant>,
        ) -> Result<String> {
            self.0.lock().unwrap().push(deadline);
            Ok("通过".into())
        }
    }
    struct ProbeFactory(std::sync::Arc<std::sync::Mutex<Vec<Option<std::time::Instant>>>>);
    impl crate::sop::planning::ReviewSessionFactory for ProbeFactory {
        fn open(&mut self) -> Result<Box<dyn crate::sop::planning::ReviewSession>> {
            Ok(Box::new(Probe(self.0.clone())))
        }
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut factory = ProbeFactory(seen.clone());
    let mut budget = ReviewBudget::default();
    let (_r, opened) = review_with_gate::<u8, _, _>(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        |_| Ok(ReviewOutcome::verdict(1u8)),
    )
    .unwrap();
    assert_eq!(opened, 1);
    let got = seen.lock().unwrap();
    assert_eq!(got.len(), 1, "每轮一个墙钟：{got:?}");
    assert!(got[0].is_some(), "默认必须带墙钟（DEVFORGE_REVIEW_TIMEOUT_MS 未置 0）");
}
