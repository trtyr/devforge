//! 任务层（P004）：项目下的工作单元，绑定一个 SOP 跑流水线。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 已创建，未跑过。
    Pending,
    /// 有 run 正在执行。
    Running,
    /// 最近一次 run 正常完成。
    Completed,
    /// 最近一次 run 失败（可重跑，重跑 = 新 run）。
    Failed,
    /// 归档（终态，不再执行）。
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// 任务标识（项目内唯一，kebab-case，如 `mvp-from-zero`）。
    pub id: String,
    /// 所属项目名。
    pub project: String,
    /// 绑定的内置 SOP 标识（decisions/002：SOP 内置写死）。
    pub sop: crate::domain::sop::SopRef,
    pub status: TaskStatus,
    pub created_at_ms: u64,
    /// 当前（最新）run。
    pub current_run: Option<crate::domain::run::RunId>,
    /// 全部 run 历史——重跑留档（Q004）。
    pub runs: Vec<crate::domain::run::RunId>,
}

impl Task {
    pub fn new(
        id: impl Into<String>,
        project: impl Into<String>,
        sop: impl Into<String>,
        created_at_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            project: project.into(),
            sop: sop.into(),
            status: TaskStatus::Pending,
            created_at_ms,
            current_run: None,
            runs: Vec::new(),
        }
    }

    /// 是否允许发起新 run（重跑语义）。
    pub fn can_begin_run(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Pending | TaskStatus::Completed | TaskStatus::Failed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_is_pending_without_runs() {
        let t = Task::new("mvp", "devforge", "planning", 1);
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(t.can_begin_run());
        assert!(t.runs.is_empty());
        assert!(t.current_run.is_none());
    }

    #[test]
    fn archived_task_cannot_run() {
        let mut t = Task::new("mvp", "devforge", "planning", 1);
        t.status = TaskStatus::Archived;
        assert!(!t.can_begin_run());
    }
}
