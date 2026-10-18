//! Coding SOP 结构化输出协议（P005 · 第二个内置 SOP）。
//!
//! 与 planning 的 verdict 家族同源（保守缺省哲学，P8-ISSUE-10 家族）：
//! - Coding Agent 阶段完成标记：`<stage-complete>N</stage-complete>`（无标记 → 编排器回注重推，零上限）
//! - Review Report 块：`<stage-verdict>PASS|FAIL</stage-verdict>` + `<next>fix|advance|finish</next>`
//!   + FAIL 时 `<issues>` 清单 + PASS 时可附 `<notes>` 非阻塞建议
//!
//! 黑板布局常量 = topics/shared-blackboard.md 的代码化（单一事实源在文档，这里只固化约定）。

use crate::error::Result;

// ===== 黑板布局常量 =====

/// coding 阶段过程资产子树根（任务目录下）。
pub const CODING_DIR: &str = "coding";
/// 阶段划分与状态（Coding Agent 写，Review 审计）。
pub const STAGES_FILE: &str = "coding/stages.md";
/// 当前阶段 todo list（随阶段滚动）。
pub const TODO_FILE: &str = "coding/todo.md";
/// 伴写文档目录（未来测试 Agent 的输入）。
pub const DOCS_DIR: &str = "coding/docs";
/// Review Report 目录（review-N.md）。
pub const REPORTS_DIR: &str = "coding/reports";
/// plan 偏差留痕目录（decisions/005）。
pub const BLOCKERS_DIR: &str = "coding/blockers";

// ===== 协议标签 =====

/// 阶段完成标记标签名。
pub const STAGE_COMPLETE_TAG: &str = "stage-complete";
const STAGE_VERDICT_TAG: &str = "stage-verdict";
const NEXT_TAG: &str = "next";
const NOTES_TAG: &str = "notes";

/// 阶段完成标记解析：文本中**最后一个** `<stage-complete>N</stage-complete>`。
/// 无标记/未闭合/非数字 → `None`（编排器语义：无有效标记 → 回注重推，零上限）。
pub fn parse_stage_complete(text: &str) -> Option<u64> {
    let open = format!("<{STAGE_COMPLETE_TAG}>");
    let close = format!("</{STAGE_COMPLETE_TAG}>");
    let mut last: Option<u64> = None;
    let mut rest = text;
    while let Some(pos) = rest.find(&open) {
        let after = &rest[pos + open.len()..];
        let Some(end) = after.find(&close) else {
            break;
        };
        if let Ok(n) = after[..end].trim().parse::<u64>() {
            last = Some(n);
        }
        rest = &after[end + close.len()..];
    }
    last
}

/// Review 报告裁决。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageVerdict {
    Pass,
    Fail,
}

/// 下一动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextAction {
    /// 修复后重审（同阶段）
    Fix,
    /// 进入阶段 N+1
    Advance,
    /// 树已全完成，收工
    Finish,
}

/// 解析后的 Review Report。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewReport {
    pub verdict: StageVerdict,
    pub next: NextAction,
    /// FAIL 时的问题清单（回注 Coding Agent 修复）
    pub issues: Vec<String>,
    /// PASS 时可附的非阻塞建议（advance 前先消化）
    pub notes: Vec<String>,
    /// 协议违例标记（C19-R1）：审查输出本身不合规（缺协议块/未知值/自相矛盾），
    /// **不是**对代码的判定。编排层据此重跑审查，绝不回注给被审 Agent。
    pub protocol_violation: bool,
}

