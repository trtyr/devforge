//! 审查输出的三通道机制：判负 / 协议违例 / 实例失败（change 三角色 + MVP 三循环共用一份）。
//!
//! 三条通道的处置各不相同（decisions D026）：
//! - **判负**：审查官给了可用结论（PASS，或 FAIL + 非空 issues）→ 交出调用方；
//! - **协议违例**：审查输出**本身**不合规（缺协议块 / 未知值 / 自相矛盾 / FAIL 不带 issues）
//!   → 换**全新实例**重审（绝不回注被审对象），超上限 → 质量失败（Quality，不自动重启）；
//! - **实例失败**：审查实例自己挂了（空回复 / 进程早退 / 模型通道错误）
//!   → 换新实例重试，超上限 → 基础设施故障（System，触发 run 级自动重启）。
//!
//! 上限常量是单一事实源：change 侧三个审查角色与 MVP 三个循环都用这里的两个数。

use crate::error::{DevforgeError, Result};
use crate::sop::planning::ReviewSessionFactory;

/// 协议违例重审上限：连续违例超过此数 → 质量失败（Quality，不自动重启）。
pub const MAX_PROTOCOL_VIOLATIONS: usize = 3;

/// 审查实例失败重试上限：连续失败超过此数 → 基础设施故障（System，触发 run 级自动重启）。
pub const MAX_REVIEW_INSTANCE_FAILURES: usize = 3;

/// 空箱拒收的追补上限（全仓审计 #7：跨链路单点）——裁决 FAIL 却不给失败明细时，
/// 回注空清单等于让 agent 盲改，可无界空转；连续这么多次还不补 → 质量失败（不自动重启）。
pub const MAX_EMPTY_ISSUES_NUDGES: usize = 3;

/// 单轮审查墙钟默认值（server #6）：旧实现只调 `review()`，`review_with_deadline`
/// 对 planning/coding/testing 全是死路径——审查 pi 挂死只能等 15 分钟失活收割。
/// `DEVFORGE_REVIEW_TIMEOUT_MS` 可调，0 = 不限。
pub const DEFAULT_REVIEW_TIMEOUT_MS: u64 = 30 * 60 * 1000;

/// 本轮的审查墙钟（每轮现算：一轮一个 deadline）。
fn review_deadline() -> Option<std::time::Instant> {
    let ms = std::env::var("DEVFORGE_REVIEW_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_REVIEW_TIMEOUT_MS);
    if ms == 0 {
        return None;
    }
    Some(std::time::Instant::now() + std::time::Duration::from_millis(ms))
}

/// 空箱拒收的追补话术（change 测试回注 + MVP testing 循环共用）。
pub const ISSUES_NUDGE: &str = "\
你给出的裁决是 FAIL，但没有给出 <issues> 失败明细（每条一行，`- ` 开头）。\
这条回注已被引擎拒收：请补齐失败明细后重发。";

/// 审查预算：两个计数的**生命周期由调用方决定**。
///
/// - change 侧：每次审查调用一个（一次审查就是一次判负机会）；
/// - MVP 循环：整个循环一个（保住「跨阶段累计违例」的既有语义）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReviewBudget {
    /// 累计协议违例次数。
    pub violations: usize,
    /// 累计审查实例失败次数。
    pub instance_failures: usize,
}

/// 解析器产出的审查结果 + 通道判定。
pub struct ReviewOutcome<T> {
    /// 解析出的报告（判负通道才有意义；违例通道里是解析器的保守结果）。
    pub report: T,
    /// 协议违例原因：`Some` = 审查输出本身不合规（换新实例重审）；`None` = 判负通道。
    pub violation: Option<String>,
}

impl<T> ReviewOutcome<T> {
    /// 判负通道：审查官给了可用结论。
    pub fn verdict(report: T) -> Self {
        Self {
            report,
            violation: None,
        }
    }

