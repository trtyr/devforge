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
// 自审 minor（19）：状态枚举的线上词汇统一成 kebab-case——旧口径以 PascalCase 序列化
//（"Running"/"Settled"/"Failed"），与仓库其余 API 词汇（kebab/snake/dot）不一致。
// 旧值 alias 只为**读回 2026-10-27 之前落盘的 store.json**（数据迁移，不是接口双权威）；
// 序列化永远只产出新词汇。
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    #[serde(alias = "Running")]
    Running,
    /// 正常结束（剧本内完成——终态）。剧本内失败落 [`RunStatus::Failed`]。
    #[serde(alias = "Settled")]
    Settled,
    #[serde(alias = "Failed")]
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
    /// 失败原因（自审 major：旧实现收下 FinishOutcome 的 reason 后只落状态，
    /// 原因既不落库也不进日志，重启后无法复盘）。serde default = 旧记录兼容。
    #[serde(default)]
    pub reason: Option<String>,
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
            reason: None,
        }
    }
}
