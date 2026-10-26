//! （2026-10-26 测试分离：自 src/error.rs 内联平移，纯移动零改动。）
use super::*;
use std::error::Error as _;

#[test]
fn display_includes_context_and_source() {
    let err: DevforgeError = std::fs::read_to_string("/nonexistent/file")
        .ctx("读取 plan tree")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("读取 plan tree"), "缺上下文: {msg}");
    assert!(msg.contains("IO"), "缺源头: {msg}");
}

#[test]
fn source_chain_walkable() {
    let err: DevforgeError = serde_json::from_str::<String>("not json")
        .ctx("解析 RPC 记录")
        .unwrap_err();
    // 链：WithContext -> Json
    let source = err.source().expect("应有 source");
    assert!(source.to_string().contains("JSON"));
}

#[test]
fn agent_failure_is_classified() {
    let e = DevforgeError::agent("plan-review", "进程异常退出");
    assert!(e.is_agent_failure());
    assert!(e.to_string().contains("plan-review"));

    let sys = DevforgeError::System("台账损坏".into());
    assert!(!sys.is_agent_failure());
    assert!(!sys.is_quality_failure());

    let q = DevforgeError::Quality("审查连续违例".into());
    assert!(q.is_quality_failure());
    assert!(!q.is_agent_failure());
}

/// 地基批次 #1（与引擎批次 #6 同根）：`.ctx()` 包装后分类仍要认得出——
/// 旧实现用 `matches!(self, ...)` 只看顶层，Quality/Agent 一包就变「非质量错误」。
#[test]
fn classification_survives_context_wrapping() {
    let q = "审查连续违例".to_string();
    let wrapped: crate::error::Result<()> = Err(DevforgeError::Quality(q));
    let e = wrapped.ctx("Plan 对话").unwrap_err();
    assert!(e.is_quality_failure(), "包装后仍应判质量失败：{e:?}");

    let wrapped: crate::error::Result<()> =
        Err(DevforgeError::agent("plan-review", "进程异常退出"));
    let a = wrapped.ctx("Plan 对话").unwrap_err();
    assert!(a.is_agent_failure(), "包装后仍应判 agent 失败：{a:?}");

    let wrapped: crate::error::Result<()> = Err(DevforgeError::System("台账损坏".into()));
    let s = wrapped.ctx("写断点").unwrap_err();
    assert!(!s.is_quality_failure());
}
