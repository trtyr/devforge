//! （2026-10-26 测试分离：自 src/sop/testing_protocol.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn skeleton_creates_all_dirs_and_is_idempotent() {
    let board = tempfile::tempdir().unwrap();
    create_testing_skeleton(board.path()).unwrap();
    for dir in TESTING_SKELETON_DIRS {
        assert!(board.path().join(dir).is_dir(), "{dir} 应存在");
    }
    // 幂等：重复创建不报错
    create_testing_skeleton(board.path()).unwrap();
}

#[test]
fn finding_record_parses_full_form() {
    let text = "\n# Finding F-001\n\n- 文档出处：coding/docs/02-api.md §3.2\n\
- 文档说：空标题创建返回 400\n\
- 实测：系统返回 500\n\
- 复现步骤：POST /todos {\"title\":\"\"}\n\
- 严重度：high\n\
- 状态：open\n";
    let rec = parse_finding_record(text);
    assert_eq!(rec.doc_source, "coding/docs/02-api.md §3.2");
    assert_eq!(rec.doc_says, "空标题创建返回 400");
    assert_eq!(rec.observed, "系统返回 500");
    assert_eq!(rec.repro, "POST /todos {\"title\":\"\"}");
    assert_eq!(rec.severity, "high");
    assert_eq!(rec.status, FindingStatus::Open);
    assert!(rec.disposition.is_empty());
}

#[test]
fn finding_status_is_conservative() {
    assert_eq!(FindingStatus::parse("open"), FindingStatus::Open);
    assert_eq!(FindingStatus::parse("triaged"), FindingStatus::Triaged);
    assert_eq!(FindingStatus::parse("fixed"), FindingStatus::Fixed);
    assert_eq!(FindingStatus::parse("verified"), FindingStatus::Verified);
    // 未知/空/胡写 → Open（保守：未明确标记的一律按未处置）
    assert_eq!(FindingStatus::parse(""), FindingStatus::Open);
    assert_eq!(FindingStatus::parse("done??"), FindingStatus::Open);
    assert_eq!(FindingStatus::parse("closed"), FindingStatus::Open);
}

#[test]
fn finding_record_missing_fields_default_empty_and_open() {
    let rec = parse_finding_record("# 空 finding\n\n随便写点没有小节格式的东西\n");
    assert_eq!(rec, FindingRecord::default());
    assert_eq!(rec.status, FindingStatus::Open);
}

#[test]
fn finding_record_disposition_and_verified_status() {
    let text = "- 文档出处：coding/docs/01.md §1\n- 状态：verified\n- 处置摘要：已补 02-api.md §3.2 错误码表\n";
    let rec = parse_finding_record(text);
    assert_eq!(rec.status, FindingStatus::Verified);
    assert_eq!(rec.disposition, "已补 02-api.md §3.2 错误码表");
}

// ===== Test Review 解析（T003：保守路径全覆盖） =====

#[test]
fn test_review_fail_with_doc_side_reroute() {
    let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
                <reroute>doc-side</reroute>\n\
                <issues>- 缺分页查询场景\n- 错误码覆盖不足</issues>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.verdict, StageVerdict::Fail);
    assert_eq!(r.next, NextAction::Fix);
    assert_eq!(r.reroute, Reroute::DocSide);
    assert_eq!(r.issues.len(), 2);
    assert!(r.issues[0].contains("分页"));
}

#[test]
fn test_review_fail_missing_reroute_defaults_test_side() {
    let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>- 断言与文档矛盾</issues>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.reroute, Reroute::TestSide, "缺失 → 保守 test-side（D003）");
}

#[test]
fn test_review_fail_invalid_reroute_defaults_test_side() {
    let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
                <reroute>user-side</reroute>\n<issues>- 场景不合理</issues>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.reroute, Reroute::TestSide, "未知值 → 保守 test-side（不滥升级）");
}

#[test]
fn test_review_pass_advance_with_notes_reroute_placeholder() {
    let text = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>\n\
                <reroute>doc-side</reroute>\n<notes>- 建议补超时用例</notes>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.verdict, StageVerdict::Pass);
    assert_eq!(r.next, NextAction::Advance);
    assert_eq!(r.reroute, Reroute::TestSide, "PASS 的 reroute 无意义，恒归位");
    assert_eq!(r.notes, vec!["建议补超时用例"]);
}

