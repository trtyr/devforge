//! 审计报告组装（P013 M2 · T2.2/T2.3）：跨单元去重 + 严重度分级 + MD 报告。

use super::finding::{Finding, Severity};
use std::collections::BTreeMap;

/// 跨单元去重（T2.2）：同位置（path+line+view）完全重复 → 合并保留主张最详尽的
/// 一条（evidence 最长）；同位置不同主张 → 保留（同一位置可以有多个不同问题）。
pub fn dedup(findings: &[Finding]) -> Vec<Finding> {
    let mut by_pos: BTreeMap<(String, u32, String), Finding> = BTreeMap::new();
    for f in findings {
        let key = (f.path.clone(), f.line, serde_json::to_string(&f.view).unwrap_or_default());
        match by_pos.get(&key) {
            Some(existing) => {
                if f.evidence.len() > existing.evidence.len() {
                    by_pos.insert(key, f.clone());
                }
            }
            None => {
                by_pos.insert(key, f.clone());
            }
        }
    }
    by_pos.into_values().collect()
}

/// 按严重度分组排序（critical → nit）。
pub fn by_severity(findings: &[Finding]) -> BTreeMap<Severity, Vec<&Finding>> {
    let mut grouped: BTreeMap<Severity, Vec<&Finding>> = BTreeMap::new();
    for f in findings {
        grouped.entry(f.severity).or_default().push(f);
    }
    grouped
}

/// Markdown 审计报告（人读版；JSON 版由 runner 直接序列化发现结构）。
pub fn render_markdown(
    project: &str,
    task: &str,
    local: &[Finding],
    cross: &[Finding],
    dropped: &[(String, String)],
    abandoned: &[String],
) -> String {
    let mut all: Vec<&Finding> = local.iter().chain(cross.iter()).collect();
    all.sort_by_key(|f| f.severity);

    let sev_name = |s: Severity| match s {
        Severity::Critical => "critical",
        Severity::Major => "major",
        Severity::Minor => "minor",
        Severity::Nit => "nit",
    };
    let counts: BTreeMap<Severity, usize> = all
        .iter()
        .fold(BTreeMap::new(), |mut m, f| {
            *m.entry(f.severity).or_default() += 1;
            m
        });

    let mut md = String::new();
    md.push_str(&format!(
        "# 全量代码审计报告 — {project}/{task}\n\n\
         > 发现总数 {}（critical {} / major {} / minor {} / nit {}）；\
         幽灵发现已核验过滤 {} 条；放弃批次 {} 个。\n\n",
        all.len(),
        counts.get(&Severity::Critical).copied().unwrap_or(0),
        counts.get(&Severity::Major).copied().unwrap_or(0),
        counts.get(&Severity::Minor).copied().unwrap_or(0),
        counts.get(&Severity::Nit).copied().unwrap_or(0),
        dropped.len(),
        abandoned.len(),
    ));

    if let Some(crit) = counts.get(&Severity::Critical) {
        md.push_str(&format!(
            "## 执行摘要\n\n⚠️ 存在 {crit} 条 critical 发现，需优先处置（见下表）。\n\n"
        ));
    } else {
        md.push_str("## 执行摘要\n\n未发现 critical 级问题。\n\n");
    }

    for sev in [
        Severity::Critical,
        Severity::Major,
        Severity::Minor,
        Severity::Nit,
    ] {
        let items = all.iter().filter(|f| f.severity == sev).collect::<Vec<_>>();
        if items.is_empty() {
            continue;
        }
        md.push_str(&format!("## {}（{} 条）\n\n", sev_name(sev), items.len()));
        md.push_str("| 位置 | 视角 | 主张 | 证据 | 批次 |\n|---|---|---|---|---|\n");
        for f in &items {
            md.push_str(&format!(
                "| `{}` | {:?} | {} | {} | {} |\n",
                format!("{}:{}", f.path, f.line),
                f.view,
                f.claim,
                f.evidence,
                f.batch,
            ));
        }
        md.push('\n');
    }

    if !dropped.is_empty() {
        md.push_str(&format!("## 核验过滤记录（{} 条幽灵发现被丢弃）\n\n", dropped.len()));
        for (pos, reason) in dropped {
            md.push_str(&format!("- `{pos}`：{reason}\n"));
        }
        md.push('\n');
    }
    if !abandoned.is_empty() {
        md.push_str(&format!("## 放弃批次（{} 个，诚实失败不入 findings）\n\n", abandoned.len()));
        for a in abandoned {
            md.push_str(&format!("- {a}\n"));
        }
    }
    md
}

#[cfg(test)]
mod tests {
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
        let a = finding("src/x.rs", 1, View::Correctness, Severity::Major, "主张 A");
        let b = finding("src/x.rs", 1, View::Security, Severity::Critical, "主张 B");
        let out = dedup(&[a, b]);
        assert_eq!(out.len(), 2, "不同视角的问题不是重复");
    }

    #[test]
    fn markdown_groups_by_severity_with_summary() {
        let local = vec![
            finding("src/a.rs", 1, View::Correctness, Severity::Critical, "e1"),
            finding("src/b.rs", 2, View::Security, Severity::Nit, "e2"),
        ];
        let md = render_markdown("p", "t", &local, &[], &[], &[]);
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
            &[],
            &[("src/ghost.rs:1".into(), "文件不存在".into())],
            &["unit-009：两次协议违例".into()],
        );
        assert!(md.contains("核验过滤记录"));
        assert!(md.contains("放弃批次"));
        assert!(md.contains("未发现 critical 级问题"));
    }
}
