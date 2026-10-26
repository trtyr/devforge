//! （2026-10-26 测试分离：自 src/domain/runner.rs 内联平移，纯移动零改动。）
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
