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
pub use runner::{RunEngine, RunOutcome, Runner};
pub use sop::{Sop, SopRegistry, PLANNING_SOP};
pub use store::Store;
pub use task::{Task, TaskStatus};

/// 当前时间的 epoch 毫秒（实体的 created_at/started_at 用）。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