/// 解析 Review 文本为结构化报告。
///
/// 保守缺省（与 planning parse_verdict 同哲学）：
/// - `<stage-verdict>` 缺失/未闭合/未知值 → 保守 FAIL + 违例原因作为 issue
/// - PASS 但 `<next>` 缺失/未知 → 保守 FAIL（PASS 必须携带去向）
/// - FAIL 但 `<next>` 缺失 → 缺省 Fix（语义安全：FAIL 自然是修）
/// - 语义矛盾（PASS+fix、FAIL+advance/finish）→ 保守 FAIL + 违例原因
pub fn parse_review_report(text: &str) -> Result<ReviewReport> {
    let Some(verdict_raw) = extract_block(text, STAGE_VERDICT_TAG) else {
        return Ok(conservative("Review 输出缺 <stage-verdict> 协议块"));
    };
    let verdict = match verdict_raw {
        "PASS" => StageVerdict::Pass,
        "FAIL" => StageVerdict::Fail,
        other => {
            return Ok(conservative(&format!(
                "未知 stage-verdict `{other}`（只认 PASS/FAIL），保守按 FAIL 重审"
            )));
        }
    };

    let Some(next_raw) = extract_block(text, NEXT_TAG) else {
        // FAIL 缺 next 缺省 Fix；PASS 缺 next 是协议违例（PASS 必须携带去向）
        return match verdict {
            StageVerdict::Fail => Ok(ReviewReport {
                verdict,
                next: NextAction::Fix,
                issues: extract_lines(text, "issues"),
                notes: Vec::new(),
                protocol_violation: false,
            }),
            StageVerdict::Pass => Ok(conservative(
                "stage-verdict=PASS 但缺 <next> 协议块（PASS 必须携带 fix|advance|finish 去向）",
            )),
        };
    };
    let next = match next_raw {
        "fix" => NextAction::Fix,
        "advance" => NextAction::Advance,
        "finish" => NextAction::Finish,
        other => {
            return Ok(conservative(&format!(
                "未知 next `{other}`（只认 fix/advance/finish），保守按 FAIL 重审"
            )));
        }
    };

    // 语义一致性：PASS+fix、FAIL+advance/finish 都是自我矛盾
    let contradictory = matches!(
        (verdict.clone(), next),
        (StageVerdict::Pass, NextAction::Fix)
            | (StageVerdict::Fail, NextAction::Advance)
            | (StageVerdict::Fail, NextAction::Finish)
    );
    if contradictory {
        return Ok(conservative(&format!(
            "协议矛盾：stage-verdict={verdict_raw} 与 next={next_raw} 不可能同时成立，保守按 FAIL 重审"
        )));
    }

    Ok(ReviewReport {
        issues: extract_lines(text, "issues"),
        notes: extract_lines(text, NOTES_TAG),
        verdict,
        next,
        protocol_violation: false,
    })
}

/// 保守 FAIL 报告（协议违例）：原因是审查输出本身不合规，**不是**代码问题；
/// 标记 `protocol_violation` 让编排层重跑审查，而不是把这条回注给被审 Agent（C19-R1）。
fn conservative(reason: &str) -> ReviewReport {
    ReviewReport {
        verdict: StageVerdict::Fail,
        next: NextAction::Fix,
        issues: vec![format!(
            "{reason}。请原样重新提交完整协议块（此为审查输出协议问题，非代码问题）"
        )],
        notes: Vec::new(),
        protocol_violation: true,
    }
}

/// 提取 `<tag>...</tag>` 块内文（trim）。缺失/未闭合 → None。
pub(crate) fn extract_block<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let pos = text.find(&open)?;
    let after = &text[pos + open.len()..];
    let end = after.find(&close)?;
    Some(after[..end].trim())
}

