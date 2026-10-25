//! doc-from-zero 协议解析（P008 · D020）。
//!
//! 协议三块（与 P005/P007 同形）：
//! - 干活方：`<doc-complete>unit-xxx</doc-complete>`
//! - 审查方：`<doc-verdict>PASS|FAIL</doc-verdict>` + `<route>phase0|phase1|phase2|phase3|finish</route>` + `<issues>`
//!
//! 保守规则（与 P007 同口径）：缺 route / 值非法 → **按 phase3 处理**（能局部改就不重跑全局）；
//! 协议块缺失/自相矛盾 → `protocol_violation = true`，由编排层重跑本轮审查而不是回注被审 Agent。

use crate::error::Result;

/// 连续协议违例上限（复用 P007 口径 C19-R1）。
pub const MAX_DOC_PROTOCOL_VIOLATIONS: usize = 3;

/// 无完成标记时的重推上限（2026-10-20）：标记由模型生成，连续不发就是卡死，
/// 不能像旧口径那样零上限空转。
pub const MAX_DOC_CONTINUE_NUDGES: usize = 3;

/// DR-8（10-23）：空响应（0 字符）重试上限——空响应是 provider 侧失败信号（非 agent 不配合），
/// 超限后升级为基础设施故障（System → 重试层自动重启），不计入 doc-complete nudges。
pub const MAX_DOC_EMPTY_REPLIES: usize = 3;

/// 审查空回复 / 进程异常的重试上限（2026-10-20 实测：doc-review 静默死亡）。
pub const MAX_DOC_REVIEW_RETRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocRoute {
    Phase0,
    Phase1,
    Phase2,
    Phase3,
    Finish,
}

impl DocRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            DocRoute::Phase0 => "phase0",
            DocRoute::Phase1 => "phase1",
            DocRoute::Phase2 => "phase2",
            DocRoute::Phase3 => "phase3",
            DocRoute::Finish => "finish",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "phase0" => Some(Self::Phase0),
            "phase1" => Some(Self::Phase1),
            "phase2" => Some(Self::Phase2),
            "phase3" => Some(Self::Phase3),
            "finish" => Some(Self::Finish),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocReviewReport {
    pub verdict: DocVerdict,
    /// 打回时要回哪一阶段；PASS 时只关心是否为 Finish。
    pub route: DocRoute,
    pub issues: Vec<String>,
    /// 协议块缺失/无法解析（编排层据此重跑审查，不回注 agent）
    pub protocol_violation: bool,
}

/// 解析 `<doc-complete>unit-xxx</doc-complete>`；取不到或为空 → None。
/// 引擎审计修复 #8：尾部协议位——只认「其后直到文本尾都是空白」的最后一次出现，
/// 正文引用的示例块（其后还有正文）不再能盖过尾部真标记（change SOP 同款纪律）。
pub fn parse_doc_complete(text: &str) -> Option<String> {
    let tag = "doc-complete";
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut window = text;
    while let Some(pos) = window.rfind(&open) {
        let start = pos + open.len();
        if let Some(rel) = window[start..].find(&close) {
            let end = start + rel;
            if text[end + close.len()..].trim().is_empty() {
                let unit = window[start..end].trim().to_string();
                if !unit.is_empty() {
                    return Some(unit);
                }
            }
        }
        // 该出现不满足尾部协议位（或为空标记）：排除，往前找
        window = &window[..pos];
    }
    None
}

