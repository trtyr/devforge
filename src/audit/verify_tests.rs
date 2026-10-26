//! （2026-10-26 测试分离：自 src/audit/verify.rs 内联平移，纯移动零改动。）
use super::*;
use crate::audit::finding::{Severity, View};

fn finding(path: &str, line: u32) -> Finding {
    Finding {
        path: path.into(),
        line,
        view: View::Correctness,
        severity: Severity::Major,
        claim: "测试发现".into(),
        evidence: format!("{path}:{line} 处存在问题"),
        batch: "unit-001".into(),
    }
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/real.rs"), "line1\nline2\nline3\n").unwrap();
    tmp
}

/// 第三方 r3 遗留：「猜行号」不再能进报告——只写 path:line 不引原文 → 丢弃；
/// 引用目标行原文 → 通过。
#[test]
fn evidence_must_quote_the_source_line() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/real.rs"),
        "let value = compute();\nlet b = 2;\n",
    )
    .unwrap();
    let mut guess = finding("src/real.rs", 1);
    guess.evidence = "src/real.rs:1 这里可能有问题".into();
    let v = verify(tmp.path(), &[guess]);
    assert!(v.kept.is_empty(), "未引原文应被丢弃：{:?}", v.kept);
    assert!(v.dropped[0].1.contains("未引用原文"), "{}", v.dropped[0].1);

    let mut quoted = finding("src/real.rs", 1);
    quoted.evidence = "`let value = compute();`（src/real.rs:1）算完没校验".into();
    assert_eq!(verify(tmp.path(), &[quoted]).kept.len(), 1, "引用原文应通过");
}

#[test]
fn ghost_file_findings_are_dropped() {
    let r = fixture();
    let findings = vec![finding("src/ghost.rs", 5), finding("src/real.rs", 2)];
    let v = verify(r.path(), &findings);
    assert_eq!(v.kept.len(), 1);
    assert_eq!(v.kept[0].path, "src/real.rs");
    assert_eq!(v.dropped.len(), 1);
    assert!(v.dropped[0].1.contains("不存在"), "{}", v.dropped[0].1);
}

#[test]
fn out_of_range_line_is_dropped() {
    let r = fixture();
    let findings = vec![finding("src/real.rs", 99)];
    let v = verify(r.path(), &findings);
    assert!(v.kept.is_empty());
    assert!(v.dropped[0].1.contains("行号越界"), "{}", v.dropped[0].1);
}

#[test]
fn valid_findings_pass_with_ghost_counting() {
    let r = fixture();
    let findings = vec![
        finding("src/real.rs", 1),
        finding("src/real.rs", 3),
        finding("src/nope.rs", 1),
    ];
    let v = verify(r.path(), &findings);
    assert_eq!(v.kept.len(), 2);
    assert_eq!(v.dropped.len(), 1);
}

#[test]
fn evidence_ref_to_nonexistent_file_is_dropped() {
    let r = fixture();
    let mut f = finding("src/real.rs", 2);
    // 自身引用在位（过「证据指错位置」门），幽灵引用触发「引用越界」门
    f.evidence = "src/real.rs:2 参照 src/ghost.rs:1 的实现".into();
    let v = verify(r.path(), &[f]);
    assert!(v.kept.is_empty(), "证据引用幽灵文件应整条丢弃");
    assert!(v.dropped[0].1.contains("文件不存在"), "{}", v.dropped[0].1);
}

/// 自审 major 回归：证据全部指向别的文件 = 「读过再报」失效，整条丢弃。
#[test]
fn evidence_pointing_only_at_other_files_is_dropped() {
    let r = fixture();
    let mut f = finding("src/real.rs", 2);
    f.evidence = "参照 src/other.rs:1 的实现".into();
    std::fs::write(r.path().join("src/other.rs"), "fn other() {}\n").unwrap();
    let v = verify(r.path(), &[f]);
    assert!(v.kept.is_empty(), "证据不指自身位置应丢弃");
    assert!(
        v.dropped[0].1.contains("证据指错位置"),
        "{}",
        v.dropped[0].1
    );
}

/// 审计修复 #7：路径穿越（../ 或绝对路径）不读盘直接丢弃，原因标「越界」非「不存在」
/// ——核验器只做存在性检查，不能被当成任意路径读盘器。
#[test]
fn traversal_paths_are_rejected_without_disk_read() {
    let r = fixture();
    let findings = vec![finding("../../etc/passwd", 1), finding("/etc/passwd", 1)];
    let v = verify(r.path(), &findings);
    assert!(v.kept.is_empty(), "穿越路径不得进入 kept");
    assert_eq!(v.dropped.len(), 2);
    for (loc, reason) in &v.dropped {
        assert!(reason.contains("路径越界"), "{loc}: {reason}");
        assert!(!reason.contains("不存在"), "应静态拒绝而非当幽灵：{reason}");
    }
}

/// 审计修复 #9：行号 0 与越界同样丢弃（1-based 约定下 0 必非法）。
#[test]
fn zero_line_is_dropped() {
    let r = fixture();
    let v = verify(r.path(), &[finding("src/real.rs", 0)]);
    assert!(v.kept.is_empty(), "line=0 不得进 kept");
    assert!(v.dropped[0].1.contains("行号越界"), "{}", v.dropped[0].1);
}

/// 审计修复 #4：兑现「零证据不入」——空 evidence 或无 path:line 引用的散文
/// 直接丢弃（M0 起推给核验器至今未实现，结构合法就能进报告的口子被关上）。
#[test]
fn empty_or_referenceless_evidence_is_dropped() {
    let r = fixture();
    let mut no_evidence = finding("src/real.rs", 1);
    no_evidence.evidence = "   ".into();
    let mut prose_only = finding("src/real.rs", 2);
    prose_only.evidence = "看起来这里有些问题，建议检查一下".into(); // 无任何 path:line 引用
    let v = verify(r.path(), &[no_evidence, prose_only]);
    assert!(v.kept.is_empty(), "零证据发现不得进 kept：{:?}", v.kept);
    assert_eq!(v.dropped.len(), 2);
    for (_, reason) in &v.dropped {
        assert!(reason.contains("零证据"), "{reason}");
    }
}
