//! （2026-10-26 测试分离：自 src/change/state.rs 内联平移，纯移动零改动。）
use super::*;
use crate::runstate::StateKey;

fn store(root: &std::path::Path) -> StateStore {
    StateStore::new(
        root,
        StateKey::new("proj", "task", CHANGE_SOP),
    )
}

#[test]
fn roundtrip_persists_phase_baseline_and_counters() {
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let mut st = ChangeState::new("change-20261020-x", "abc123");
    st.phase = ChangePhase::Coding;
    st.bump_roundtrip();
    st.mark_acceptance_done("x=1 → 2");
    st.bump_review();
    save(&s, &st).unwrap();

    let got = load(&s, "change-20261020-x", "abc123");
    assert_eq!(got, st);
}

#[test]
fn checkpoint_lives_on_blackboard_path() {
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let st = ChangeState::new("c1", "sha1");
    save(&s, &st).unwrap();
    let path = s.path();
    assert!(
        path.ends_with("proj/task/change/state.json"),
        "断点应落黑板：{}",
        path.display()
    );
    assert!(path.is_file());
}

#[test]
fn missing_checkpoint_means_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let st = load(&s, "c1", "sha1");
    assert_eq!(st.phase, ChangePhase::Plan);
    assert_eq!(st.roundtrips, 0);
    assert_eq!(st.baseline_sha, "sha1");
}

#[test]
fn resume_keeps_roundtrips_across_sessions() {
    // 模拟「testing 中途进程退出」：同一黑板再打开，往返计数不丢
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let mut st = ChangeState::new("c1", "sha1");
    st.bump_roundtrip();
    st.bump_roundtrip();
    st.phase = ChangePhase::Testing;
    save(&s, &st).unwrap();

    // 新会话（新 StateStore，同路径）续跑
    let s2 = StateStore::new(
        tmp.path(),
        StateKey::new("proj", "task", CHANGE_SOP),
    );
    let resumed = load(&s2, "c1", "sha1");
    assert_eq!(resumed.roundtrips, 2, "往返计数必须跨会话保留");
    assert_eq!(resumed.phase, ChangePhase::Testing, "从 testing 继续");
}

#[test]
fn counters_are_idempotent() {
    let mut st = ChangeState::new("c1", "sha1");
    st.mark_acceptance_done("a");
    st.mark_acceptance_done("a");
    assert_eq!(st.acceptance_done, vec!["a".to_string()]);
}

#[test]
fn failure_context_persists_across_sessions() {
    // 错实现形态：上下文只活在内存里（不落盘）→ 续跑丢明细，本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let mut st = ChangeState::new("c1", "sha1");
    st.set_test_failure(
        1,
        vec![
            "tests/change_ask_ring.rs:557 断言失败".to_string(),
            "接口返回 500".to_string(),
        ],
    );
    save(&s, &st).unwrap();

    // 新会话（新 StateStore，同一黑板路径）续跑：上下文仍在、明细逐字不变
    let s2 = StateStore::new(tmp.path(), StateKey::new("proj", "task", CHANGE_SOP));
    let got = load(&s2, "c1", "sha1");
    let ctx = got.last_test_failure.expect("失败上下文必须跨会话保留");
    assert_eq!(ctx.round, 1, "轮次要原样保留");
    assert_eq!(
        ctx.issues,
        vec![
            "tests/change_ask_ring.rs:557 断言失败".to_string(),
            "接口返回 500".to_string()
        ],
        "失败明细必须逐条原文保留（含 path:line）"
    );
}

#[test]
fn old_checkpoint_without_failure_key_still_loads() {
    // 错实现形态：新字段没加 default / 用了非 Option → 旧断点反序列化失败被 .ok() 吞掉
    // → 断点冷启动（计数归零）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let s = store(tmp.path());
    let mut rs = s.fresh_state();
    // 旧版断点：change 段里没有失败上下文这个键
    rs.data = serde_json::json!({"change": {
        "phase": "coding",
        "baseline_sha": "sha1",
        "roundtrips": 2,
        "impl_steps_done": [1, 2],
        "acceptance_done": ["x=1 → 2"],
        "review_rounds": 1,
        "test_reroutes": 2,
        "change_id": "c1"
    }});
    s.save(&mut rs).unwrap();

    let got = load(&s, "c1", "sha1");
    assert_eq!(
        got.last_test_failure, None,
        "旧断点没有失败上下文 → 加载后应是「无上下文」"
    );
    assert_eq!(got.phase, ChangePhase::Coding, "旧断点不能被冷启动重置");
    assert_eq!(got.roundtrips, 2, "往返计数不能丢");
    assert_eq!(got.test_reroutes, 2, "回注计数不能丢");
    // 引擎审计修复 #10：fixture 里的 impl_steps_done 键是旧断点文件的活样本——
    // serde 忽略未知字段语义下加载成功，字段已删（死状态，活体步对账走 git log）。
    assert_eq!(got.acceptance_done, vec!["x=1 → 2".to_string()]);
}

#[test]
fn failure_context_is_overwritten_not_appended() {
    // 错实现形态：新一次回注把 issues 追加到旧列表 → 已修好的旧失败又被喂回去
    let mut st = ChangeState::new("c1", "sha1");
    st.set_test_failure(1, vec!["旧失败".to_string()]);
    st.set_test_failure(2, vec!["新失败".to_string()]);
    let ctx = st.last_test_failure.expect("应有上下文");
    assert_eq!(ctx.round, 2, "轮次要跟着更新");
    assert_eq!(ctx.issues, vec!["新失败".to_string()], "新 issues 覆盖旧的");
}

#[test]
fn clearing_failure_context_empties_it() {
    let mut st = ChangeState::new("c1", "sha1");
    st.set_test_failure(1, vec!["失败".to_string()]);
    st.clear_test_failure();
    assert_eq!(st.last_test_failure, None, "清除后应回到「无上下文」");
}

#[test]
fn foreign_state_is_ignored() {
    // 另一个 SOP 的断点不会污染 change 的扩段
    let tmp = tempfile::tempdir().unwrap();
    let other = StateStore::new(tmp.path(), StateKey::new("proj", "task", "doc-from-zero"));
    let mut rs = other.fresh_state();
    rs.data = serde_json::json!({"done": ["u1"]});
    other.save(&mut rs).unwrap();

    let s = store(tmp.path());
    let st = load(&s, "c1", "sha1");
    assert_eq!(st.roundtrips, 0);
    assert_eq!(st.change_id, "c1");
}
