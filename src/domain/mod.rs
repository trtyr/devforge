//! 领域层（P004 · 项目/任务/SOP 三层管理）。
//!
//! 结构：项目 → 任务（绑定 SOP，run 执行实例）→ SOP（内置写死，decisions/002）。
//! 生命周期与重跑留档语义见 P004 decisions/001-003 与 open-questions Q004。

pub mod project;
pub mod run;
pub mod contract;
pub mod runner;
pub mod sop;
pub mod store;
pub mod task;

pub use project::Project;
pub use run::{Run, RunId, RunStatus};
pub use runner::{RunControl, RunEngine, RunOutcome};
pub use sop::{Sop, SopRegistry, PLANNING_SOP};
pub use store::Store;
pub use task::{Task, TaskStatus};

/// 当前时间的 epoch 毫秒（实体的 created_at/started_at 用）。
///
/// 自审 minor（26）：时钟异常（机器时间早于 epoch）旧口径 `unwrap_or(0)` 静默产出
/// 1970 时间戳，而 created_at/started_at 会直接参与超时与新鲜度比较（“证据晚于本轮开始”
/// 这类判定会全部错判）。现在：降级到单调时钟并大声告警，不静默变 1970。
pub fn now_ms() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as u64,
        Err(e) => {
            let fallback = monotonic_ms();
            tracing::error!(
                component = "domain",
                event = "clock.before_epoch",
                error = %e,
                fallback_ms = fallback,
                "系统时间早于 epoch（时钟异常）——退到单调时钟，时间戳不再是墙钟"
            );
            fallback
        }
    }
}

/// 进程内单调毫秒（仅当时钟异常时的降级值；不声称是墙钟）。
fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}
