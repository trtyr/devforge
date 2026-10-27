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

/// 带「引用原文」的证据（夹具文件是 `let lineN = N;` 形态）。
fn quoted(path: &str, line: u32) -> Finding {
    let mut f = finding(path, line);
    f.evidence = format!("`let line{line} = {line};`（{path}:{line}）这里有问题");
    f
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    // 自审 minor（8）：短行不再整条豁免「引用原文」——夹具行必须够长才谈得上引用
    std::fs::write(
        tmp.path().join("src/real.rs"),
        "let line1 = 1;\nlet line2 = 2;\nlet line3 = 3;\n",
    )
    .unwrap();
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
    let findings = vec![finding("src/ghost.rs", 5), quoted("src/real.rs", 2)];
    let v = verify(r.path(), &findings);
    assert_eq!(v.kept.len(), 1);
    assert_eq!(v.kept[0].path, "src/real.rs");
    assert_eq!(v.dropped.len(), 1);
    assert!(v.dropped[0].1.contains("不存在"), "{}", v.dropped[0].1);
}

#[test]
fn out_of_range_line_is_dropped() {
    let r = fixture();
    let findings = vec![quoted("src/real.rs", 99)];
    let v = verify(r.path(), &findings);
    assert!(v.kept.is_empty());
    assert!(v.dropped[0].1.contains("行号越界"), "{}", v.dropped[0].1);
}

#[test]
fn valid_findings_pass_with_ghost_counting() {
    let r = fixture();
    let findings = vec![
        quoted("src/real.rs", 1),
        quoted("src/real.rs", 3),
        finding("src/nope.rs", 1),
    ];
    let v = verify(r.path(), &findings);
    assert_eq!(v.kept.len(), 2);
    assert_eq!(v.dropped.len(), 1);
}

/// 自审 minor（38）：次要引用指向**不存在**的文件不再判死整条发现（URL 端口、semver、
/// 仓外绝对路径、别处快照里的文件都会命中这一类）——只留痕忽略。
#[test]
fn evidence_ref_to_nonexistent_file_is_ignored_not_fatal() {
    let r = fixture();
    let mut f = quoted("src/real.rs", 2);
    f.evidence = format!("{}；另外参照 src/ghost.rs:1 的同类实现", f.evidence);
    let v = verify(r.path(), &[f]);
    assert_eq!(v.kept.len(), 1, "幽灵次要引用不得判死整条：{:?}", v.dropped);
}

/// 但**存在**文件的引用行号越界仍然判死（证据与代码不一致 = 在猜）。
#[test]
fn evidence_ref_line_out_of_range_is_still_fatal() {
    let r = fixture();
    let mut f = quoted("src/real.rs", 1);
    f.evidence = format!("{}；参照 src/real.rs:999 的第二处", f.evidence);
    let v = verify(r.path(), &[f]);
    assert!(v.kept.is_empty(), "引用行号越界应判死");
    assert!(v.dropped[0].1.contains("越界"), "{}", v.dropped[0].1);
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

// ── P012-T007 · AU-01：借前缀不再能过——不出现指认行的 path:line 组合即拒 ──
#[test]
fn borrowed_prefix_without_line_ref_is_dropped() {
    // 高频前缀攻击面：evidence 引同文件**另一行**（前缀同族），贴了指认行原文——
    // 旧判据（前缀 contains 就过）放行；新判据要求出现指认行的 path:line 组合。
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/m.rs"),
        "use crate::error::A;\nuse crate::error::B;\n",
    )
    .unwrap();
    let mut f = finding("src/m.rs", 2);
    f.evidence = "同族问题见 src/m.rs:1，形如 `use crate::error::B;`".into();
    let v = verify(tmp.path(), &[f]);
    assert!(v.kept.is_empty(), "借前缀不得进 kept：{:?}", v.kept);
    assert!(v.dropped[0].1.contains("未引用原文"), "{}", v.dropped[0].1);
}

