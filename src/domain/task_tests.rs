//! （2026-10-26 测试分离：自 src/domain/task.rs 内联平移，纯移动零改动。）
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
