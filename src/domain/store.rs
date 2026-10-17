//! 三层对象的内存存储 + JSON 持久化。
//!
//! 生命周期守卫集中在此：
//! - 系统级串行（decisions/003 实现口径）：任一时刻最多一个 Running run
//! - 任务状态机：Pending/Completed/Failed 可发起 run；Running/Archived 不可
//! - 重跑留档（Q004）：runs 只增不减，current_run 指向最新

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::run::{Run, RunId, RunStatus};
use crate::domain::sop::SopRegistry;
use crate::domain::task::{Task, TaskStatus};
use crate::domain::{now_ms, Project};
use crate::error::{Context as _, DevforgeError, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
}

/// run 结束的结果（runner 驱动 store 收尾用）。
#[derive(Debug, Clone, PartialEq)]
pub enum FinishOutcome {
    Completed,
    Failed { reason: String },
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    // ---------- 项目层 ----------

    /// 回滚插入（P1-ISSUE-01：目录落地失败时不留半注册项目）。
    pub fn remove_project(&mut self, name: &str) {
        self.projects.retain(|p| p.name != name);
    }

    pub fn create_project(&mut self, project: Project) -> Result<&Project> {
        if self.projects.iter().any(|p| p.name == project.name) {
            return Err(DevforgeError::Store(format!(
                "项目 `{}` 已存在",
                project.name
            )));
        }
        self.projects.push(project);
        Ok(self.projects.last().expect("刚 push"))
    }

    pub fn get_project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    pub fn list_projects(&self) -> &[Project] {
        &self.projects
    }

    // ---------- 任务层 ----------

    /// 创建任务：项目须存在、任务 id 项目内唯一、SOP 须已内置注册。
    pub fn create_task(
        &mut self,
        registry: &SopRegistry,
        project: &str,
        id: impl Into<String>,
        sop: impl Into<String>,
    ) -> Result<&Task> {
        let sop = sop.into();
        registry.validate(&sop)?;
        if self.get_project(project).is_none() {
            return Err(DevforgeError::Store(format!("项目 `{project}` 不存在")));
        }
        let id = id.into();
        if self
            .tasks
            .iter()
            .any(|t| t.project == project && t.id == id)
        {
            return Err(DevforgeError::Store(format!(
                "任务 `{id}` 在项目 `{project}` 下已存在"
            )));
        }
        let task = Task::new(id, project, sop, now_ms());
        let id = task.id.clone();
        self.tasks.push(task);
        Ok(self
            .tasks
            .iter()
            .find(|t| t.project == project && t.id == id)
            .expect("刚 push"))
    }

