//! 任务层（P004）：项目下的工作单元，绑定一个 SOP 跑流水线。

use std::path::PathBuf;
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
    /// 依赖未就绪/准入拒绝（P012 M4）：调度器恢复联动自动迁出，非终态。
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// 任务标识（项目内唯一，kebab-case，如 `mvp-from-zero`）。
    pub id: String,
    /// 所属项目名。
    pub project: String,
    /// 绑定的内置 SOP 标识（decisions/002：SOP 内置写死；同任务可跨 SOP 先后触发，P005）。
    pub sop: crate::domain::sop::SopRef,
    /// 代码工作区 = 项目登记的 local_path（decisions/001：代码归项目层，黑板只放过程资产）。
    /// 创建任务时从项目快照；老数据缺省为空（编码 SOP 会拒跑并提示）。
    #[serde(default)]
    pub workspace: PathBuf,
    pub status: TaskStatus,
    pub created_at_ms: u64,
    /// 当前（最新）run。
    pub current_run: Option<crate::domain::run::RunId>,
    /// 全部 run 历史——重跑留档（Q004）。
    pub runs: Vec<crate::domain::run::RunId>,
    /// 工作项契约（P012 M0）：创建时提交不可变；老数据缺省 None（无契约 = 不做边界终校验）。
    #[serde(default)]
    pub contract: Option<crate::domain::contract::WorkItemContract>,
    /// 依赖边（P012 M4 变更 DAG）：依赖任务全部 Completed 后本任务才可被调度。
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 独占窗口（P012 M4）：运行期间冻结其他含冲突文件任务的派工（首批简化：运行中冻结全部其他派工）。
    #[serde(default)]
    pub exclusive: bool,
}

/// 创建时的依赖图声明（P012 T4.1）：依赖边 + 独占标记，校验后落入 Task。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskDeps {
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub exclusive: bool,
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
            workspace: PathBuf::new(),
            status: TaskStatus::Pending,
            created_at_ms,
            current_run: None,
            runs: Vec::new(),
            contract: None,
            depends_on: Vec::new(),
            exclusive: false,
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
#[path = "task_tests.rs"]
mod tests;
