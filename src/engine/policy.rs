//! 运行级横切策略单点（ARC-R5）：重试 / 干预映射 / 事件字段形状，同模块同居。
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
        // attempt 在**每次尝试开始时**写进 run 上下文（010 F6 靶 · R-1）：第 2 次及以后的尝试期间
        // 若失败被 `map_intervention` 收成质量失败，那条 WARN 读到的必须是**本次**尝试号，
        // 而不是上一次失败时留下的陈旧值。
        crate::observability::set_attempt(attempt);
        let result = execute(attempt);
        if let Err(e) = &result {
            log.push(format!("第{attempt}次: {e}"));
        }
        let retryable = is_infra_failure(&result) && !control.is_cancelled();
        // 原始错误全量可见（010 F6 靶 · 需求 1）：每次尝试失败先落一条**分级**条目——
        // 质量类失败落 WARN，其余落 ERROR；落盘点在**决定重试之前**（先记错误、再决定重启）。
        if let Err(e) = &result {
            let (message, as_warn) = if e.is_quality_failure() {
                ("run 尝试失败（质量类，不重启）", true)
            } else if matches!(e, DevforgeError::Config(_)) {
                // 审计修复 #2：配置类失败与质量类同款分级（WARN + 不重启）——
                // 配置问题重启永远不会变好，ERROR 会误导成基础设施故障。
                ("run 尝试失败（配置类，不重启）", true)
            } else {
                ("run 尝试失败", false)
            };
            log_attempt_failed_shape("run-retry", message, as_warn, attempt, e, retryable);
        }
        if !retryable || attempt >= policy.max_attempts {
            // 多次尝试后仍失败：错误串必须带每次尝试的末段，否则事后无法定位是哪一次怎么挂的
            let final_result = if attempt > 1 && log.len() > 1 && result.is_err() {
                Err(DevforgeError::System(format!(
                    "{}（共尝试 {attempt} 次；尝试记录：{}）",
                    log.last().cloned().unwrap_or_default(),
                    log.join(" / ")
                )))
            } else {
                result
            };
            // 尝试耗尽的终态条目：含总尝试次数与最终错误全文（与尝试级条目同形状）
            if attempt >= policy.max_attempts
                && let Err(e) = &final_result
            {
                log_attempt_exhausted(attempt, e);
            }
            return (final_result, attempt);
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

/// 尝试级失败条目（原始错误全量可见 · 需求 1）。
///
/// 分级：质量类失败（`DevforgeError::Quality`）落 **WARN**，其余一切失败落 **ERROR**。
/// 字段与 `map_intervention` 的质量分支**同构**（`component` / `event` / `attempt` /
/// `error` / `error_chain` / `retryable`）——两处字段名一致，事后按字段过滤才对得上。
/// `run.attempt_failed` 条目的**形状单点**（010 F6 靶）：字段集（component / event / attempt /
/// error / error_chain / retryable）只在这里构造一次；两条落点（重试层 / SOP 层
/// [`map_intervention`] 的质量分支）传值——改一处漏一处的漂移从机制上不可能。
pub(crate) fn log_attempt_failed_shape(
    component: &str,
    message: &'static str,
    as_warn: bool,
    attempt: u32,
    e: &DevforgeError,
    retryable: bool,
) {
    let error_chain = crate::observability::error_chain(e);
    if as_warn {
        tracing::warn!(
            component,
            event = "run.attempt_failed",
            attempt,
            error = %e,
            error_chain = %error_chain,
            retryable,
            "{message}"
        );
    } else {
        tracing::error!(
            component,
            event = "run.attempt_failed",
            attempt,
            error = %e,
            error_chain = %error_chain,
            retryable,
            "{message}"
        );
    }
}

/// 尝试耗尽的终态条目（含总尝试次数与最终错误全文）。
fn log_attempt_exhausted(attempt: u32, e: &DevforgeError) {
    let error_chain = crate::observability::error_chain(e);
    if e.is_quality_failure() {
        tracing::warn!(
            component = "run-retry",
            event = "run.attempt_exhausted",
            attempts = attempt,
            error = %e,
            error_chain = %error_chain,
            "run 尝试耗尽（质量类）"
        );
    } else {
        tracing::error!(
            component = "run-retry",
            event = "run.attempt_exhausted",
            attempts = attempt,
            error = %e,
            error_chain = %error_chain,
            "run 尝试耗尽"
        );
    }
}

/// 干预性死亡映射为明确的 Failed 原因（而非底层 RPC EOF 噪音）。横切策略单点
/// （ARC-R5 自 BuiltinRpcEngine 平移，方法 → 自由函数：它不读引擎状态，与具体 SOP 无关）。
/// 六处 SOP 接线（planning/coding/testing/doc-fix/doc/change）统一调这里。
pub fn map_intervention(
    control: &RunControl,
    component: &str,
    e: DevforgeError,
) -> crate::error::Result<RunOutcome> {
    if control.is_cancelled() {
        Ok(RunOutcome::Failed {
            reason: "已取消（cancel 端点）".into(),
        })
    } else if control.is_stalled() {
        Ok(RunOutcome::Failed {
            reason: "已失活（会话无进展超过闲置阈值，被 watchdog 终止）".into(),
        })
    } else if control.timed_out(crate::domain::now_ms()) {
        Ok(RunOutcome::Failed {
            reason: "已超时（run 超时，被 watchdog 终止）".into(),
        })
    } else if let DevforgeError::Quality(msg) = &e {
        // 质量失败（审查打回/协议违例达上限/越界申报）：明确的 Failed，**不进自动重启**（010 D004）
        tracing::warn!(component, reason = %msg, "SOP 质量失败（不重试）");
        // 原始错误全量可见（010 F6 靶 · 需求 1）：质量类失败在生产链路上到不了重试层
        // （这里被转成 `Ok(Failed)`），不在这儿落就永远看不见——形状经 log_attempt_failed_shape
        // 单点构造（ARC-R5：与重试层同一份字段集，不再两处手写）。
        log_attempt_failed_shape(
            component,
            "SOP 质量失败（原始错误已落盘，不重启）",
            true,
            crate::observability::attempt(),
            &e,
            false,
        );
        Ok(RunOutcome::Failed {
            reason: msg.clone(),
        })
    } else if let DevforgeError::Config(msg) = &e {
        // 审计修复 #2：配置失败（缺 profile/缺 API key/模型名错误）与质量失败同款处理——
        // 明确的 Failed、不进自动重启（修好配置才有意义，重启只会重复同一失败）。
        tracing::warn!(component, reason = %msg, "SOP 配置失败（不重试，修配置后重启 run）");
        log_attempt_failed_shape(
            component,
            "SOP 配置失败（不重启，修配置后重启 run）",
            true,
            crate::observability::attempt(),
            &e,
            false,
        );
        Ok(RunOutcome::Failed {
            reason: msg.clone(),
        })
    } else {
        tracing::warn!(component, error = %e, "SOP 非干预性失败");
        Err(e)
    }
}

/// 重启编排的记账（ARC-R5 自 server::run_in_background 闭包体平移）：attempt 递增落盘
/// （审计用；断点本体由各 SOP 自己维护）+ 重启 WARN + run.retry 事件。
/// 事件推送经 `notify_retry` 回调注入（server 的 emit 是 UI 中继能力，不反向依赖）。
pub fn run_attempt_bookkeeping(
    plans_root: &std::path::Path,
    project: &str,
    task_id: &str,
    sop: &str,
    run_id: &str,
    attempt: u32,
    notify_retry: impl FnOnce(serde_json::Value),
) {
    // attempt 递增落盘（审计用；断点本体由各 SOP 自己维护）
    let store = crate::runstate::StateStore::new(
        plans_root.to_path_buf(),
        crate::runstate::StateKey::new(project, task_id, sop),
    );
    let mut rs = match store.load() {
        crate::runstate::Loaded::Found(s) => s,
        _ => store.fresh_state(),
    };
    rs.attempt = attempt;
    if let Err(e) = store.save(&mut rs) {
        tracing::warn!(component = "run", error = %e, "attempt 落盘失败");
    }
    tracing::warn!(
        component = "run",
        attempt,
        project = %project,
        task = %task_id,
        "基础设施故障，自动重启 run（从断点继续）"
    );
    notify_retry(serde_json::json!({
        "type": "run.retry",
        "project": project,
        "task": task_id,
        "run_id": run_id,
        "attempt": attempt,
    }));
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
}

#[cfg(test)]
mod log_capture_tests {
    //! 「原始错误全量可见」的行为断言：钉**日志面**（JSONL 条目的级别与字段），
    //! 不是钉内部实现（不数函数被调了几次）。
    //!
    //! 错实现形态（任一命中即红）：
    //! ① 基础设施故障落 WARN（应 ERROR）/ 质量失败落 ERROR（应 WARN）；
    //! ② 落盘时缺 `error_chain`（source 链看不见）/ 缺 `attempt` / `retryable`；
    //! ③ 尝试耗尽时少一条 `run.attempt_exhausted`（或次数字段错）。

    use super::*;

    use crate::observability::test_support::capture_jsonl;

    #[test]
    fn infra_failures_are_logged_as_error_with_the_full_error_text() {
        let text = capture_jsonl(|| {
            let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
                Err(DevforgeError::System(format!("第 {n} 次挂了")))
            });
            assert_eq!(attempts, 4);
            assert!(result.is_err());
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed"))
            .collect();
        assert_eq!(lines.len(), 4, "4 次尝试应各落一条尝试级条目：{text}");
        for (i, line) in lines.iter().enumerate() {
            let attempt = i + 1;
            assert!(line.contains("\"level\":\"ERROR\""), "基础设施故障应落 ERROR：{line}");
            assert!(line.contains("\"component\":\"run-retry\""), "{line}");
            assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
            assert!(line.contains(&format!("\"attempt\":{attempt}")), "{line}");
            assert!(line.contains("\"retryable\":true"), "第 1 次失败后仍会重启：{line}");
            assert!(line.contains("第"), "缺原始错误全文：{line}");
            assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
        }
        let exhausted: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_exhausted"))
            .collect();
        assert_eq!(exhausted.len(), 1, "应有且仅有一条终态条目：{text}");
        assert!(exhausted[0].contains("\"attempts\":4"), "终态条目要带尝试次数：{}", exhausted[0]);
        assert!(exhausted[0].contains("\"level\":\"ERROR\""), "{}", exhausted[0]);
    }

    #[test]
    fn quality_failures_are_logged_as_warn_and_are_not_retried() {
        let text = capture_jsonl(|| {
            let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
                Err(DevforgeError::Quality("审查打回：缺验收标准".into()))
            });
            assert_eq!(attempts, 1, "质量失败不得重启");
            assert!(result.is_err());
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed"))
            .collect();
        assert_eq!(lines.len(), 1, "质量失败应落一条条目：{text}");
        assert!(lines[0].contains("\"level\":\"WARN\""), "质量失败应落 WARN：{}", lines[0]);
        assert!(lines[0].contains("\"retryable\":false"), "质量失败不重启：{}", lines[0]);
        assert!(lines[0].contains("\"attempt\":1"), "{}", lines[0]);
        assert!(lines[0].contains("缺验收标准"), "缺原始错误全文：{}", lines[0]);
        assert!(lines[0].contains("error_chain"), "缺 source 链字段：{}", lines[0]);
    }

    /// 审计修复 #2 回归：配置类失败（缺 API key 等 success:false）不进自动重启——
    /// 错实现形态：command_failure 返回 System → is_infra_failure 命中 → 盲重启 4 次
    /// （退避 10s/30s/90s），配置问题重启永远不会变好。
    #[test]
    fn config_failures_are_not_retried_and_logged_as_warn() {
        let text = capture_jsonl(|| {
            let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
                Err(DevforgeError::Config("缺 API key（配置问题）".into()))
            });
            assert_eq!(attempts, 1, "配置失败不得重启：{attempts}");
            assert!(result.is_err());
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed"))
            .collect();
        assert_eq!(lines.len(), 1, "配置失败应落一条条目：{text}");
        assert!(lines[0].contains("\"level\":\"WARN\""), "{}", lines[0]);
        assert!(lines[0].contains("\"retryable\":false"), "配置失败不重启：{}", lines[0]);
        assert!(lines[0].contains("缺 API key"), "缺原始错误全文：{}", lines[0]);
    }

    #[test]
    fn the_retry_layer_updates_the_run_context_attempt() {
        // 错实现形态：重试层不更新 run 上下文 → `map_intervention` 读到的 attempt 不是本次的
        let id = "run-attempt";
        // 观测根按 run 注入到临时目录 → 不写用户 HOME
        let dir = tempfile::tempdir().unwrap();
        crate::observability::begin_run_in(dir.path().to_path_buf(), id, "coding");
        let (_result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            Err(DevforgeError::System("挂了".into()))
        });
        assert_eq!(attempts, 4);
        let snap = crate::observability::current().expect("run 上下文应存在");
        assert_eq!(snap.attempt, 4, "run 上下文的 attempt 应与本次尝试一致");
        assert_eq!(crate::observability::attempt(), 4, "attempt 读取口应给同一个值");
        crate::observability::end_run(id);
    }
}
