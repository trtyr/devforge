//! （2026-10-26 测试分离：自 src/sop/coding_protocol.rs 内联平移，纯移动零改动。）
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
fn report_fail_without_issues_is_a_protocol_violation() {
    // 错实现形态：把「FAIL 但没列 issues」当成判负（protocol_violation = false）→ 本用例必须变红
    for text in [
        "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>",
        "<stage-verdict>FAIL</stage-verdict>",
    ] {
        let r = parse_review_report(text).unwrap();
        assert!(
            r.protocol_violation,
            "判负没有 problems 清单 = 审查输出不合规: {text}"
        );
        assert!(
            !r.issues.is_empty(),
            "违例原因要进 issues（供错误文本用，不是被审对象的问题）: {text}"
        );
    }
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
