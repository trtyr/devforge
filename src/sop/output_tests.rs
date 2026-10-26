//! （2026-10-26 测试分离：自 src/sop/output.rs 内联平移，纯移动零改动。）
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
