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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
    /// 并发 run 上限（P012 M4）：默认 1 = decisions/003 串行兼容；调度器/worktree 模式下调高。
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_runs: usize,
    /// 持久化路径（复验 backlog：store 纯内存是设计错误——重启即蒸发）。
    /// None = 内存态（单测）；Some = 每个 &mut 变更方法尾调 persist()。
    #[serde(skip)]
    pub persist_path: Option<std::path::PathBuf>,
}

fn default_max_concurrent() -> usize {
    1
}

impl Default for Store {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            tasks: Vec::new(),
            runs: Vec::new(),
            max_concurrent_runs: default_max_concurrent(),
            persist_path: None,
        }
    }
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

    /// 开启持久化：后续所有变更方法自动落盘（服务启动时用；单测不调即内存态）。
    pub fn with_persist(mut self, path: std::path::PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }

    /// 变更落盘（persist_path 未设则空操作）。失败只 WARN 不阻断（可观测优先于强一致）。
    fn persist(&self) {
        if let Some(path) = &self.persist_path {
            if let Err(e) = self.save(path) {
                tracing::warn!(
                    component = "store",
                    path = %path.display(),
                    error = %e,
                    "store 落盘失败（内存态继续，重启将丢自上次成功落盘后的变更）"
                );
            }
        }
    }

    // ---------- 项目层 ----------

    /// 回滚插入（P1-ISSUE-01：目录落地失败时不留半注册项目）。
    pub fn remove_project(&mut self, name: &str) {
        self.projects.retain(|p| p.name != name);
        self.persist();
    }

    pub fn create_project(&mut self, project: Project) -> Result<&Project> {
        if self.projects.iter().any(|p| p.name == project.name) {
            return Err(DevforgeError::Store(format!(
                "项目 `{}` 已存在",
                project.name
            )));
        }
        self.projects.push(project);
        self.persist();
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
    /// 契约（P012 M0）创建时提交不可变；None = 无契约（不做边界终校验）。
    /// 依赖图（P012 T4.1）：依赖须本项目内存在且无环（含自依赖）。
    pub fn create_task(
        &mut self,
        registry: &SopRegistry,
        project: &str,
        id: impl Into<String>,
        sop: impl Into<String>,
        contract: Option<crate::domain::contract::WorkItemContract>,
        deps: crate::domain::task::TaskDeps,
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
        // 依赖校验（T4.1）：存在性 + 无环（把待建节点也放进图一起检）。
        for dep in &deps.depends_on {
            if dep == &id {
                return Err(DevforgeError::Store(format!(
                    "任务 `{id}` 不能依赖自身"
                )));
            }
            if !self
                .tasks
                .iter()
                .any(|t| t.project == project && &t.id == dep)
            {
                return Err(DevforgeError::Store(format!(
                    "依赖任务 `{dep}` 在项目 `{project}` 下不存在"
                )));
            }
        }
        Self::assert_acyclic(self, project, &id, &deps.depends_on)?;
        let mut task = Task::new(id, project, sop, now_ms());
        task.contract = contract;
        task.depends_on = deps.depends_on;
        task.exclusive = deps.exclusive;
        // P005 decisions/001：任务快照项目登记的代码工作区（编码 SOP 用；黑板由引擎建）
        task.workspace = self
            .get_project(project)
            .map(|p| p.local_path.clone())
            .unwrap_or_default();
        let id = task.id.clone();
        self.tasks.push(task);
        self.persist();
        Ok(self
            .tasks
            .iter()
            .find(|t| t.project == project && t.id == id)
            .expect("刚 push"))
    }

    /// 环检测（T4.1）：假设 `new_id` 带 `depends_on` 建入后，项目图不得有环。
    /// DFS 三色标记；待建节点以虚拟边参与。
    fn assert_acyclic(
        &self,
        project: &str,
        new_id: &str,
        new_deps: &[String],
    ) -> Result<()> {
        use std::collections::{HashMap, VecDeque};
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for t in self.tasks.iter().filter(|t| t.project == project) {
            edges.entry(&t.id).or_default().extend(
                t.depends_on.iter().map(|s| s.as_str()),
            );
        }
        edges.entry(new_id).or_default().extend(
            new_deps.iter().map(|s| s.as_str()),
        );
        // Kahn：入度归零计数 = 无环；残图非空 = 有环
        let mut indegree: HashMap<&str, usize> = edges.keys().map(|k| (*k, 0)).collect();
        for outs in edges.values() {
            for d in outs {
                *indegree.entry(d).or_insert(0) += 1;
            }
        }
        let mut queue: VecDeque<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut visited = 0usize;
        while let Some(n) = queue.pop_front() {
            visited += 1;
            if let Some(outs) = edges.get(n) {
                for d in outs {
                    let e = indegree.get_mut(d).expect("边端点必在图内");
                    *e -= 1;
                    if *e == 0 {
                        queue.push_back(d);
                    }
                }
            }
        }
        if visited != edges.len() {
            return Err(DevforgeError::Store(format!(
                "依赖图存在环：加入 `{new_id}` 后项目 `{project}` 的 DAG 无拓扑序"
            )));
        }
        Ok(())
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

    /// 置任务状态（P012 M4 调度器专用：Blocked ⇄ Pending 恢复联动；其他调用方勿用）。
    pub fn set_task_status(
        &mut self,
        project: &str,
        id: &str,
        status: TaskStatus,
    ) -> Result<()> {
        self.get_task_mut(project, id)?.status = status;
        self.persist();
        Ok(())
    }

    pub fn list_tasks(&self, project: &str) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.project == project).collect()
    }

    /// 变更任务的 SOP（P005 跨 SOP 触发：同任务先规划后编码）。
    /// 校验注册 + 任务存在 + 状态可发起（Running/Archived 下禁改）。
    pub fn set_task_sop(
        &mut self,
        registry: &SopRegistry,
        project: &str,
        id: &str,
        sop: impl Into<String>,
    ) -> Result<()> {
        let sop = sop.into();
        registry.validate(&sop)?;
        let task = self.get_task_mut(project, id)?;
        if !task.can_begin_run() {
            return Err(DevforgeError::Store(format!(
                "任务 `{project}/{id}` 当前状态（{:?}）不可变更 SOP",
                task.status
            )));
        }
        task.sop = sop;
        self.persist();
        Ok(())
    }

    // ---------- Run 生命周期 ----------

    /// 系统级串行守卫（decisions/003 实现口径）：任一时刻最多一个 Running run。
    pub fn has_running_run(&self) -> bool {
        self.running_run_count() > 0
    }

    /// 当前 Running run 数（P012 M4 并发调度用）。
    pub fn running_run_count(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .count()
    }

    /// 发起 run：守卫（并发上限 + 任务状态机）通过后创建新 run 并挂到任务上。
    /// 并发上限：`max_concurrent_runs`（默认 1 = decisions/003 串行兼容；M4 调度器调高）。
    pub fn begin_run(&mut self, project: &str, task_id: &str) -> Result<Run> {
        if self.running_run_count() >= self.max_concurrent_runs {
            return Err(DevforgeError::Store(format!(
                "已达并发上限 {}（P012 M4：worktree 隔离下允许多 run）",
                self.max_concurrent_runs
            )));
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
        self.persist();
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
        self.persist();
        Ok(())
    }

    pub fn get_run(&self, run_id: &RunId) -> Option<&Run> {
        self.runs.iter().find(|r| &r.id == run_id)
    }

    pub fn list_runs(&self) -> &[Run] {
        &self.runs
    }

    // ---------- 持久化（JSON 单文件，MVP 口径） ----------

    /// 铁律守卫（用户 10-25 拍板）：路径命中临时目录返回违反原因，否则 None。
    /// 覆盖 /tmp、/var/tmp、/dev/shm、/run 及其子路径（前缀判定，含恰等）。
    pub fn tmp_path_violation(path: &std::path::Path) -> Option<String> {
        let p = path.to_string_lossy();
        for tmp in ["/tmp", "/var/tmp", "/dev/shm", "/run"] {
            if p == tmp || p.starts_with(&format!("{tmp}/")) {
                return Some(format!("位于临时目录 {tmp}（非持久，重启即清）"));
            }
        }
        None
    }

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
        s.create_task(&SopRegistry::builtin(), "devforge", "mvp", PLANNING_SOP.id, None, Default::default())
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
            .create_task(&SopRegistry::builtin(), "ghost", "t1", "planning", None, Default::default())
            .unwrap_err();
        assert!(err.to_string().contains("不存在"));

        // P005：coding 已入注册表（不再是未知 SOP），且任务快照工作区
        let t = s
            .create_task(&SopRegistry::builtin(), "p", "t1", "coding", None, Default::default())
            .unwrap();
        assert_eq!(t.sop, "coding");
        assert_eq!(t.workspace, std::path::PathBuf::from("/p"), "快照项目 local_path");

        let err = s
            .create_task(&SopRegistry::builtin(), "p", "t2", "ghost", None, Default::default())
            .unwrap_err();
        assert!(err.to_string().contains("未知 SOP"));

        // P005 跨 SOP：set_task_sop 校验注册 + 状态可发起
        s.set_task_sop(&SopRegistry::builtin(), "p", "t1", "planning").unwrap();
        assert_eq!(s.get_task("p", "t1").unwrap().sop, "planning");
        let err = s
            .set_task_sop(&SopRegistry::builtin(), "p", "t1", "ghost")
            .unwrap_err();
        assert!(err.to_string().contains("未知 SOP"));
    }

    #[test]
    fn system_level_serial_guard() {
        let mut s = setup();
        let r1 = s.begin_run("devforge", "mvp").unwrap();
        // 全局已有一个 Running run → 拒绝第二个（decisions/003 上限化：默认 max=1 兼容串行）
        let err = s.begin_run("devforge", "mvp").unwrap_err();
        assert!(err.to_string().contains("并发上限"), "{err}");
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

    // ---------- 变更 DAG（P012 T4.1） ----------

    use crate::domain::task::TaskDeps;

    #[test]
    fn dag_rejects_missing_dependency() {
        let mut s = setup();
        let err = s
            .create_task(
                &SopRegistry::builtin(),
                "devforge",
                "c",
                "planning",
                None,
                TaskDeps { depends_on: vec!["ghost".into()], exclusive: false },
            )
            .unwrap_err();
        assert!(format!("{err}").contains("不存在"), "应报依赖不存在：{err}");
    }

    #[test]
    fn dag_rejects_self_dependency() {
        let mut s = setup();
        let err = s
            .create_task(
                &SopRegistry::builtin(),
                "devforge",
                "mvp2",
                "planning",
                None,
                TaskDeps { depends_on: vec!["mvp2".into()], exclusive: false },
            )
            .unwrap_err();
        assert!(format!("{err}").contains("自身"), "应报自依赖：{err}");
    }

    #[test]
    fn dag_chain_creates_with_edges() {
        let mut s = setup();
        // a（无依赖）→ b 依赖 a → c 依赖 b：链式落库 + 字段回读
        s.create_task(
            &SopRegistry::builtin(),
            "devforge",
            "dag-a",
            "planning",
            None,
            TaskDeps::default(),
        )
        .unwrap();
        s.create_task(
            &SopRegistry::builtin(),
            "devforge",
            "dag-b",
            "planning",
            None,
            TaskDeps { depends_on: vec!["dag-a".into()], exclusive: true },
        )
        .unwrap();
        s.create_task(
            &SopRegistry::builtin(),
            "devforge",
            "dag-c",
            "planning",
            None,
            TaskDeps { depends_on: vec!["dag-b".into()], exclusive: false },
        )
        .unwrap();
        let b = s.get_task("devforge", "dag-b").unwrap();
        assert_eq!(b.depends_on, vec!["dag-a".to_string()]);
        assert!(b.exclusive, "独占标记应落库");
        let c = s.get_task("devforge", "dag-c").unwrap();
        assert_eq!(c.depends_on, vec!["dag-b".to_string()]);
    }

    #[test]
    fn persist_roundtrip_survives_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        let mut s = Store::new().with_persist(path.clone());
        s.create_project(Project::new("p", std::path::PathBuf::from("/x"), 1))
            .unwrap();
        s.create_task(
            &SopRegistry::builtin(),
            "p",
            "t",
            "planning",
            None,
            TaskDeps::default(),
        )
        .unwrap();
        let run = s.begin_run("p", "t").unwrap();
        s.finish_run(&run.id, FinishOutcome::Failed { reason: "x".into() })
            .unwrap();
        drop(s);
        let s2 = Store::load(&path).unwrap();
        assert_eq!(s2.projects.len(), 1, "项目应持久化");
        assert_eq!(s2.tasks.len(), 1, "任务应持久化");
        assert_eq!(s2.runs.len(), 1, "run 应持久化");
        assert_eq!(s2.tasks[0].status, TaskStatus::Failed, "终态应持久化");
    }

    #[test]
    fn tmp_paths_are_rejected_by_guard() {
        use std::path::Path;
        assert!(Store::tmp_path_violation(Path::new("/tmp/devforge")).is_some());
        assert!(Store::tmp_path_violation(Path::new("/tmp")).is_some());
        assert!(Store::tmp_path_violation(Path::new("/var/tmp/devforge-accept")).is_some());
        assert!(Store::tmp_path_violation(Path::new("/dev/shm/x")).is_some());
        assert!(Store::tmp_path_violation(Path::new("/home/u/devforge")).is_none());
        assert!(Store::tmp_path_violation(Path::new("/var/lib/devforge")).is_none());
    }

}