#[test]
fn quoted_prefix_adjacent_to_ref_passes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/m.rs"),
        "use crate::error::A;\nuse crate::error::B;\n",
    )
    .unwrap();
    let mut f = finding("src/m.rs", 2);
    f.evidence = "src/m.rs:2 的 `use crate::error::B;` 拼写错误".into();
    let v = verify(tmp.path(), &[f]);
    assert_eq!(v.kept.len(), 1, "组合与原文紧邻应通过：{:?}", v.dropped);
}

// ── 自审 minor（37）：本文件唯二的安全分支此前没有任何回归测试 ──

/// 安全分支 A：仓内符号链接指向仓外 → 拒绝读（不把核验器变成任意路径读盘器）。
#[cfg(unix)]
#[test]
fn symlinked_path_escaping_the_repo_is_refused() {
    let r = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.rs"), "let secret = 1;\n").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.rs"),
        r.path().join("src/link.rs"),
    )
    .unwrap();

    let f = quoted("src/link.rs", 1);
    let v = verify(r.path(), &[f]);
    assert!(
        v.kept.is_empty(),
        "经符号链接越界必须拒绝读取：{:?}",
        v.kept
    );
    assert!(
        v.dropped[0].1.contains("符号链接") || v.dropped[0].1.contains("拒绝"),
        "丢弃原因要说明越界：{}",
        v.dropped[0].1
    );
}

/// 安全分支 B：证据里的**穿越引用**直接忽略（不读盘、不判死整条）。
#[test]
fn evidence_ref_with_traversal_is_ignored() {
    let r = fixture();
    let mut f = quoted("src/real.rs", 1);
    f.evidence = format!("{}；参照 ../../etc/passwd:1 与 https://x:8080/a 与 v1:2", f.evidence);
    let v = verify(r.path(), &[f]);
    assert_eq!(v.kept.len(), 1, "穿越/URL 次要引用不得判死：{:?}", v.dropped);
    assert!(
        v.dropped.is_empty(),
        "不得因为仓外引用产生丢弃记录：{:?}",
        v.dropped
    );
}

/// 自审 minor（8）：短结构行不再整条豁免引用原文——必须与 `path:line` 相邻地引用到。
#[test]
fn short_lines_still_need_an_adjacent_quote() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/s.rs"), "fn f() {\n}\n").unwrap();

    let mut unquoted = finding("src/s.rs", 2);
    unquoted.evidence = "src/s.rs:2 这里少了处理".into();
    let v = verify(tmp.path(), &[unquoted]);
    assert!(
        v.kept.is_empty(),
        "短行也要引用原文（旧口径整条豁免）：{:?}",
        v.kept
    );

    let mut quoted_ok = finding("src/s.rs", 2);
    quoted_ok.evidence = "`}`（src/s.rs:2）收尾过早".into();
    assert_eq!(
        verify(tmp.path(), &[quoted_ok]).kept.len(),
        1,
        "相邻引用短行原文应通过"
    );
}

/// 自审 minor（36）：读盘失败不得被改写成「文件不存在」（基础设施故障 vs 路径幻觉）。
#[cfg(unix)]
#[test]
fn unreadable_file_is_reported_as_infrastructure_not_ghost() {
    let r = fixture();
    // 用**目录**占位同名的 .rs：canonicalize 成功但读不出内容 → Io 类
    std::fs::create_dir_all(r.path().join("src/dir.rs")).unwrap();
    let mut f = finding("src/dir.rs", 1);
    f.evidence = "src/dir.rs:1 有问题".into();
    let v = verify(r.path(), &[f]);
    assert!(v.kept.is_empty());
    assert!(
        v.dropped[0].1.contains("基础设施故障"),
        "读失败要归类为基础设施：{}",
        v.dropped[0].1
    );
    assert!(
        !v.dropped[0].1.contains("幽灵发现"),
        "不得把读不到写成路径幻觉：{}",
        v.dropped[0].1
    );
}
