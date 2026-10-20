//! 越界二分与往返（P009 T009 / T010c）。
//!
//! 口径（D007/D008/D018）：
//! - **越界**有两种：coding 超出**正确**计划的边界（范围膨胀）、或**计划本身漏了**必须改的文件。
//!   区分信号只有一个：**有没有先回 plan 申报**。
//! - 越界 + `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界，
//!   补完重进 coding；越界不申报 → FAIL。
//! - 往返上限 3 次，第 4 次必须停下（防止边界被一轮轮撑大）。

use crate::error::{DevforgeError, Result};

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
    RoutePlan { gaps: Vec<ImpactGap> },
    /// 越界且没申报（或申报不完整）→ 范围膨胀，FAIL。
    OutOfBoundary { extra: Vec<String> },
}

impl BoundaryVerdict {
    pub fn is_fail(&self) -> bool {
        matches!(self, Self::OutOfBoundary { .. })
    }
}

/// 解析 `<route>...</route>`。
pub fn parse_route(text: &str) -> Option<String> {
    let start = text.find("<route>")? + "<route>".len();
    let end = text[start..].find("</route>")? + start;
    Some(text[start..end].trim().to_string())
}

/// 解析 `<impact-gap>` 块（每条形如 `- 文件：x` / `- 为什么必须改：y` / `- 证据：z:12`）。
pub fn parse_impact_gaps(text: &str) -> Vec<ImpactGap> {
    let Some(start) = text.find("<impact-gap>") else {
        return Vec::new();
    };
    let body_start = start + "<impact-gap>".len();
    let body_end = text[body_start..]
        .find("</impact-gap>")
        .map(|i| body_start + i)
        .unwrap_or(text.len());
    let body = &text[body_start..body_end];

    let mut gaps: Vec<ImpactGap> = Vec::new();
    for line in body.lines() {
        let t = line.trim().trim_start_matches(['-', '*']).trim();
        let Some((key, value)) = t.split_once([':', '：']) else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "文件" | "file" | "File" => {
                if !gaps.is_empty() {
                    // 新条目的文件行：前一条若没有理由/证据就丢掉（不完整申报不算数）
                    if gaps.last().is_some_and(|g| g.reason.is_empty()) {
                        gaps.pop();
                    }
                }
                gaps.push(ImpactGap {
                    file: value,
                    reason: String::new(),
                    evidence: String::new(),
                });
            }
            "为什么必须改" | "理由" | "reason" => {
                if let Some(last) = gaps.last_mut() {
                    last.reason = value;
                }
            }
            "证据" | "evidence" => {
                if let Some(last) = gaps.last_mut() {
                    last.evidence = value;
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
pub fn classify(changed: &[String], whitelist: &[String], reply: &str) -> BoundaryVerdict {
    let extra: Vec<String> = changed
        .iter()
        .filter(|f| !whitelist.iter().any(|w| w == *f))
        .cloned()
        .collect();
    if extra.is_empty() {
        return BoundaryVerdict::InBoundary;
    }

    let declared_route = parse_route(reply).as_deref() == Some("plan");
    let gaps = parse_impact_gaps(reply);
    if !declared_route || gaps.is_empty() {
        return BoundaryVerdict::OutOfBoundary { extra };
    }
    // 申报必须覆盖每个越界文件，且申报里的文件必须真的越界（不能拿申报当万能钥匙）
    let covered = extra
        .iter()
        .all(|f| gaps.iter().any(|g| g.file == *f || g.file.contains(f.as_str())));
    if !covered {
        return BoundaryVerdict::OutOfBoundary { extra };
    }
    BoundaryVerdict::RoutePlan { gaps }
}

/// 第 `n` 次往返是否还允许（n 从 1 开始）。
pub fn check_roundtrip(n: usize) -> Result<()> {
    if n > MAX_ROUNDTRIPS {
        return Err(DevforgeError::Quality(format!(
            "plan↔coding 已经往返 {n} 次（上限 {MAX_ROUNDTRIPS}）——停下来问人，不再自动扩边界"
        )));
    }
    Ok(())
}

/// 把申报的文件并进白名单（引擎用于「补完边界后重新校验」与审计展示）。
pub fn merge_gaps_into_whitelist(whitelist: &mut Vec<String>, gaps: &[ImpactGap]) {
    for g in gaps {
        if !whitelist.iter().any(|w| w == &g.file) {
            whitelist.push(g.file.clone());
        }
    }
    whitelist.sort();
    whitelist.dedup();
}

/// 打回 plan 时给 plan agent 的指令：把这些缺口补进计划，然后重新交审。
pub fn replan_instruction(gaps: &[ImpactGap], roundtrip: usize) -> String {
    let mut out = format!(
        "coding 阶段发现计划缺口（第 {roundtrip}/{MAX_ROUNDTRIPS} 次往返）。请只补边界，不要重写整份计划：\n\n"
    );
    for g in gaps {
        out.push_str(&format!(
            "- `{}`：{}（证据：{}）\n",
            g.file, g.reason, g.evidence
        ));
    }
    out.push_str(
        "\n要求：更新「影响面」与「三张表」，把上述文件写进白名单，并在「风险」里记录这次缺口。\
补完重新交审（审查只审增量，仍不看代码）。",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    const REPLY_DECLARE: &str = "发现计划漏了文件。\n<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n- 为什么必须改：需求依赖的接口在这里\n- 证据：src/extra.rs:42\n</impact-gap>";

    #[test]
    fn inside_whitelist_is_clean() {
        let v = classify(
            &s(&["src/a.rs", "tests/a_test.rs"]),
            &s(&["src/a.rs", "tests/a_test.rs"]),
            "干完了\n<stage-complete>1</stage-complete>",
        );
        assert_eq!(v, BoundaryVerdict::InBoundary);
    }

    #[test]
    fn out_of_boundary_without_declaration_fails() {
        let v = classify(
            &s(&["src/a.rs", "src/extra.rs"]),
            &s(&["src/a.rs"]),
            "顺手改了一下，应该没事\n<stage-complete>1</stage-complete>",
        );
        match v {
            BoundaryVerdict::OutOfBoundary { extra } => assert_eq!(extra, s(&["src/extra.rs"])),
            other => panic!("应为范围膨胀 FAIL，实际 {other:?}"),
        }
    }

    #[test]
    fn declared_gap_routes_back_to_plan_and_whitelist_grows() {
        let mut whitelist = s(&["src/a.rs"]);
        let v = classify(&s(&["src/a.rs", "src/extra.rs"]), &whitelist, REPLY_DECLARE);
        let BoundaryVerdict::RoutePlan { gaps } = v else {
            panic!("带申报应回 plan，实际 {v:?}");
        };
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].file, "src/extra.rs");
        assert_eq!(gaps[0].evidence, "src/extra.rs:42");

        // 补边界后重算：越界消失
        merge_gaps_into_whitelist(&mut whitelist, &gaps);
        assert!(whitelist.contains(&"src/extra.rs".to_string()));
        let again = classify(&s(&["src/a.rs", "src/extra.rs"]), &whitelist, "干完了");
        assert_eq!(again, BoundaryVerdict::InBoundary);
    }

    #[test]
    fn declaration_must_cover_every_extra_file() {
        let v = classify(
            &s(&["src/extra.rs", "src/another.rs"]),
            &s(&["src/a.rs"]),
            REPLY_DECLARE,
        );
        assert!(v.is_fail(), "申报只盖一半也是范围膨胀：{v:?}");
    }

    #[test]
    fn incomplete_gap_entry_counts_as_missing_declaration() {
        let reply = "<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n</impact-gap>";
        let v = classify(&s(&["src/extra.rs"]), &s(&["src/a.rs"]), reply);
        assert!(v.is_fail(), "缺理由/证据的申报不算申报：{v:?}");
    }

    #[test]
    fn route_other_than_plan_is_not_a_declaration() {
        let reply = "<route>coding</route>\n<impact-gap>\n- 文件：src/extra.rs\n- 为什么必须改：x\n- 证据：src/extra.rs:1\n</impact-gap>";
        let v = classify(&s(&["src/extra.rs"]), &s(&["src/a.rs"]), reply);
        assert!(v.is_fail(), "{v:?}");
    }

    #[test]
    fn roundtrip_is_capped_at_three() {
        for n in 1..=MAX_ROUNDTRIPS {
            check_roundtrip(n).unwrap();
        }
        let err = check_roundtrip(MAX_ROUNDTRIPS + 1).unwrap_err();
        assert!(err.to_string().contains("上限 3"), "{err}");
        assert!(err.is_quality_failure(), "往返超限是质量失败，不该自动重启");
    }

    #[test]
    fn replan_instruction_lists_gaps_and_asks_only_for_boundary() {
        let gaps = vec![ImpactGap {
            file: "src/extra.rs".into(),
            reason: "接口在这里".into(),
            evidence: "src/extra.rs:42".into(),
        }];
        let text = replan_instruction(&gaps, 2);
        assert!(text.contains("src/extra.rs"), "{text}");
        assert!(text.contains("只补边界"), "{text}");
        assert!(text.contains("第 2/3 次往返"), "{text}");
    }

    #[test]
    fn route_parse_is_tolerant_of_whitespace() {
        assert_eq!(parse_route("<route> plan </route>").as_deref(), Some("plan"));
        assert_eq!(parse_route("没有路由块"), None);
    }
}
