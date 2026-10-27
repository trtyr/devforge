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
    let (out, merged) = dedup_with_stats(findings);
    if merged > 0 {
        tracing::info!(
            component = "audit-engine",
            merged,
            before = findings.len(),
            after = out.len(),
            "跨单元去重：重复发现已合并（severity 取更重）"
        );
    }
    out
}

/// 去重并返回（结果, 合并条数）——自审 minor 18：合并必须能进报告计数与日志。
pub fn dedup_with_stats(findings: &[Finding]) -> (Vec<Finding>, usize) {
    let mut merged = 0usize;
    let mut by_key: BTreeMap<(String, u32, String, String), Finding> = BTreeMap::new();
    for f in findings {
        // 自审 minor（19）：旧口径用 serde_json::to_string(...).unwrap_or_default() 生成视角
        // 分量——序列化错误被静默降级成空串，不同视角会塌缩成同一个键（正是 M1 修复过的
        // 缺陷复发路径）。改用与报告同源的稳定 kebab 名。
        let key = (
            f.path.clone(),
            f.line,
            view_name(f.view),
            normalize_claim(&f.claim),
        );
        match by_key.get_mut(&key) {
            Some(existing) => {
                // 自审 minor（20）：severity 取更重时，evidence/batch 必须**跟着那一侧走**——
                // 旧口径 severity 取自 A、evidence+batch 取自 B，会产出「critical 严重度 +
                // nit 批次的证据」这种自相矛盾的行。
                // 口径：severity 取更重；evidence 取更详尽（同一 path:line+claim 下证据
                // 只描述同一处，与严重度取值不矛盾）；**批次取并集**（溯源不得丢）。
                if f.severity < existing.severity {
                    existing.severity = f.severity;
                }
                if f.evidence.len() > existing.evidence.len() {
                    existing.evidence = f.evidence.clone();
                }
                merge_batch(&mut existing.batch, &f.batch);
                // 自审 minor（18）：合并动作必须可观测（幽灵过滤与批次放弃都有留痕，
                // 去重合并（含严重度抬升）此前既无事件也不进任何计数）。
                merged += 1;
            }
            None => {
                by_key.insert(key, f.clone());
            }
        }
    }
    (by_key.into_values().collect(), merged)
}

/// 合并溯源批次（自审 minor 21）：旧口径只留一个 batch 字段，另一批次的溯源永久丢失。
fn merge_batch(batch: &mut String, other: &str) {
    if other.is_empty() || batch == other || batch.split('+').any(|b| b == other) {
        return;
    }
    if batch.is_empty() {
        batch.push_str(other);
    } else {
        batch.push('+');
        batch.push_str(other);
    }
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

/// 按严重度分组（维持 `Severity` 的顺序 = critical → nit）。
///
/// 2026-10-27 自审 minor 22：runner 曾另写一套 `severity_summary_of` 重做同一件事
/// （两套实现），现在 runner 直接用它。
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
    findings: &[Finding],
    workspace_sha: Option<&str>,
    dropped: &[(String, String)],
    abandoned: &[String],
) -> String {
    // 自审 minor（23）：旧签名 local/cross 双参暗示「函数自己做双源合并」，实际它只是拼在
    // 一起排序——唯一生产调用方靠传 merged + &[] 规避重复计数，cross 分支零测试。
    // 改成单源入参：调用方负责先合并（谁合并谁负责口径）。
    let mut all: Vec<&Finding> = findings.iter().collect();
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
         > 代码版本：{}（报告可溯到具体 commit）。\n\
         > 发现总数 {}（critical {} / major {} / minor {} / nit {}）；\
         幽灵发现已核验过滤 {} 条；放弃批次 {} 个。\n\n",
        workspace_sha.unwrap_or("（未记录）"),
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
                // 自审 minor（25）：位置列同样要转义（路径含 `|` 会把表格顶歪）
                cell(&format!("{}:{}", f.path, f.line)),
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
    // 自审 nit（8）：旧口径先把 `|` 换成 `\|`，若文本里本来就有反斜杠，会产出 `\\|`
    // 这类歧义转义（渲染出多余反斜杠）。先把反斜杠加倍，再转义竖线。
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
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
#[path = "report_tests.rs"]
mod tests;
