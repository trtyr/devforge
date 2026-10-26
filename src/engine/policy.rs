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

/// 只有基础设施类错误值得重启（引擎审计修复 #3：分类单一事实源——剥 WithContext 包装看根因）。
/// 可重启：根因为 `System`（子进程死亡/流提前 EOF/挂死收割）或 `Io`（spawn 瞬时失败——
/// 进程表满/临时资源耗尽；二进制不存在属配置问题但 spawn 层分不清，接受重试后
/// Failed 的成本换取瞬时故障自愈，决策注释即文档）。
/// 不重启：`Quality`（重启只会得到同一结论）、`Config`（修配置后重启 run）、
/// `Agent`（子 Agent 决策类失败）、`RpcProtocol`（真实协议违例——坏数据不是外部故障）、
/// `Store`/`Json`（数据模型故障，重启不修数据）。
pub fn is_infra_failure(result: &crate::error::Result<RunOutcome>) -> bool {
    matches!(
        result.as_ref().err().map(|e| e.root_cause()),
        Some(DevforgeError::System(_) | DevforgeError::Io(_))
    )
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
    } else if let DevforgeError::Quality(msg) = e.root_cause() {
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
    } else if let DevforgeError::Config(msg) = e.root_cause() {
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
#[path = "policy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "policy_log_capture_tests.rs"]
mod log_capture_tests;
