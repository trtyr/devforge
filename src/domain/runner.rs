//! 串行执行循环（P004 · decisions/003：架构并发、实现先串行）。
//!
//! Runner 泛型注入执行引擎（trait [`RunEngine`]）——task-4 的 RPC 编排引擎将实现它。
//! MVP 串行 = Store 层的全局串行守卫 + 同步顺序调用；并发放开时换调度器，引擎不变。

use crate::domain::run::Run;
use crate::domain::store::{FinishOutcome, Store};
use crate::domain::task::Task;
use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

/// 单 run 的干预控制面（P8-ISSUE-01 修复：取消端点 + 可选超时的载体）。
/// 零硬限制哲学：deadline 默认 None（不限时）；超时/取消均为显式外部干预。
/// 超时机制重设计（006 decisions/002）：墙钟预算（显式 opt-in）与失活收割
/// （会话无进展，显式 opt-in）分离；stalled 标记由 watchdog 在失活收割时置位，
/// 让 map_intervention 给出明确的 Failed 原因而非底层 RPC 噪音。
pub struct RunControl {
    /// 取消标志（cancel 端点置位）。
    pub cancel: AtomicBool,
    /// 绝对截止时间（epoch ms）；None = 不限时。
    pub deadline_ms: Option<u64>,
    /// 失活标志：watchdog 判定「会话无进展超过闲置阈值」并收割时置位。
    pub stalled: AtomicBool,
}

impl Default for RunControl {
    fn default() -> Self {
        Self::new()
    }
}

impl RunControl {
    pub fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            deadline_ms: None,
            stalled: AtomicBool::new(false),
        }
    }

    pub fn with_deadline(mut self, deadline_ms: u64) -> Self {
        self.deadline_ms = Some(deadline_ms);
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn timed_out(&self, now_ms: u64) -> bool {
        matches!(self.deadline_ms, Some(dl) if now_ms >= dl)
    }

    /// watchdog 失活收割时置位。
    pub fn mark_stalled(&self) {
        self.stalled.store(true, Ordering::SeqCst);
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled.load(Ordering::SeqCst)
    }
}

/// 单 run 执行引擎抽象。P001 的 RPC 引擎（spawn pi 子进程跑 SOP）实现本 trait。
pub trait RunEngine {
    /// 执行一次 run。Err = 技术故障（如子进程无法启动）；Ok(Failed) = 剧本内失败。
    /// control：取消/超时干预面（引擎应周期性检查；阻塞读类引擎配合 watchdog 杀进程）。
    fn execute(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Completed,
    Failed { reason: String },
}

pub struct Runner<E: RunEngine> {
    pub engine: E,
}

impl<E: RunEngine> Runner<E> {
    pub fn new(engine: E) -> Self {
        Self { engine }
    }

    /// 跑一个任务的一次 run：begin → execute → finish。
    /// 全程遵守 Store 的生命周期守卫（串行守卫/状态机/重跑留档）。
    pub fn run_task(
        &mut self,
        store: &mut Store,
        project: &str,
        task_id: &str,
        control: &RunControl,
    ) -> Result<RunOutcome> {
        let run = store.begin_run(project, task_id)?;
        tracing::info!(
            component = "runner",
            run_id = %run.id.0,
            project,
            task = task_id,
            "run 开始"
        );

        // 引擎需要任务快照；store 的可变借用在此先释放
        let task = store
            .get_task(project, task_id)
            .cloned()
            .expect("begin_run 已校验存在");

        let result = self.engine.execute(&task, &run, control);

        match result {
            Ok(outcome) => {
                let finish = match &outcome {
                    RunOutcome::Completed => FinishOutcome::Completed,
                    RunOutcome::Failed { reason } => FinishOutcome::Failed {
                        reason: reason.clone(),
                    },
                };
                store.finish_run(&run.id, finish)?;
                tracing::info!(component = "runner", run_id = %run.id.0, "run 结束");
                Ok(outcome)
            }
            Err(e) => {
                // 技术故障：run 与任务都按失败收尾，再把错误抛给调用方决策
                store.finish_run(
                    &run.id,
                    FinishOutcome::Failed {
                        reason: e.to_string(),
                    },
                )?;
                tracing::error!(component = "runner", run_id = %run.id.0, error = %e, "run 技术故障");
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sop::SopRegistry;
    use crate::domain::{Project, TaskStatus};
    use std::path::PathBuf;

    struct MockEngine {
        /// Ok = 正常返回该结果；Err(()) = 技术故障（mock 内固定构造 agent 错误）
        result: Result<RunOutcome, ()>,
        seen_runs: Vec<String>,
    }

    impl RunEngine for MockEngine {
        fn execute(&mut self, _task: &Task, run: &Run, _control: &RunControl) -> Result<RunOutcome> {
            self.seen_runs.push(run.id.0.clone());
            match &self.result {
                Ok(o) => Ok(o.clone()),
                Err(()) => Err(crate::error::DevforgeError::agent("plan-agent", "进程起不来")),
            }
        }
    }

    fn setup() -> (Store, Runner<MockEngine>) {
        let mut store = Store::new();
        store
            .create_project(Project::new("devforge", PathBuf::from("/d"), 1))
            .unwrap();
        store
            .create_task(&SopRegistry::builtin(), "devforge", "mvp", "planning", None, Default::default())
            .unwrap();
        let runner = Runner::new(MockEngine {
            result: Ok(RunOutcome::Completed),
            seen_runs: Vec::new(),
        });
        (store, runner)
    }

    #[test]
    fn happy_path_updates_lifecycle() {
        let (mut store, mut runner) = setup();
        let out = runner
            .run_task(&mut store, "devforge", "mvp", &RunControl::new())
            .unwrap();
        assert_eq!(out, RunOutcome::Completed);
        let task = store.get_task("devforge", "mvp").unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.runs.len(), 1);
    }

    #[test]
    fn script_failure_marks_task_failed_but_is_ok() {
        let (mut store, mut runner) = setup();
        runner.engine.result = Ok(RunOutcome::Failed {
            reason: "review 三连打回".into(),
        });
        let out = runner
            .run_task(&mut store, "devforge", "mvp", &RunControl::new())
            .unwrap();
        assert_eq!(
            out,
            RunOutcome::Failed {
                reason: "review 三连打回".into()
            }
        );
        assert_eq!(store.get_task("devforge", "mvp").unwrap().status, TaskStatus::Failed);
    }

    #[test]
    fn technical_error_fails_run_and_propagates() {
        let (mut store, mut runner) = setup();
        runner.engine.result = Err(());
        let err = runner
            .run_task(&mut store, "devforge", "mvp", &RunControl::new())
            .unwrap_err();
        assert!(err.is_agent_failure());
        // 技术故障也要落终态，不留悬空 Running
        assert!(!store.has_running_run());
        assert_eq!(store.get_task("devforge", "mvp").unwrap().status, TaskStatus::Failed);
    }

    #[test]
    fn rerun_appends_history() {
        let (mut store, mut runner) = setup();
        runner.engine.result = Ok(RunOutcome::Failed {
            reason: "第一次失败".into(),
        });
        runner
            .run_task(&mut store, "devforge", "mvp", &RunControl::new())
            .unwrap();
        runner.engine.result = Ok(RunOutcome::Completed);
        runner
            .run_task(&mut store, "devforge", "mvp", &RunControl::new())
            .unwrap();

        let task = store.get_task("devforge", "mvp").unwrap();
        assert_eq!(task.runs.len(), 2);
        assert_eq!(task.status, TaskStatus::Completed);
    }
}