    pub fn get_task(&self, project: &str, id: &str) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| t.project == project && t.id == id)
    }

    fn get_task_mut(&mut self, project: &str, id: &str) -> Result<&mut Task> {
        self.tasks
            .iter_mut()
            .find(|t| t.project == project && t.id == id)
            .ok_or_else(|| DevforgeError::Store(format!("任务 `{project}/{id}` 不存在")))
    }

    pub fn list_tasks(&self, project: &str) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.project == project).collect()
    }

    // ---------- Run 生命周期 ----------

    /// 系统级串行守卫（decisions/003 实现口径）：任一时刻最多一个 Running run。
    pub fn has_running_run(&self) -> bool {
        self.runs.iter().any(|r| r.status == RunStatus::Running)
    }

    /// 发起 run：守卫（全局串行 + 任务状态机）通过后创建新 run 并挂到任务上。
    pub fn begin_run(&mut self, project: &str, task_id: &str) -> Result<Run> {
        if self.has_running_run() {
            return Err(DevforgeError::Store(
                "已有运行中的任务（MVP 串行执行，一次一个）".into(),
            ));
        }
        let task = self.get_task_mut(project, task_id)?;
        if !task.can_begin_run() {
            return Err(DevforgeError::Store(format!(
                "任务 `{project}/{task_id}` 状态为 {:?}，不可发起 run",
                task.status
            )));
        }
        let run = Run::new(project, task_id);
        let run_id = run.id.clone();
        self.runs.push(run);
        let task = self.get_task_mut(project, task_id)?;
        task.status = TaskStatus::Running;
        task.current_run = Some(run_id.clone());
        task.runs.push(run_id);
        Ok(self
            .runs
            .last()
            .expect("刚 push")
            .clone())
    }

    /// 收尾 run：更新 run 终态与任务状态。
    pub fn finish_run(&mut self, run_id: &RunId, outcome: FinishOutcome) -> Result<()> {
        let run = self
            .runs
            .iter_mut()
            .find(|r| &r.id == run_id)
            .ok_or_else(|| DevforgeError::Store(format!("run `{}` 不存在", run_id.0)))?;
        if run.status != RunStatus::Running {
            return Err(DevforgeError::Store(format!(
                "run `{}` 状态为 {:?}，不可收尾",
                run_id.0, run.status
            )));
        }
        run.status = match outcome {
            FinishOutcome::Completed => RunStatus::Settled,
            FinishOutcome::Failed { .. } => RunStatus::Failed,
        };
        run.ended_at_ms = Some(now_ms());

        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.project == run.task_project && t.id == run.task_id)
            .ok_or_else(|| {
                DevforgeError::Store(format!(
                    "run `{}` 的任务 `{}/{}` 不存在",
                    run_id.0, run.task_project, run.task_id
                ))
            })?;
        task.status = match outcome {
            FinishOutcome::Completed => TaskStatus::Completed,
            FinishOutcome::Failed { .. } => TaskStatus::Failed,
        };
        Ok(())
    }

    pub fn get_run(&self, run_id: &RunId) -> Option<&Run> {
        self.runs.iter().find(|r| &r.id == run_id)
    }

    pub fn list_runs(&self) -> &[Run] {
        &self.runs
    }

    // ---------- 持久化（JSON 单文件，MVP 口径） ----------

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .ctx("序列化 store")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx("创建 store 目录")?;
        }
        std::fs::write(path, json).ctx(format!("写入 store {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .ctx(format!("读取 store {}", path.display()))?;
        serde_json::from_str(&json).ctx("解析 store")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sop::PLANNING_SOP;

    fn setup() -> Store {
        let mut s = Store::new();
        s.create_project(Project::new(
            "devforge",
            std::path::PathBuf::from("/home/me/devforge"),
            1,
        ))
        .unwrap();
        s.create_task(&SopRegistry::builtin(), "devforge", "mvp", PLANNING_SOP.id)
            .unwrap();
        s
    }

    #[test]
    fn duplicate_project_rejected() {
        let mut s = setup();
        let err = s
            .create_project(Project::new("devforge", std::path::PathBuf::from("/x"), 2))
            .unwrap_err();
        assert!(err.to_string().contains("已存在"));
    }

    #[test]
    fn create_task_validates_project_and_sop() {
        let mut s = Store::new();
        s.create_project(Project::new("p", std::path::PathBuf::from("/p"), 1))
            .unwrap();
        let err = s
            .create_task(&SopRegistry::builtin(), "ghost", "t1", "planning")
            .unwrap_err();
        assert!(err.to_string().contains("不存在"));

        let err = s
            .create_task(&SopRegistry::builtin(), "p", "t1", "coding")
            .unwrap_err();
        assert!(err.to_string().contains("未知 SOP"));
    }

    #[test]
    fn system_level_serial_guard() {
        let mut s = setup();
        let r1 = s.begin_run("devforge", "mvp").unwrap();
        // 全局已有一个 Running run → 拒绝第二个（哪怕不同任务）
        let err = s.begin_run("devforge", "mvp").unwrap_err();
        assert!(err.to_string().contains("串行"));
        s.finish_run(&r1.id, FinishOutcome::Completed).unwrap();
        assert!(!s.has_running_run());
    }

    #[test]
    fn task_status_machine_guards() {
        let mut s = setup();
        s.begin_run("devforge", "mvp").unwrap();
        // 任务 Running 中不可归档重入——状态机守卫体现在 finish 后才能再跑
        let task = s.get_task("devforge", "mvp").unwrap();
        assert_eq!(task.status, TaskStatus::Running);
    }

    #[test]
    fn rerun_keeps_history_and_updates_current() {
        let mut s = setup();
        // run1: 失败
        let r1 = s.begin_run("devforge", "mvp").unwrap();
        s.finish_run(&r1.id, FinishOutcome::Failed { reason: "引擎炸了".into() })
            .unwrap();
        assert_eq!(s.get_task("devforge", "mvp").unwrap().status, TaskStatus::Failed);
        // run2: 重跑成功
        let r2 = s.begin_run("devforge", "mvp").unwrap();
        s.finish_run(&r2.id, FinishOutcome::Completed).unwrap();

        let task = s.get_task("devforge", "mvp").unwrap();
        assert_eq!(task.runs.len(), 2, "重跑应留档全部 run");
        assert_eq!(task.current_run, Some(r2.id.clone()), "current 指向最新");
        // 旧 run 留档且终态不变
        assert_eq!(s.get_run(&r1.id).unwrap().status, RunStatus::Failed);
        assert_eq!(s.get_run(&r2.id).unwrap().status, RunStatus::Settled);
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn json_roundtrip() {
        let mut s = setup();
        s.begin_run("devforge", "mvp").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        s.save(&path).unwrap();
        let loaded = Store::load(&path).unwrap();
        assert_eq!(s, loaded);
    }
}
