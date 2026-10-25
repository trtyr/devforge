//! 审计报告组装（P013 M2 · T2.2/T2.3）：跨单元去重 + 严重度分级 + MD 报告。

use super::finding::{Finding, Severity, View};
use std::collections::BTreeMap;

/// 跨单元去重（T2.2）：**同位置（path+line+view+归一化 claim）** 完全重复 → 合并；
/// 同位置同视角但主张不同 → 保留（同一位置可以存在多个不同问题）。
///
/// 全仓审计 M1：旧键是 `(path, line, view)`，同位置同视角不同主张被当重复合并，
/// 且 severity 不参比较 —— critical 可被 nit 顶掉（与注释承诺相反）。
/// 现键加归一化 claim；真重复仍合并，但 **severity 取更重**（防御性：重复条目间的
/// 严重度分歧不应因证据长短而丢重的那个）。
pub fn dedup(findings: &[Finding]) -> Vec<Finding> {
    let mut by_key: BTreeMap<(String, u32, String, String), Finding> = BTreeMap::new();
    for f in findings {
        let key = (
            f.path.clone(),
            f.line,
            serde_json::to_string(&f.view).unwrap_or_default(),
            normalize_claim(&f.claim),
        );
        match by_key.get_mut(&key) {
            Some(existing) => {
                // 完全重复（同位置同视角同主张）：severity 取更重（Critical < Major < …）
                if f.severity < existing.severity {
                    existing.severity = f.severity;
                }
                if f.evidence.len() > existing.evidence.len() {
                    existing.evidence = f.evidence.clone();
                    existing.batch = f.batch.clone();
                }
            }
            None => {
                by_key.insert(key, f.clone());
            }
        }
    }
    by_key.into_values().collect()
}

/// claim 归一（去重键用）：小写 + 压缩空白 —— 只是格式差异的主张算同一条。
fn normalize_claim(claim: &str) -> String {
    let mut out = String::with_capacity(claim.len());
    let mut prev_space = false;
    for ch in claim.trim().to_lowercase().chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
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
            // 全仓审计 m3：表格单元转义（含 | 或换行会错行）；view 用 kebab-case
            // 与 JSON 口径一致（旧实现 {:?} 打的是 `Correctness`）
            md.push_str(&format!(
                "| `{}` | {} | {} | {} | {} |\n",
                format!("{}:{}", f.path, f.line),
                view_name(f.view),
                cell(&f.claim),
                cell(&f.evidence),
                cell(&f.batch),
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

/// Markdown 表格单元转义（全仓审计 m3）。
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
        .replace(['\n', '\r'], " ")
}

/// view 的 JSON 口径名（kebab-case）。
fn view_name(v: View) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
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
        let md = render_markdown("p", "t", &[f], &[], &[], &[]);
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
}
