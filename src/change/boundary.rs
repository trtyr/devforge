//! 越界二分与往返（P009 T009 / T010c）。
//!
//! 口径（D007/D008/D018）：
//! - **越界**有两种：coding 超出**正确**计划的边界（范围膨胀）、或**计划本身漏了**必须改的文件。
//!   区分信号只有一个：**有没有先回 plan 申报**。
//! - 越界 + `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界，
//!   补完重进 coding；越界不申报 → FAIL。
//! - 往返上限 3 次，第 4 次必须停下（防止边界被一轮轮撑大）。

use crate::error::{DevforgeError, Result};
use std::fmt::Write as _;

/// 允许的 plan↔coding 往返次数。
pub const MAX_ROUNDTRIPS: usize = 3;

/// 一条「计划漏了」的申报。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactGap {
    pub file: String,
    pub reason: String,
    pub evidence: String,
}

/// 边界判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryVerdict {
    /// 改动全在白名单内。
    InBoundary,
    /// 越界且已申报 → 回 plan 补边界。
    /// `reason` 是 coding 回复的散文理由（自审 major：空 gaps 回规划时，
    /// 旧实现把 coding 的回退理由整段丢弃，plan agent 拿到无内容指令空转）。
    RoutePlan { gaps: Vec<ImpactGap>, reason: String },
    /// 一段越界且没申报（或申报不完整）→ 范围膨胀，FAIL。
    OutOfBoundary { extra: Vec<String> },
}

/// 解析 `<route>...</route>`：**只认尾部协议位**。
///
/// 口径（D026 · Q1/Q5）：route 块独占一行，其后只允许 `<issues>` / `<impact-gap>` 协议块与空白；
/// 块之前允许任意散文。散文里**提及** `<route>` 不算（c5 场的死亡真因：模型正文里的
/// 「未触发 `<route>coding</route>`」被当成了回退指令 → 空 issues → 占位文案 → Quality）。
pub fn parse_route(text: &str) -> Option<String> {
    let mut rest = text.trim_end();
    // 剥掉尾部的协议块（`<issues>` / `<impact-gap>`，可多个）——route 块之后只允许它们
    loop {
        let mut stripped = false;
        for (open, close) in [("<issues>", "</issues>"), ("<impact-gap>", "</impact-gap>")] {
            let Some(end) = rest.rfind(close) else {
                continue;
            };
            if !rest[end + close.len()..].trim().is_empty() {
                continue; // 协议块之后还有别的 → 不是尾部协议位
            }
            let Some(start) = rest[..end].rfind(open) else {
                continue;
            };
            rest = rest[..start].trim_end();
            stripped = true;
            break;
        }
        if !stripped {
            // 自审 critical（unit-048）：被截断的 <impact-gap>（无闭合标签但延伸到
            // 文末）也算协议尾巴——否则「route=plan + 被截断的申报」会让 route
            // 识别失败，合法回退被硬判范围膨胀。截断块定义上延伸到文末，后面无内容。
            if let Some(start) = rest.rfind("<impact-gap>") {
                let after = &rest[start + "<impact-gap>".len()..];
                if !after.contains("</impact-gap>") {
                    rest = rest[..start].trim_end();
                    stripped = true;
                }
            }
        }
        if !stripped {
            break;
        }
    }
    // 尾部必须是一行独立的 route 块（块内只有路由值）
    let line = rest.rsplit('\n').next()?.trim();
    let inner = line.strip_prefix("<route>")?.strip_suffix("</route>")?;
    let value = inner.trim();
    if value.is_empty() || value.contains(['<', '>']) {
        return None;
    }
    Some(value.to_string())
}

