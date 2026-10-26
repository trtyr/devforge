//! （2026-10-26 测试分离：自 src/sop/doc/protocol.rs 内联平移，纯移动零改动。）
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
