//! （2026-10-26 测试分离：自 src/audit/report.rs 内联平移，纯移动零改动。）
use super::*;
use crate::audit::finding::View;

fn finding(path: &str, line: u32, view: View, sev: Severity, evidence: &str) -> Finding {
    Finding {
        path: path.into(),
        line,
        view,
        severity: sev,
        claim: "主张".into(),
        evidence: evidence.into(),
        batch: "unit-001".into(),
    }
}

#[test]
fn exact_duplicates_merge_keeping_richest_evidence() {
    let a = finding("src/x.rs", 1, View::Correctness, Severity::Major, "短");
    let mut b = finding("src/x.rs", 1, View::Correctness, Severity::Major, "更长的证据描述");
    b.batch = "unit-002".into();
    let out = dedup(&[a, b]);
    assert_eq!(out.len(), 1, "同位置重复应合并");
    assert!(out[0].evidence.contains("更长"), "保留证据更详尽的：{:?}", out[0].evidence);
}

#[test]
fn same_position_different_claims_are_kept() {
    // 全仓审计 M1：同位置**同视角**不同主张 = 不同问题，必须都保留——
    // 旧测试用不同视角（Correctness vs Security）绕开了真缺陷。
    let a = finding("src/x.rs", 1, View::Correctness, Severity::Major, "主张 A");
    let mut b = finding("src/x.rs", 1, View::Correctness, Severity::Critical, "主张 B");
    b.claim = "边界条件未处理".into();
    let out = dedup(&[a, b]);
    assert_eq!(out.len(), 2, "同位置同视角不同主张应保留两条：{out:?}");
    assert!(
        out.iter().any(|f| f.severity == Severity::Critical),
        "critical 不得因为与另一条同位置而被吞"
    );
}

/// 全仓审计 M1 回归：真重复（同位置同视角同主张）合并时 severity 取更重，
/// 错实现形态：只按 evidence 长度取一条 → critical 被 nit 顶掉。
#[test]
fn duplicate_merge_never_downgrades_severity() {
    let mut a = finding("src/x.rs", 7, View::Security, Severity::Nit, "超长证据描述超长证据描述");
    a.claim = "注入风险".into();
    let mut b = finding("src/x.rs", 7, View::Security, Severity::Critical, "短");
    b.claim = "注入风险".into();
    let out = dedup(&[a, b]);
    assert_eq!(out.len(), 1, "完全重复应合并");
    assert_eq!(
        out[0].severity,
        Severity::Critical,
        "合并后 severity 必须取更重的一条"
    );
    assert!(out[0].evidence.contains("超长"), "证据仍取更详尽的");
}

#[test]
fn markdown_groups_by_severity_with_summary() {
    let local = vec![
        finding("src/a.rs", 1, View::Correctness, Severity::Critical, "e1"),
        finding("src/b.rs", 2, View::Security, Severity::Nit, "e2"),
    ];
    let md = render_markdown("p", "t", &local, Some("abc1234"), &[], &[]);
    assert!(md.contains("critical 1"), "{md}");
    assert!(md.contains("## critical（1 条）"));
    assert!(md.contains("`src/a.rs:1`"));
    assert!(md.contains("未发现") == false);
}

#[test]
fn dropped_and_abandoned_are_recorded() {
    let md = render_markdown(
        "p",
        "t",
        &[],
        Some("sha-abc"),
        &[("src/ghost.rs:1".into(), "文件不存在".into())],
        &["unit-009：两次协议违例".into()],
    );
    assert!(md.contains("核验过滤记录"));
    assert!(md.contains("放弃批次"));
    assert!(md.contains("未发现 critical 级问题"));
}

/// 全仓审计 m3：表格单元转义与 view 口径——错实现形态：
/// claim/evidence 中的 `|` 与换行直接塞进表格（错行），view 用 `{:?}` 打成 `Correctness`。
#[test]
fn markdown_escapes_table_cells_and_uses_kebab_view() {
    let mut f = finding(
        "src/a.rs",
        1,
        View::Correctness,
        Severity::Major,
        "证据|带管道\n第二行",
    );
    f.claim = "主张|带管道".into();
    let md = render_markdown("p", "t", &[f], None, &[], &[]);
    assert!(md.contains("correctness"), "view 应为 kebab-case：{md}");
    assert!(!md.contains("Correctness"), "不得出现 Debug 形态：{md}");
    assert!(md.contains("主张\\|带管道"), "claim 的管道应转义：{md}");
    assert!(!md.contains("带管道\n"), "单元格内不得保留换行：{md}");
    let row = md.lines().find(|l| l.contains("src/a.rs:1")).unwrap();
    // 去掉转义管道后，剩下的才是表格结构分隔（首尾各一 + 5 列之间 = 6）
    let structural = row.replace("\\|", "");
    assert_eq!(
        structural.matches('|').count(),
        6,
        "转义后结构分隔应仍为 6 个管道：{row}"
    );
}

/// 自审 minor（18/21）：合并必须可计数，且**两个批次的溯源都要保住**（旧口径只留一个）。
#[test]
fn merge_keeps_both_batches_and_reports_count() {
    let mut a = finding("src/x.rs", 3, View::Security, Severity::Major, "短");
    a.claim = "同一主张".into();
    a.batch = "unit-001".into();
    let mut b = finding("src/x.rs", 3, View::Security, Severity::Critical, "更长证据描述");
    b.claim = "同一主张".into();
    b.batch = "unit-002".into();

    let (out, merged) = dedup_with_stats(&[a, b]);
    assert_eq!(merged, 1, "合并条数必须可计数（旧口径既无事件也不进任何计数）");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].severity, Severity::Critical);
    assert!(
        out[0].batch.contains("unit-001") && out[0].batch.contains("unit-002"),
        "两个批次的溯源都要保留：{}",
        out[0].batch
    );
}

/// 自审 minor（19）：视角分量必须用稳定 kebab 名——不同视角不得塌缩成同一个去重键。
#[test]
fn different_views_never_collapse_into_one_key() {
    let mut a = finding("src/x.rs", 5, View::Correctness, Severity::Major, "证据");
    a.claim = "同名主张".into();
    let mut b = finding("src/x.rs", 5, View::Security, Severity::Major, "证据");
    b.claim = "同名主张".into();
    let out = dedup(&[a, b]);
    assert_eq!(out.len(), 2, "不同视角是不同问题：{out:?}");
}

/// 自审 minor（24）：人读版 MD 必须带代码版本（可溯到具体 commit）。
#[test]
fn markdown_carries_workspace_sha() {
    let md = render_markdown("p", "t", &[], Some("deadbeef"), &[], &[]);
    assert!(md.contains("deadbeef"), "MD 报告缺代码版本：{md}");
    let md2 = render_markdown("p", "t", &[], None, &[], &[]);
    assert!(md2.contains("未记录"), "拿不到 sha 时也要显式说明：{md2}");
}
