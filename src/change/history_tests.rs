//! （2026-10-26 测试分离：自 src/change/history.rs 内联平移，纯移动零改动。）
use super::*;

fn gap(file: &str) -> ImpactGap {
    ImpactGap {
        file: file.into(),
        reason: "接口在这".into(),
        evidence: format!("{file}:9"),
    }
}

#[test]
fn first_roundtrip_creates_history_with_header() {
    let tmp = tempfile::tempdir().unwrap();
    let path = append_roundtrip(tmp.path(), "change-20261020-x", 1, &[gap("src/extra.rs")])
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# 变更往返历史"), "{text}");
    assert!(text.contains("| 1 |"), "{text}");
    assert!(text.contains("src/extra.rs"), "{text}");
}

#[test]
fn later_roundtrips_append_without_losing_history() {
    let tmp = tempfile::tempdir().unwrap();
    append_roundtrip(tmp.path(), "c1", 1, &[gap("a.rs")]).unwrap();
    let path = append_roundtrip(tmp.path(), "c1", 2, &[gap("b.rs")]).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("a.rs") && text.contains("b.rs"), "{text}");
    assert_eq!(text.matches("# 变更往返历史").count(), 1, "表头只一次");
    assert!(text.contains("| 2 |"), "{text}");
}

#[test]
fn multiple_gaps_in_one_roundtrip_all_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let path = append_roundtrip(tmp.path(), "c1", 1, &[gap("a.rs"), gap("b.rs")]).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("| 1 |").count(), 2, "{text}");
}
