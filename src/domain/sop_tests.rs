//! （2026-10-26 测试分离：自 src/domain/sop.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn builtin_sops_cover_pipeline_stages() {
    let reg = SopRegistry::builtin();
    assert_eq!(
        reg.ids(),
        vec!["planning", "coding", "testing", "doc-fix", "doc-from-zero", "change", "code-review"]
    );
    for id in reg.ids() {
        assert!(reg.validate(id).is_ok());
    }
}

#[test]
fn unknown_sop_rejected() {
    let reg = SopRegistry::builtin();
    let err = reg.validate("ghost").unwrap_err();
    assert!(err.to_string().contains("未知 SOP"));
}

// ── 2026-10-27 自审 domain 批回归 ──

/// 自审 minor（68）：空注册表必须显式构造（`empty()`），不再经 `Default` 静默得到
/// 一张「什么都不认识」的表（与 builtin() 语义相反）。
#[test]
fn empty_registry_is_explicit_and_recognizes_nothing() {
    let empty = SopRegistry::empty();
    assert!(empty.ids().is_empty());
    assert!(empty.validate("planning").is_err());
    // builtin 仍认识全部
    assert!(!SopRegistry::builtin().ids().is_empty());
}

/// 自审 minor（59）：Sop 可对外序列化（快照/日志），但不宣称可从 JSON 反序列化
/// （`&'static str` 字段做不到，旧口径的 Deserialize 派生是个假能力）。
#[test]
fn sop_serializes_for_display() {
    let v = serde_json::to_value(PLANNING_SOP).unwrap();
    assert_eq!(v["id"], "planning");
    assert!(v["name"].as_str().is_some_and(|s| !s.is_empty()));
}
