//! （2026-10-26 测试分离：自 src/engine/policy.rs 内联平移，纯移动零改动。）
use super::*;
use std::cell::Cell;
use std::rc::Rc;

fn infra() -> crate::error::Result<RunOutcome> {
    Err(DevforgeError::System("upstream 500".into()))
}

#[test]
fn two_infra_failures_then_success_settles_on_third_attempt() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), move |n| {
        c.set(c.get() + 1);
        if n < 3 { infra() } else { Ok(RunOutcome::Completed) }
    });
    assert!(matches!(result, Ok(RunOutcome::Completed)));
    assert_eq!(attempts, 3);
    assert_eq!(calls.get(), 3);
}

#[test]
fn persistent_infra_failure_stops_at_four_attempts() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), move |n| {
        c.set(c.get() + 1);
        Err(DevforgeError::System(format!("第 {n} 次挂了")))
    });
    assert!(result.is_err(), "连续故障必须判死，不能无限重试");
    assert_eq!(attempts, 4, "首次 + 3 次重试");
    assert_eq!(calls.get(), 4, "不得多跑第 5 次");
    // 错误串必须带每次尝试的末段（评估定位需要）
    let msg = result.unwrap_err().to_string();
    for n in 1..=4 {
        assert!(msg.contains(&format!("第 {n} 次挂了")), "缺第 {n} 次记录：{msg}");
    }
    assert!(msg.contains("共尝试 4 次"), "{msg}");
}

#[test]
fn quality_failure_is_not_retried() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), move |_| {
        c.set(c.get() + 1);
        Ok(RunOutcome::Failed {
            reason: "审查打回：缺验收标准".into(),
        })
    });
    assert!(matches!(result, Ok(RunOutcome::Failed { .. })));
    assert_eq!(attempts, 1, "质量失败不得自动重启");
    assert_eq!(calls.get(), 1);
}

#[test]
fn cancelled_run_is_not_retried() {
    let control = RunControl::new();
    control.cancel();
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &control, move |_| {
        c.set(c.get() + 1);
        infra()
    });
    assert!(result.is_err());
    assert_eq!(attempts, 1, "人喊停就停，不再重启");
    assert_eq!(calls.get(), 1);
}

#[test]
fn backoff_schedule_matches_decision() {
    let p = RetryPolicy::default();
    assert_eq!(p.backoff_ms, vec![10_000, 30_000, 90_000]);
    assert_eq!(p.backoff_for(1), 10_000);
    assert_eq!(p.backoff_for(2), 30_000);
    assert_eq!(p.backoff_for(3), 90_000);
}

#[test]
fn non_system_errors_are_not_infra() {
    let other: crate::error::Result<RunOutcome> =
        Err(DevforgeError::Store("任务不存在".into()));
    assert!(!is_infra_failure(&other));
    assert!(is_infra_failure(&infra()));
}

#[test]
fn the_run_context_carries_the_current_attempt_while_the_attempt_runs() {
    // 错实现形态：attempt 只在**失败分支**里写 → 第 2 次尝试期间上下文仍是上一次的值
    //   → `map_intervention` 的质量 WARN 会带陈旧的 attempt → 本用例必须变红
    crate::observability::set_attempt(0); // 起点归零：别让别的用例留下的残值影响判读
    let mut seen: Vec<u32> = Vec::new();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
        seen.push(crate::observability::attempt());
        if n < 3 { infra() } else { Ok(RunOutcome::Completed) }
    });
    assert!(matches!(result, Ok(RunOutcome::Completed)));
    assert_eq!(attempts, 3);
    assert_eq!(
        seen,
        vec![1, 2, 3],
        "每次尝试**开始时** run 上下文就应是本次尝试号（R-1）"
    );
}
