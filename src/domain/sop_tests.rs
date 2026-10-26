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