    /// 协议违例通道：审查输出本身不合规（原因进错误文本，不回注被审对象）。
    pub fn violation(report: T, reason: impl Into<String>) -> Self {
        Self {
            report,
            violation: Some(reason.into()),
        }
    }
}

/// 把 `parse_review_report` 的结果接进三通道（`protocol_violation` = 违例通道）。
pub fn outcome_of_review_report(
    report: crate::sop::coding_protocol::ReviewReport,
) -> ReviewOutcome<crate::sop::coding_protocol::ReviewReport> {
    if report.protocol_violation {
        let reason = report.issues.join("；");
        ReviewOutcome::violation(report, reason)
    } else {
        ReviewOutcome::verdict(report)
    }
}

/// 跑一轮审查：每轮开**新**实例，按三通道分类。
///
/// 返回（判负通道的报告，实际开过的实例数）——实例数即「审查轮次」：
/// 违例重审与实例失败重试都计入，报告里的轮次如实反映重审。
///
/// `base_round` 是调用方已用掉的轮次数；`instruction` 按**本次实例的轮次号**生成指令
/// （MVP 侧的审查指令带轮次号，change 侧的固定不变）。
pub fn review_with_gate<T, P, I>(
    factory: &mut dyn ReviewSessionFactory,
    base_round: usize,
    mut instruction: I,
    budget: &mut ReviewBudget,
    parse: P,
) -> Result<(T, usize)>
where
    I: FnMut(usize) -> String,
    P: Fn(&str) -> Result<ReviewOutcome<T>>,
{
    let mut opened = 0usize;
    loop {
        opened += 1;
        let round = base_round + opened;
        let mut session = match factory.open() {
            Ok(s) => s,
            Err(e) => {
                // 实例开不起来 = 实例失败（同一类故障），不是判负
                if let Some(fatal) =
                    instance_failure(budget, round, &format!("开审查实例失败：{e}"))
                {
                    return Err(fatal);
                }
                continue;
            }
        };
        let outcome = session.review_with_deadline(&instruction(round), review_deadline());
        drop(session); // 用完即弃（无论结论），下一轮必是全新实例
        let text = match outcome {
            Ok(t) => t,
            Err(e) => {
                if let Some(fatal) =
                    instance_failure(budget, round, &format!("审查实例失败：{e}"))
                {
                    return Err(fatal);
                }
                continue;
            }
        };
        if text.trim().is_empty() {
            if let Some(fatal) = instance_failure(
                budget,
                round,
                "审查实例返回空回复（进程正常结束但未产出一字）",
            ) {
                return Err(fatal);
            }
            continue;
        }
        let parsed = parse(&text)?;
        let Some(reason) = parsed.violation else {
            return Ok((parsed.report, opened));
        };
        budget.violations += 1;
        if budget.violations > MAX_PROTOCOL_VIOLATIONS {
            return Err(DevforgeError::Quality(format!(
                "审查连续 {} 轮输出协议违例（{reason}），判定为质量失败（不自动重启），停止 run",
                budget.violations
            )));
        }
        tracing::warn!(
            component = "review-gate",
            round,
            violations = budget.violations,
            "审查输出协议违例，换新实例重审（不回注被审对象）"
        );
    }
}

/// 实例失败：计数 + 判还能不能重试。
///
/// `None` = 还能重试（换新实例再审一次）；`Some(基础设施故障)` = 已耗尽。
fn instance_failure(
    budget: &mut ReviewBudget,
    round: usize,
    reason: &str,
) -> Option<DevforgeError> {
    budget.instance_failures += 1;
    if budget.instance_failures > MAX_REVIEW_INSTANCE_FAILURES {
        return Some(DevforgeError::System(format!(
            "审查实例连续 {} 次失败（最后一次：{reason}），判定为基础设施故障（触发 run 级自动重启）",
            budget.instance_failures
        )));
    }
    tracing::warn!(
        component = "review-gate",
        round,
        failures = budget.instance_failures,
        "审查实例失败，换新实例重试（不判负）"
    );
    None
}

#[cfg(test)]
mod tests {
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
}
