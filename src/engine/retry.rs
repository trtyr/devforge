//! 运行级重试（plan 010 · T005/T006）：把「基础设施故障」与「质量失败」分开处理。
//!
//! 口径（010 D004）：
//! - **基础设施故障**（`DevforgeError::System`：上游 5xx、审查空回复、子进程瞬时死）→
//!   同一 run 自动重启，最多 3 次（总尝试 4 次），退避 10s/30s/90s，每次从断点继续。
//! - **质量失败**（`RunOutcome::Failed`：审查打回、协议违例上限、越界申报、验收不通过）→
//!   不重启，重启只会得到同一个结论，还会掩盖计划/实现问题。
//! - **已取消** → 不重启（人喊停就停）。

use crate::domain::runner::{RunControl, RunOutcome};
use crate::error::DevforgeError;

/// 总尝试次数上限：首次 + 3 次重试。
pub const MAX_ATTEMPTS: u32 = 4;

/// 默认退避（毫秒），长度 = 重试次数。
pub fn default_backoff_ms() -> Vec<u64> {
    vec![10_000, 30_000, 90_000]
}

/// 重试策略（测试注入 `sleep = false`，避免真睡）。
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: Vec<u64>,
    /// 是否真的 sleep（生产 true，单测 false）。
    pub sleep: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_ATTEMPTS,
            backoff_ms: default_backoff_ms(),
            sleep: true,
        }
    }
}

impl RetryPolicy {
    /// 单测用：不睡、退避归零。
    pub fn fast() -> Self {
        Self {
            max_attempts: MAX_ATTEMPTS,
            backoff_ms: vec![0, 0, 0],
            sleep: false,
        }
    }

    fn backoff_for(&self, attempt: u32) -> u64 {
        // attempt 从 1 开始；第 1 次失败后用 backoff[0]
        self.backoff_ms
            .get(attempt.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(0)
    }
}

/// 只有基础设施类错误值得重启。
pub fn is_infra_failure(result: &crate::error::Result<RunOutcome>) -> bool {
    matches!(result, Err(DevforgeError::System(_)))
}

/// 带重试地执行一次 run。
///
/// `execute(attempt)` 收到**从 1 开始**的尝试次数；返回（最终结果，实际尝试次数）。
/// 是否重跑只由「基础设施故障 + 未取消 + 未到上限」三件事决定。
pub fn run_with_retry<F>(
    policy: &RetryPolicy,
    control: &RunControl,
    mut execute: F,
) -> (crate::error::Result<RunOutcome>, u32)
where
    F: FnMut(u32) -> crate::error::Result<RunOutcome>,
{
    let mut attempt = 1u32;
    let mut log: Vec<String> = Vec::new();
    loop {
        let result = execute(attempt);
        if let Err(e) = &result {
            log.push(format!("第{attempt}次: {e}"));
        }
        let retryable = is_infra_failure(&result) && !control.is_cancelled();
        if !retryable || attempt >= policy.max_attempts {
            // 多次尝试后仍失败：错误串必须带每次尝试的末段，否则事后无法定位是哪一次怎么挂的
            if attempt > 1 && log.len() > 1 && result.is_err() {
                return (
                    Err(DevforgeError::System(format!(
                        "{}（共尝试 {attempt} 次；尝试记录：{}）",
                        log.last().cloned().unwrap_or_default(),
                        log.join(" / ")
                    ))),
                    attempt,
                );
            }
            return (result, attempt);
        }
        if policy.sleep {
            let ms = policy.backoff_for(attempt);
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
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
}
