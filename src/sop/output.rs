//! 统一结构化输出协议（P001 open-questions Q002/Q004）。
//!
//! 两个 Agent 的结构化信号共用一套机制：
//! - Plan Agent 完成标记：文本含 `<plan-complete>`
//! - Review verdict 块：`<verdict>PASS|FAIL</verdict>` + FAIL 时 `<issues>` 清单

use crate::error::Result;

/// Plan Agent 完成标记（用户确认完成后输出）。
pub const COMPLETION_TAG: &str = "<plan-complete>";

/// Review 文本是否携带完成标记。
pub fn is_complete(text: &str) -> bool {
    text.contains(COMPLETION_TAG)
}

/// Review 结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// 判负：审查官给了结论，`issues` 逐条原文（回注被审 agent）。
    Fail { issues: Vec<String> },
    /// 协议违例：审查输出**本身**不合规（缺块 / 未知值 / 自相矛盾 / FAIL 不带 issues）
    /// —— 不是对 plan tree 的判定，**不得**当 issue 回注被审 agent（decisions D026）。
    ProtocolViolation { reason: String },
}

impl Verdict {
    /// 协议违例原因（判负通道 → `None`）。
    pub fn violation_reason(&self) -> Option<String> {
        match self {
            Verdict::ProtocolViolation { reason } => Some(reason.clone()),
            _ => None,
        }
    }
}

/// 解析 Review 文本中的 verdict 块（P8-ISSUE-10 修复：缺失/未知/未闭合不再硬错误，
/// 而是保守按 FAIL 处理——协议违例本身作为 issue 进重查循环，新实例重审）。
///
/// 合法形态：
/// ```text
/// ...自由分析...
/// <verdict>PASS</verdict>
/// ```
/// ```text
/// ...自由分析...
/// <verdict>FAIL</verdict>
/// <issues>
/// - 矛盾一
/// - 矛盾二
/// </issues>
/// ```
///
/// 协议违例（`Verdict::ProtocolViolation`）：缺块 / 未知值 / 未闭合 / **FAIL 不带 issues**
/// —— 审查输出本身不合规，**不是**对 plan tree 的判定，不得回注被审 agent（decisions D026）。
pub fn parse_verdict(text: &str) -> Result<Verdict> {
    let Some(open) = text.find("<verdict>") else {
        return Ok(conservative_fail("Review 输出缺 <verdict> 协议块"));
    };
    let after_open = &text[open + "<verdict>".len()..];
    let Some(close) = after_open.find("</verdict>") else {
        return Ok(conservative_fail("verdict 块未闭合"));
    };
    let verdict = after_open[..close].trim();

    match verdict {
        "PASS" => Ok(Verdict::Pass),
        "FAIL" => {
            let issues = extract_issues(&after_open[close + "</verdict>".len()..]);
            if issues.is_empty() {
                // 判负却没有问题清单 = 审查输出不合规（违例通道），不是空清单的判负
                return Ok(conservative_fail(
                    "verdict=FAIL 但缺 <issues> 清单（判负必须逐条列问题）",
                ));
            }
            Ok(Verdict::Fail { issues })
        }
        other => Ok(conservative_fail(&format!(
            "未知 verdict `{other}`（只认 PASS/FAIL），保守按 FAIL 重查"
        ))),
    }
}

/// 保守 FAIL：**协议违例**（审查输出本身不合规）—— 换全新实例重审，不回注被审 agent。
fn conservative_fail(reason: &str) -> Verdict {
    Verdict::ProtocolViolation {
        reason: format!(
            "{reason}。请原样重新提交 verdict（此为审查输出协议问题，非 plan tree 问题）"
        ),
    }
}

/// 提取 `<issues>` 块内的清单行（去掉 `- ` 前缀）。无块 → 空。
fn extract_issues(text: &str) -> Vec<String> {
    let Some(start) = text.find("<issues>") else {
        return Vec::new();
    };
    let body = &text[start + "<issues>".len()..];
    let Some(end) = body.find("</issues>") else {
        return Vec::new();
    };
    body[..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.strip_prefix("- ").unwrap_or(l).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_tag_detection() {
        assert!(is_complete("搞定了。<plan-complete>"));
        assert!(is_complete("<plan-complete>"));
        assert!(!is_complete("还在写"));
        assert!(!is_complete(""));
    }

    #[test]
    fn verdict_pass() {
        let v = parse_verdict("整体通顺，无矛盾。\n<verdict>PASS</verdict>\n").unwrap();
        assert_eq!(v, Verdict::Pass);
    }

    #[test]
    fn verdict_fail_with_issues() {
        let text = "发现前后矛盾。\n<verdict>FAIL</verdict>\n<issues>\n- 规划说 Rust 又说 Python\n- 阶段顺序颠倒\n</issues>\n";
        let v = parse_verdict(text).unwrap();
        assert_eq!(
            v,
            Verdict::Fail {
                issues: vec!["规划说 Rust 又说 Python".into(), "阶段顺序颠倒".into()]
            }
        );
    }

    #[test]
    fn fail_without_issues_block_is_a_protocol_violation() {
        // 错实现形态：把 FAIL 空箱当成「判负但没列问题」（空清单）→ 本用例必须变红
        let v = parse_verdict("<verdict>FAIL</verdict>").unwrap();
        assert!(
            matches!(v, Verdict::ProtocolViolation { .. }),
            "FAIL 不带 issues 是协议违例，不是空清单的判负: {v:?}"
        );
    }

    #[test]
    fn missing_or_unknown_verdict_is_a_protocol_violation() {
        // P8-ISSUE-10 修复 + D026：协议违例不再硬错误，也不再当 issue 回注被审 agent
        for text in ["没有任何块", "<verdict>MAYBE</verdict>", "<verdict>PASS</verdict"] {
            let v = parse_verdict(text).expect("不再 Err");
            let Verdict::ProtocolViolation { reason } = v else {
                panic!("应判协议违例（不是判负）: {text}");
            };
            assert!(!reason.is_empty(), "违例原因要能进错误文本: {text}");
        }
    }
}