/// 提取 `<tag>` 块内清单行（去 `- ` 前缀，跳过空行）。缺失 → 空。
pub(crate) fn extract_lines(text: &str, tag: &str) -> Vec<String> {
    extract_block(text, tag)
        .map(|body| {
            body.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| l.strip_prefix("- ").unwrap_or(l).to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 黑板布局常量与文档一致性 =====

    #[test]
    fn blackboard_layout_matches_topic_doc() {
        // topics/shared-blackboard.md 的代码化锚点：改名必须同步文档
        assert_eq!(CODING_DIR, "coding");
        assert_eq!(STAGES_FILE, "coding/stages.md");
        assert_eq!(TODO_FILE, "coding/todo.md");
        assert_eq!(DOCS_DIR, "coding/docs");
        assert_eq!(REPORTS_DIR, "coding/reports");
        assert_eq!(BLOCKERS_DIR, "coding/blockers");
    }

    // ===== 阶段完成标记 =====

    #[test]
    fn stage_complete_basic() {
        assert_eq!(
            parse_stage_complete("阶段 2 完成。\n<stage-complete>2</stage-complete>\n"),
            Some(2)
        );
        assert_eq!(parse_stage_complete("<stage-complete>1</stage-complete>"), Some(1));
    }

    #[test]
    fn stage_complete_absent_or_malformed_is_none() {
        // 无标记 → 编排器回注重推（decisions/003 零上限）
        assert_eq!(parse_stage_complete("还在写，没完成"), None);
        assert_eq!(parse_stage_complete(""), None);
        // 未闭合
        assert_eq!(parse_stage_complete("<stage-complete>3</stage-complete"), None);
        // 非数字 → 无有效标记
        assert_eq!(parse_stage_complete("<stage-complete>三</stage-complete>"), None);
    }

    #[test]
    fn stage_complete_takes_last_marker() {
        let text = "<stage-complete>1</stage-complete>\n中间又改了\n<stage-complete>2</stage-complete>";
        assert_eq!(parse_stage_complete(text), Some(2));
    }

    // ===== Review Report 正常路径 =====

    #[test]
    fn report_pass_advance_with_notes() {
        let text = "阶段合格。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>\n<notes>\n- 建议补个边界测试\n- 命名可以再收敛\n</notes>\n";
        let r = parse_review_report(text).unwrap();
        assert_eq!(
            r,
            ReviewReport {
                verdict: StageVerdict::Pass,
                next: NextAction::Advance,
                issues: vec![],
                notes: vec!["建议补个边界测试".into(), "命名可以再收敛".into()],
                protocol_violation: false,
            }
        );
    }

    #[test]
    fn report_pass_finish_minimal() {
        let r = parse_review_report("<stage-verdict>PASS</stage-verdict>\n<next>finish</next>").unwrap();
        assert_eq!(r.verdict, StageVerdict::Pass);
        assert_eq!(r.next, NextAction::Finish);
        assert!(r.issues.is_empty() && r.notes.is_empty());
    }

    #[test]
    fn report_fail_fix_with_issues() {
        let text = "有问题。\n<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 登录没处理过期 token\n- 文档漏写错误码\n</issues>\n";
        let r = parse_review_report(text).unwrap();
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert_eq!(r.next, NextAction::Fix);
        assert_eq!(r.issues, vec!["登录没处理过期 token".to_string(), "文档漏写错误码".to_string()]);
        assert!(r.notes.is_empty());
    }

    // ===== 保守缺省路径 =====

    #[test]
    fn report_missing_or_unclosed_verdict_is_conservative() {
        for text in ["纯自由文本没有协议块", "<stage-verdict>PASS</stage-verdict"] {
            let r = parse_review_report(text).unwrap();
            assert_eq!(r.verdict, StageVerdict::Fail, "缺块应保守 FAIL: {text}");
            assert_eq!(r.next, NextAction::Fix);
            assert!(!r.issues.is_empty(), "违例原因应作为 issue: {text}");
        }
    }

    #[test]
    fn report_unknown_verdict_is_conservative() {
        let r = parse_review_report("<stage-verdict>MAYBE</stage-verdict>\n<next>fix</next>").unwrap();
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues.iter().any(|i| i.contains("MAYBE")));
    }

    #[test]
    fn report_pass_missing_next_is_conservative() {
        // PASS 必须携带去向——缺 next 是协议违例
        let r = parse_review_report("<stage-verdict>PASS</stage-verdict>").unwrap();
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues.iter().any(|i| i.contains("next")));
    }

    #[test]
    fn report_fail_missing_next_defaults_fix() {
        // FAIL 缺 next 语义安全缺省：修
        let r = parse_review_report("<stage-verdict>FAIL</stage-verdict>\n<issues>\n- bug\n</issues>").unwrap();
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert_eq!(r.next, NextAction::Fix);
        assert_eq!(r.issues, vec!["bug".to_string()]);
    }

    #[test]
    fn report_unknown_next_is_conservative() {
        let r = parse_review_report("<stage-verdict>PASS</stage-verdict>\n<next>maybe</next>").unwrap();
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues.iter().any(|i| i.contains("maybe")));
    }

    #[test]
    fn report_semantic_contradictions_are_conservative() {
        for (verdict, next) in [
            ("PASS", "fix"),
            ("FAIL", "advance"),
            ("FAIL", "finish"),
        ] {
            let text = format!("<stage-verdict>{verdict}</stage-verdict>\n<next>{next}</next>");
            let r = parse_review_report(&text).unwrap();
            assert_eq!(r.verdict, StageVerdict::Fail, "{verdict}+{next} 应保守 FAIL");
            assert_eq!(r.next, NextAction::Fix);
            assert!(!r.issues.is_empty(), "矛盾原因应作为 issue: {verdict}+{next}");
        }
    }
}
