//! Run 执行实例（P004）：task 是逻辑单元，run 是一次执行。
//!
//! 重跑语义（P004 open-questions Q004 默认）：重跑 = 新 run，旧 run 留档可溯，
//! 任务当前产物指向最新 run。

use serde::{Deserialize, Serialize};

use crate::domain::now_ms;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    /// 正常结束（剧本内完成或剧本内失败都已 settle——终态）。
    Settled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub task_project: String,
    pub task_id: String,
    pub status: RunStatus,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

impl Run {
    pub fn new(task_project: impl Into<String>, task_id: impl Into<String>) -> Self {
        Self {
            id: RunId::generate(),
            task_project: task_project.into(),
            task_id: task_id.into(),
            status: RunStatus::Running,
            started_at_ms: now_ms(),
            ended_at_ms: None,
        }
    }
}