/// 解析审查回复的三块协议；违例不报错（交给编排层重跑），只在报告里标记。
pub fn parse_doc_review(text: &str) -> Result<DocReviewReport> {
    let verdict = extract_tag(text, "doc-verdict").and_then(|v| match v.trim().to_ascii_uppercase().as_str() {
        "PASS" => Some(DocVerdict::Pass),
        "FAIL" => Some(DocVerdict::Fail),
        _ => None,
    });
    let Some(verdict) = verdict else {
        return Ok(DocReviewReport {
            verdict: DocVerdict::Fail,
            route: DocRoute::Phase3,
            issues: vec!["审查输出缺 <doc-verdict> 协议块".into()],
            protocol_violation: true,
        });
    };

    let route_raw = extract_tag(text, "route");
    let route = route_raw
        .as_deref()
        .and_then(DocRoute::parse)
        // 保守：缺失/非法 → phase3（PASS 也会被编排层当成"继续下一单元"）
        .unwrap_or(DocRoute::Phase3);

    let issues = extract_tag(text, "issues")
        .map(|block| {
            block
                .lines()
                .map(|l| l.trim().trim_start_matches(['-', '*', ' ']).to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 语义自洽：FAIL 必带 issues；PASS 不应带 issues 就不算违例（宽容）
    if verdict == DocVerdict::Fail && issues.is_empty() {
        return Ok(DocReviewReport {
            verdict,
            route,
            issues: vec!["审查判定 FAIL 但未列任何问题".into()],
            protocol_violation: true,
        });
    }

    Ok(DocReviewReport {
        verdict,
        route,
        issues,
        protocol_violation: false,
    })
}

/// 引擎审计修复 #8：前置协议位——取第一次出现（DR-13 裁决块在第一行，
/// 正文引用的示例块在其后，不再能盖过真裁决）。
fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(text[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unit_marker() {
        let text = "写完一单元。\n<doc-complete>unit-core-auth-07</doc-complete>\n";
        assert_eq!(parse_doc_complete(text).as_deref(), Some("unit-core-auth-07"));
    }

    #[test]
    fn empty_unit_marker_is_none() {
        assert!(parse_doc_complete("<doc-complete>  </doc-complete>").is_none());
        assert!(parse_doc_complete("没有标记").is_none());
    }

    #[test]
    fn parses_pass_with_finish() {
        let text = "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>\n";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.verdict, DocVerdict::Pass);
        assert_eq!(r.route, DocRoute::Finish);
        assert!(!r.protocol_violation);
    }

    #[test]
    fn parses_fail_with_route_and_issues() {
        let text = "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错：doc 说 X，代码是 Y\n- 缺 coverage\n</issues>\n";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.verdict, DocVerdict::Fail);
        assert_eq!(r.route, DocRoute::Phase1);
        assert_eq!(r.issues.len(), 2);
        assert!(r.issues[0].contains("事实错"));
        assert!(!r.protocol_violation);
    }

    #[test]
    fn missing_route_conservatively_routes_to_phase3() {
        let text = "<doc-verdict>FAIL</doc-verdict>\n<issues>\n- 缺必填节\n</issues>\n";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.route, DocRoute::Phase3);
        assert!(!r.protocol_violation);
    }

    #[test]
    fn illegal_route_conservatively_routes_to_phase3() {
        let text = "<doc-verdict>FAIL</doc-verdict>\n<route>phase9</route>\n<issues>\n- x\n</issues>\n";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.route, DocRoute::Phase3);
    }

    #[test]
    fn missing_verdict_is_protocol_violation() {
        let r = parse_doc_review("我审完了，看起来还行。").unwrap();
        assert!(r.protocol_violation);
    }

    #[test]
    fn fail_without_issues_is_protocol_violation() {
        let r = parse_doc_review("<doc-verdict>FAIL</doc-verdict>\n<route>phase3</route>").unwrap();
        assert!(r.protocol_violation);
    }

    #[test]
    fn first_block_wins() {
        // 引擎审计修复 #8：协议位收口后取第一次出现——重复块以真裁决（第一行）为准
        let text = "<doc-verdict>FAIL</doc-verdict>\n<doc-verdict>PASS</doc-verdict>\n<route>finish</route>";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.verdict, DocVerdict::Fail);
    }

    /// 引擎审计修复 #8 回归：正文引用的示例块不得盖过第一行真裁决。
    #[test]
    fn quoted_example_cannot_override_head_verdict() {
        let text = "<doc-verdict>PASS</doc-verdict>\n\n说明：若不合格应输出 <doc-verdict>FAIL</doc-verdict>。";
        let r = parse_doc_review(text).unwrap();
        assert_eq!(r.verdict, DocVerdict::Pass, "正文引文不得改写裁决");
    }

    /// 引擎审计修复 #8 回归：doc-complete 尾部协议位——正文里的示例标记
    /// （其后还有正文）不收，只收尾部真标记。
    #[test]
    fn complete_marker_ignores_quoted_examples() {
        let text = "先看示例：<doc-complete>unit-x</doc-complete>（这是格式说明）\n正文写完了\n<doc-complete>unit-a-01</doc-complete>";
        assert_eq!(parse_doc_complete(text).as_deref(), Some("unit-a-01"));
    }
}
