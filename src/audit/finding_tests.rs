//! （2026-10-26 测试分离：自 src/audit/finding.rs 内联平移，纯移动零改动。）
use super::*;

const OK_REPLY: &str = r#"<audit-findings>
[{"path":"src/x.rs","line":42,"view":"correctness","severity":"major","claim":"边界遗漏","evidence":"src/x.rs:42 在 None 时直接解包"}]
</audit-findings>
批次总评：……"#;

#[test]
fn parses_well_formed_findings() {
    let f = parse_findings(OK_REPLY).unwrap();
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].path, "src/x.rs");
    assert_eq!(f[0].line, 42);
    assert_eq!(f[0].view, View::Correctness);
    assert_eq!(f[0].severity, Severity::Major);
}

#[test]
fn missing_block_is_protocol_violation() {
    assert!(parse_findings("没有协议块的回复").is_err());
}

#[test]
fn illegal_view_is_protocol_violation() {
    let bad = r#"<audit-findings>
[{"path":"a","line":1,"view":"幻想视角","severity":"major","claim":"x","evidence":"y"}]
</audit-findings>"#;
    assert!(parse_findings(bad).unwrap_err().contains("view 非法"));
}

/// 协议层与核验层的职责边界：空 evidence **可解析**（结构合法），但核验层会丢弃；
/// 丢弃行为的回归在 `verify::tests::empty_or_referenceless_evidence_is_dropped`。
/// 第三方 r2 m6：旧版本叫这个名字却拿带证据的 OK_REPLY 断言 `!evidence.is_empty()`——
/// 零证据样本从未构造，删掉被测行为测试仍绿（空测试）。
#[test]
fn zero_evidence_parses_at_protocol_layer() {
    let reply = r#"<audit-findings>
[{"path":"a.rs","line":1,"view":"correctness","severity":"minor","claim":"x","evidence":""}]
</audit-findings>"#;
    let f = parse_findings(reply).expect("协议层不因空 evidence 拒收（结构合法）");
    assert_eq!(f.len(), 1, "零证据样本必须真能解析出一条");
    assert!(f[0].evidence.is_empty(), "样本本身必须真的是零证据");
}

/// 全仓审计 m6：行号超大值不得 `as u32` 静默回绕成合法小行号。
#[test]
fn oversized_line_is_protocol_violation() {
    let bad = r#"<audit-findings>
[{"path":"a.rs","line":4294967296,"view":"correctness","severity":"minor","claim":"c","evidence":"a.rs:1"}]
</audit-findings>"#;
    let err = parse_findings(bad).unwrap_err();
    assert!(err.contains("line 越界"), "{err}");
}

/// 全仓审计 m14：单批发现数上限——超出截断，不得无上限刷爆报告。
#[test]
fn findings_are_capped_per_batch() {
    let items: Vec<String> = (0..(MAX_FINDINGS_PER_BATCH + 5))
        .map(|i| {
            format!(
                r#"{{"path":"a.rs","line":1,"view":"correctness","severity":"nit","claim":"c{i}","evidence":"a.rs:1"}}"#
            )
        })
        .collect();
    let reply = format!("<audit-findings>[{}]</audit-findings>", items.join(","));
    let out = parse_findings(&reply).unwrap();
    assert_eq!(out.len(), MAX_FINDINGS_PER_BATCH, "超限应截断到上限");
}