#[test]
fn test_review_pass_finish() {
    let text = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.next, NextAction::Finish);
    assert!(r.issues.is_empty() && r.notes.is_empty());
}

#[test]
fn test_review_conservative_missing_verdict() {
    let r = parse_test_review_report("没有协议块的回复");
    assert_eq!(r.verdict, StageVerdict::Fail);
    assert_eq!(r.next, NextAction::Fix);
    assert_eq!(r.reroute, Reroute::TestSide);
    assert!(r.issues[0].contains("缺 <stage-verdict>"), "违例原因作为 issue");
}

#[test]
fn test_review_conservative_unknown_verdict_and_next() {
    let r = parse_test_review_report("<stage-verdict>OK</stage-verdict>\n<next>fix</next>\n");
    assert_eq!(r.verdict, StageVerdict::Fail);
    assert!(r.issues[0].contains("未知 stage-verdict"));
    let r = parse_test_review_report("<stage-verdict>FAIL</stage-verdict>\n<next>skip</next>\n");
    assert_eq!(r.next, NextAction::Fix);
    assert!(r.issues[0].contains("未知 next"));
}

#[test]
fn test_review_conservative_pass_missing_next() {
    let r = parse_test_review_report("<stage-verdict>PASS</stage-verdict>\n");
    assert_eq!(r.verdict, StageVerdict::Fail, "PASS 缺去向 → 保守 FAIL");
    assert!(r.issues[0].contains("缺 <next>"));
}

#[test]
fn test_review_conservative_fail_missing_next_defaults_fix() {
    let text = "<stage-verdict>FAIL</stage-verdict>\n<reroute>doc-side</reroute>\n<issues>- 文档薄</issues>\n";
    let r = parse_test_review_report(text);
    assert_eq!(r.next, NextAction::Fix, "FAIL 缺 next 缺省 Fix");
    assert_eq!(r.reroute, Reroute::DocSide);
}

#[test]
fn test_review_conservative_contradictions() {
    let r = parse_test_review_report("<stage-verdict>PASS</stage-verdict>\n<next>fix</next>\n");
    assert_eq!(r.verdict, StageVerdict::Fail);
    assert!(r.issues[0].contains("协议矛盾"));
    let r = parse_test_review_report("<stage-verdict>FAIL</stage-verdict>\n<next>finish</next>\n");
    assert_eq!(r.verdict, StageVerdict::Fail);
    assert!(r.issues[0].contains("协议矛盾"));
}

#[test]
fn stage_complete_reuse_across_sops() {
    // testing 与 coding 同一标记家族：单一实现（re-export 不复制逻辑）
    assert_eq!(parse_stage_complete("<stage-complete>3</stage-complete>"), Some(3));
    assert_eq!(parse_stage_complete("没有标记"), None);
}

// ===== Doc Review 解析（D004） =====

#[test]
fn doc_review_pass_fail_and_conservative() {
    let ok = parse_doc_review_report("<stage-verdict>PASS</stage-verdict>\n<notes>- 建议 Runbook 加端口说明</notes>");
    assert_eq!(ok.verdict, StageVerdict::Pass);
    assert_eq!(ok.notes.len(), 1);

    let bad = parse_doc_review_report("<stage-verdict>FAIL</stage-verdict>\n<issues>- 缺种子数据节</issues>");
    assert_eq!(bad.verdict, StageVerdict::Fail);
    assert_eq!(bad.issues, vec!["缺种子数据节"]);

    let missing = parse_doc_review_report("啥都没有");
    assert_eq!(missing.verdict, StageVerdict::Fail, "协议块缺失 → 保守 FAIL");

    let junk = parse_doc_review_report("<stage-verdict>MAYBE</stage-verdict>");
    assert_eq!(junk.verdict, StageVerdict::Fail, "未知值 → 保守 FAIL");
}

#[test]
fn reroute_parse_direct() {
    assert_eq!(Reroute::parse(Some("doc-side")), Reroute::DocSide);
    assert_eq!(Reroute::parse(Some("test-side")), Reroute::TestSide);
    assert_eq!(Reroute::parse(Some("  doc-side ")), Reroute::DocSide);
    assert_eq!(Reroute::parse(None), Reroute::TestSide);
    assert_eq!(Reroute::parse(Some("junk")), Reroute::TestSide);
}