/// 解析 `<impact-gap>` 块（每条形如 `- 文件：x` / `- 为什么必须改：y` / `- 证据：z:12`）。
///
/// 自审 critical（unit-048）：**只认最后一个块**（与 route 的尾部协议位同口径）——
/// 旧实现 `text.find` 取全文第一个块，正文里引用的协议示例块会劫持解析：
/// 真申报被丢弃（误判 FAIL），或越界文件被示例块凭空合法化。
pub fn parse_impact_gaps(text: &str) -> Vec<ImpactGap> {
    // 自审 minor（11）：与 parse_route 同口径——只认尾部协议位。旧实现 rfind 全文任意位置
    // 都认，正文里引用的格式示例块（恰好排最后）即被当成正式申报。
    let trimmed = text.trim_end();
    let Some(start) = trimmed.rfind("<impact-gap>") else {
        return Vec::new();
    };
    {
        let tail = &trimmed[start..];
        let closed_to_end = match tail.rfind("</impact-gap>") {
            Some(close_at) => tail[close_at + "</impact-gap>".len()..].trim().is_empty(),
            // 被截断的块（无闭合标签）定义上延伸到文末
            None => true,
        };
        if !closed_to_end {
            tracing::warn!(
                component = "change-boundary",
                event = "boundary.gap_ignored",
                "impact-gap 块不在回复尾部（尾部仍有正文）——按尾位纪律不认这次申报"
            );
            return Vec::new();
        }
    }
    let text = trimmed;
    let body_start = start + "<impact-gap>".len();
    let body_end = text[body_start..]
        .find("</impact-gap>")
        .map(|i| body_start + i)
        .unwrap_or(text.len());
    let body = &text[body_start..body_end];

    let mut gaps: Vec<ImpactGap> = Vec::new();
    // 自审 minor（42）：理由/证据写在「文件」行之前时旧实现直接丢弃 → 合法申报被判范围膨胀。
    // 现在先攒 pending，遇到文件行再落地（字段顺序不敏感）。
    let mut pending_reason = String::new();
    let mut pending_evidence = String::new();
    for line in body.lines() {
        let t = line.trim().trim_start_matches(['-', '*']).trim();
        let Some((key, value)) = t.split_once([':', '：']) else {
            continue;
        };
        let value = value
            .trim()
            // 自审 major：字段值净化——剥包裹用的反引号/引号（plan 里写
            // 「- 文件：`src/x.rs`」时旧实现拿 `` `src/x.rs` `` 去比对，恒不等 →
            // 合法申报被判范围膨胀）。
            .trim_matches(|c| matches!(c, '`' | '"' | '\''))
            .trim()
            .to_string();
        match key.trim() {
            "文件" | "file" | "File" => {
                // 自审 minor（39）：文件字段零校验——带空白/绝对路径/协议尖括号的值不可能是
                // 仓内相对路径，直接丢弃（否则归一化后也匹配不上，只会误导诊断）。
                if !is_plausible_repo_path(&value) {
                    continue;
                }
                // 自审 minor（41）：注释与实现对齐——不完整条目统一在末尾按三字段过滤，
                // 这里不再单独 pop（旧注释声称「没有理由/证据就丢掉」，实现只查 reason）。
                gaps.push(ImpactGap {
                    file: value,
                    reason: std::mem::take(&mut pending_reason),
                    evidence: std::mem::take(&mut pending_evidence),
                });
            }
            "为什么必须改" | "理由" | "reason" => {
                if let Some(last) = gaps.last_mut() {
                    last.reason = value;
                } else {
                    pending_reason = value;
                }
            }
            "证据" | "evidence" => {
                if let Some(last) = gaps.last_mut() {
                    last.evidence = value;
                } else {
                    pending_evidence = value;
                }
            }
            _ => {}
        }
    }
    // 只要完整条目：文件 + 理由 + 证据
    gaps.into_iter()
        .filter(|g| !g.file.is_empty() && !g.reason.is_empty() && !g.evidence.is_empty())
        .collect()
}

/// 判定一次 coding 产出的边界情况。
///
/// `changed` 来自 `git diff --name-only <baseline>`；`whitelist` 来自计划。
/// 比对前两侧先归一化（复验 #7：去 `./` 前缀 + 迁移表换算），白名单匹配
/// 与申报覆盖都按归一化后的精确路径相等——不再子串包含（`src/a.rs.bak`
/// 不得覆盖 `src/a.rs`）。
/// 申报里的文件值是否像一个仓内相对路径（自审 minor 39：零校验会让垃圾值也进白名单）。
fn is_plausible_repo_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(char::is_whitespace)
        && !value.contains(['<', '>'])
        && !value.starts_with('/')
        && !value.contains("..")
}

/// 归一化单路径：去 `./` 前缀 + 迁移表换算（比对前两侧同处理，复验 #7）。
fn normalize_one(f: &str) -> &str {
    let stripped = f.strip_prefix("./").unwrap_or(f);
    crate::change::plan_audit::normalize_path(stripped)
}

pub fn classify(changed: &[String], whitelist: &[String], reply: &str) -> BoundaryVerdict {
    let verdict = classify_inner(changed, whitelist, reply);
    // 自审 minor（13）：boundary 是 change 五道机械门里唯一不落事件的——越界清单只埋在
    // Quality 错误文本里，日志无法按字段还原判定结论。
    let (kind, extra) = match &verdict {
        BoundaryVerdict::InBoundary => ("in_boundary", 0usize),
        BoundaryVerdict::RoutePlan { gaps, .. } => ("route_plan", gaps.len()),
        BoundaryVerdict::OutOfBoundary { extra } => ("out_of_boundary", extra.len()),
    };
    tracing::info!(
        component = "change-boundary",
        event = "boundary.verdict",
        verdict = kind,
        changed = changed.len(),
        extra_or_gaps = extra,
        "边界判定"
    );
    verdict
}

