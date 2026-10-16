//! 统一结构化输出协议（P001 open-questions Q002/Q004）。
//!
//! 两个 Agent 的结构化信号共用一套机制：
//! - Plan Agent 完成标记：文本含 `<plan-complete>`
//! - Review verdict 块：`<verdict>PASS|FAIL</verdict>` + FAIL 时 `<issues>` 清单

use crate::error::{DevforgeError, Result};

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
    Fail { issues: Vec<String> },
}

/// 解析 Review 文本中的 verdict 块。
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
pub fn parse_verdict(text: &str) -> Result<Verdict> {
    let open = text
        .find("<verdict>")
        .ok_or_else(|| DevforgeError::RpcProtocol("Review 输出缺 <verdict> 块".into()))?;
    let after_open = &text[open + "<verdict>".len()..];
    let close = after_open
        .find("</verdict>")
        .ok_or_else(|| DevforgeError::RpcProtocol("verdict 块未闭合".into()))?;
    let verdict = after_open[..close].trim();

    match verdict {
        "PASS" => Ok(Verdict::Pass),
        "FAIL" => {
            let issues = extract_issues(&after_open[close + "</verdict>".len()..]);
            Ok(Verdict::Fail { issues })
        }
        other => Err(DevforgeError::RpcProtocol(format!(
            "未知 verdict `{other}`（只认 PASS/FAIL）"
        ))),
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
    fn fail_without_issues_block_is_empty_list() {
        let v = parse_verdict("<verdict>FAIL</verdict>").unwrap();
        assert_eq!(v, Verdict::Fail { issues: vec![] });
    }

    #[test]
    fn missing_or_unknown_verdict_is_error() {
        assert!(parse_verdict("没有任何块").is_err());
        assert!(parse_verdict("<verdict>MAYBE</verdict>").is_err());
        assert!(parse_verdict("<verdict>PASS</verdict").is_err()); // 未闭合
    }
}
