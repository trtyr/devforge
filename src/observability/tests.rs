//! （2026-10-26 测试分离：自 src/observability/mod.rs 内联平移，纯移动零改动。）
use super::*;
use crate::error::Context as _;

/// 地基批次 #2 回归：agent 活性戳按 run 分槽——A 的旧戳不得算到 B 头上
/// （旧实现进程级单槽，前一 run 静默超阈值时下一 run 首跳即误报 agent.stall）。
#[test]
fn agent_liveness_slot_is_per_run() {
    let dir = tempfile::tempdir().unwrap();
    begin_run_in_deferred(dir.path().to_path_buf(), "run-A", "coding");
    note_agent_event();
    assert!(agent_event_age_for("run-A").is_some(), "A 应有自己的戳");
    assert!(
        agent_event_age_for("run-B").is_none(),
        "B 未开跑不得有戳（不得继承进程级旧戳）"
    );
    end_run("run-A");
    assert!(agent_event_age_for("run-A").is_none(), "A 结束后槽必须摘掉");

    begin_run_in_deferred(dir.path().to_path_buf(), "run-B", "coding");
    assert!(
        agent_event_age_for("run-B").is_none(),
        "B 不得继承 A 的旧戳"
    );
    // B 没自己的事件 → stall_check 走「按开跑时刻起算」那条路
    assert!(stall_check("run-B").is_some());
    end_run("run-B");
}

#[test]
fn root_prefers_the_injected_value() {
    let dir = tempfile::tempdir().unwrap();
    set_root(dir.path());
    assert_eq!(root(), dir.path());
}

#[test]
fn resolve_root_prefers_devforge_home_then_home() {
    // 纯函数口径：DEVFORGE_HOME > HOME/.devforge；空串视作未设
    assert_eq!(
        resolve_root(Some("/x/devforge"), Some("/home/u")),
        PathBuf::from("/x/devforge")
    );
    assert_eq!(
        resolve_root(None, Some("/home/u")),
        PathBuf::from("/home/u/.devforge")
    );
    assert_eq!(
        resolve_root(Some("  "), Some("/home/u")),
        PathBuf::from("/home/u/.devforge")
    );
    assert_eq!(resolve_root(None, None), PathBuf::from(".devforge"));
}

#[test]
fn run_context_carries_phase_step_and_attempt() {
    let id = "run-ctx-1";
    let dir = tempfile::tempdir().unwrap();
    begin_run_in(dir.path().to_path_buf(), id, "coding");
    set_phase("testing");
    set_step("review:change-plan-review");
    set_attempt(3);
    let snap = current().expect("run 上下文应存在");
    assert_eq!(snap.run_id, id);
    assert_eq!(snap.phase, "testing");
    assert_eq!(snap.step, "review:change-plan-review");
    assert_eq!(snap.attempt, 3);
    assert_eq!(attempt(), 3, "attempt 从同一处读");
    end_run(id);
    assert!(current().is_none(), "run 结束后上下文应清位");
}

#[test]
fn the_step_of_the_current_run_is_written_into_the_heartbeat() {
    // 错实现形态：步骤不进心跳（只有阶段进）→ 死后只知道阶段，不知道卡在哪个环节
    let dir = tempfile::tempdir().unwrap();
    let id = "run-ctx-step";
    // 观测根**按 run 注入**（不抢进程级全局 → 与并行单测不互相干扰）
    begin_run_in(dir.path().to_path_buf(), id, "plan");
    set_step("review:change-coding-review");
    let hb = heartbeat::read(dir.path(), id).expect("心跳应已落盘");
    assert_eq!(hb.phase, "plan");
    assert_eq!(hb.step, "review:change-coding-review");
    end_run(id);
}

#[test]
fn run_context_is_scoped_to_the_current_thread() {
    // 错实现形态：run 上下文是进程级单例（别的线程/别的 run 会串扰）→ 本用例必须变红
    let id = "run-ctx-2";
    let dir = tempfile::tempdir().unwrap();
    begin_run_in(dir.path().to_path_buf(), id, "plan");
    let other = std::thread::spawn(|| current().is_none()).join().unwrap();
    assert!(other, "别的线程不该看到本线程的 run 上下文");
    end_run(id);
}

#[test]
fn review_round_counts_per_role_and_resets_per_run() {
    let id = "run-ctx-3";
    let dir = tempfile::tempdir().unwrap();
    begin_run_in(dir.path().to_path_buf(), id, "plan");
    assert_eq!(next_review_round("change-plan-review"), 1);
    assert_eq!(next_review_round("change-plan-review"), 2);
    assert_eq!(next_review_round("change-coding-review"), 1);
    begin_run_in(dir.path().to_path_buf(), id, "plan");
    assert_eq!(next_review_round("change-plan-review"), 1, "新 run 的计数从 1 起");
    end_run(id);
}

#[test]
fn error_chain_keeps_every_source_layer() {
    let inner: crate::error::Result<()> = Err(DevforgeError::Quality("判负".into()));
    let wrapped = inner.ctx("审查轮").unwrap_err();
    let chain = error_chain(&wrapped);
    assert!(chain.contains("审查轮"), "缺上下文层：{chain}");
    assert!(chain.contains("判负"), "缺源头层：{chain}");
    assert!(chain.contains('→'), "层与层之间要有连接符：{chain}");
}

#[test]
fn quality_error_chain_has_the_top_level_text() {
    let e = DevforgeError::Quality("协议违例达上限".into());
    let chain = error_chain(&e);
    assert!(chain.contains("协议违例达上限"), "{chain}");
    assert!(!chain.is_empty());
}