fn classify_inner(changed: &[String], whitelist: &[String], reply: &str) -> BoundaryVerdict {
    let norm_whitelist: Vec<&str> = whitelist.iter().map(|w| normalize_one(w)).collect();
    let declared_route = parse_route(reply).as_deref() == Some("plan");
    let extra: Vec<String> = changed
        .iter()
        .filter(|f| !norm_whitelist.iter().any(|w| *w == normalize_one(f)))
        .cloned()
        .collect();
    // 引擎审计修复 #1：route=plan 优先于 extra 判定——agent 请求回规划时无论有无越界
    // 都走 RoutePlan 分支（回 plan 重新规划），不得借「extra 为空 → InBoundary」
    // 带着白名单内改动直通 testing（跳过自检/步提交/审查三门）。
    // 有越界时申报仍须覆盖每个越界文件（不能拿 route token 当万能钥匙）。
    if declared_route {
        let gaps = parse_impact_gaps(reply);
        let reason = prose_reason(reply);
        // 自审 critical（unit-048）：截断尾巴在身时不再拿覆盖检查硬 FAIL——
        // 申报被截断 = 无法核实，但 route=plan 的决策本身有效；回 plan
        // （往返上限兜底）比谎称「范围膨胀」诚实。
        let truncated_tail = has_trailing_truncated_gap(reply);
        let covered = extra
            .iter()
            .all(|f| gaps.iter().any(|g| normalize_one(&g.file) == normalize_one(f)));
        if extra.is_empty() || covered || truncated_tail {
            return BoundaryVerdict::RoutePlan { gaps, reason };
        }
        return BoundaryVerdict::OutOfBoundary { extra };
    }
    // 非 route=plan：维持原语义——extra 空才算干净；非空一律范围膨胀
    //（申报路线只认 route=plan，其他 route token 或裸 impact-gap 不算申报）。
    if extra.is_empty() {
        return BoundaryVerdict::InBoundary;
    }
    BoundaryVerdict::OutOfBoundary { extra }
}

/// 回复是否以被截断的 `<impact-gap>`（无闭合标签）结尾。
fn has_trailing_truncated_gap(text: &str) -> bool {
    match text.rfind("<impact-gap>") {
        Some(start) => !text[start + "<impact-gap>".len()..].contains("</impact-gap>"),
        None => false,
    }
}

/// 自审 major：抽取 coding 回复的散文理由（首个协议标签之前的正文），
/// 供空 gaps 回规划时给 plan agent 当上下文；截到 300 字。
fn prose_reason(text: &str) -> String {
    let cut = text.find('<').unwrap_or(text.len());
    let reason = text[..cut].trim();
    if reason.is_empty() {
        return String::new();
    }
    reason.chars().take(300).collect()
}

/// 第 `n` 次往返是否还允许（n 从 1 开始）。
pub fn check_roundtrip(n: usize) -> Result<()> {
    if n > MAX_ROUNDTRIPS {
        return Err(DevforgeError::Quality(format!(
            "plan↔coding 又请求了第 {n} 次往返（上限 {MAX_ROUNDTRIPS}，已完成 {MAX_ROUNDTRIPS} 次）——停下来问人，不再自动扩边界"
        )));
    }
    Ok(())
}

/// 把申报的文件并进白名单（引擎用于「补完边界后重新校验」与审计展示）。
pub fn merge_gaps_into_whitelist(whitelist: &mut Vec<String>, gaps: &[ImpactGap]) {
    // 自审 minor（45）：旧口径按**原始字符串**比较再 sort+dedup——同一文件用两种拼写
    // （`src/a.rs` 与 `./src/a.rs`）能同时进白名单，且 sort 打乱了计划原有的白名单顺序
    // （顺序对审计展示有意义）。现在按归一化路径去重，保持原顺序追加。
    for g in gaps {
        let key = normalize_one(&g.file);
        if !whitelist.iter().any(|w| normalize_one(w) == key) {
            whitelist.push(g.file.clone());
        }
    }
}

/// 打回 plan 时给 plan agent 的指令：把这些缺口补进计划，然后重新交审。
/// `reason` 是 coding 的回退散文理由（可能为空）。
pub fn replan_instruction(gaps: &[ImpactGap], roundtrip: usize, reason: &str) -> String {
    let mut out = format!(
        "coding 阶段请求回规划（第 {roundtrip}/{MAX_ROUNDTRIPS} 次往返）。请只补边界，不要重写整份计划：\n\n"
    );
    if gaps.is_empty() {
        // 自审 major：无申报缺口的回规划（方案不可行等）——旧实现给 plan agent
        // 一条无内容指令空转；现在显式告知「无边界缺口」并携带 coding 的理由。
        out.push_str("无边界缺口申报（coding 未申报具体文件缺口）。");
    }
    for g in gaps {
        let _ = writeln!(out, "- `{}`：{}（证据：{}）", g.file, g.reason, g.evidence);
    }
    out.push_str(
        "\n要求：更新「影响面」与「三张表」，把上述文件写进白名单，并在「风险」里记录这次缺口。\
补完重新交审（审查只审增量，仍不看代码）。",
    );
    if !reason.is_empty() {
        let _ = write!(out, "\n\ncoding 的回退理由（原文）：{reason}");
    }
    out
}

#[cfg(test)]
#[path = "boundary_tests.rs"]
mod tests;
