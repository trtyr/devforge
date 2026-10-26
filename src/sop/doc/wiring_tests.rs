//! （2026-10-26 测试分离：自 src/sop/doc/wiring.rs 内联平移，纯移动零改动。）
use super::*;

/// 修复 #4 回归：findings 非空 = 不开工（Quality），不再只写证据文件就放行。
#[test]
fn gate0_findings_block_the_run() {
    let finding = crate::docgen::inventory::ClosureFinding {
        closure: crate::docgen::inventory::Closure::FileCoverage,
        path: "src/stray.py".into(),
        line: 1,
        message: "磁盘上的源文件不在清单里".into(),
    };
    let err = gate0_block(&[finding], std::path::Path::new("/w/docs/contexts/_meta"))
        .unwrap_err();
    assert!(err.is_quality_failure(), "闭包缺失是规格级不开工：{err}");
    assert!(format!("{err}").contains("gate0-closures.md"), "{err}");
    assert!(format!("{err}").contains("1 项"), "{err}");
}

#[test]
fn gate0_clean_findings_pass() {
    gate0_block(&[], std::path::Path::new("/w/_meta")).unwrap();
}

/// 修复 #6 回归：终审协议违例换实例重试——违例一次后 PASS，open 两次内成功。
#[test]
fn final_review_retries_protocol_violation() {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    let mut factory = crate::sop::doc::mocks::PipeReviewFactory {
        replies: Arc::new(Mutex::new(VecDeque::from(vec![
            "我看了看，挺好的".to_string(), // 无协议块 → violation
            "确实行\n<doc-verdict>PASS</doc-verdict>".to_string(),
        ]))),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: crate::sop::doc::mocks::no_gates(),
    };
    let v = super::final_review_with_retry(&mut factory, "审").unwrap();
    assert!(!v.protocol_violation);
    assert_eq!(
        factory.instructions.lock().unwrap().len(),
        2,
        "违例后应换实例重试一次"
    );
}

/// 修复 #6 回归：连续违例耗尽上限（3）→ 返回最后一次违例 verdict，
/// 不再无限重试（调用方按协议违例失败，账单留证）。
#[test]
fn final_review_exhausts_violations_returns_last() {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    let mut factory = crate::sop::doc::mocks::PipeReviewFactory {
        replies: Arc::new(Mutex::new(VecDeque::from(vec![
            "没块".to_string(),
            "还是没块".to_string(),
            "依旧没块".to_string(),
            "不该被取到".to_string(),
        ]))),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: crate::sop::doc::mocks::no_gates(),
    };
    let v = super::final_review_with_retry(&mut factory, "审").unwrap();
    assert!(v.protocol_violation, "耗尽应返回违例 verdict");
    assert_eq!(
        factory.instructions.lock().unwrap().len(),
        3,
        "上限 3 次后不得继续重试"
    );
}
