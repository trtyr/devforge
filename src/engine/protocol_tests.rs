//! （2026-10-26 测试分离：自 src/engine/protocol.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn encode_prompt_exact_shape() {
    let line = CommandRecord::prompt("req-2", "Review this").encode().unwrap();
    assert_eq!(
        line,
        r#"{"id":"req-2","type":"prompt","message":"Review this"}
"#
    );
}

#[test]
fn encode_get_state_omits_message() {
    let line = CommandRecord::get_state("req-3").encode().unwrap();
    assert_eq!(line, r#"{"id":"req-3","type":"get_state"}
"#);
    assert!(!line.contains("message"), "message 应省略: {line}");
}

#[test]
fn decode_response_success_and_failure() {
    let ok = decode_line(
        r#"{"id":"req-1","type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}"#,
    )
    .unwrap();
    match ok {
        Inbound::Response(r) => {
            assert!(r.success);
            assert_eq!(r.id.as_deref(), Some("req-1"));
        }
        other => panic!("应为 response: {other:?}"),
    }

    let bad = decode_line(
        r#"{"id":"req-2","type":"response","command":"set_model","success":false,"error":"Model not found"}"#,
    )
    .unwrap();
    match bad {
        Inbound::Response(r) => {
            assert!(!r.success);
            assert_eq!(r.error.as_deref(), Some("Model not found"));
        }
        other => panic!("应为 response: {other:?}"),
    }
}

#[test]
fn decode_event_and_settled_detection() {
    let e = decode_line(r#"{"type":"agent_settled"}"#).unwrap();
    match e {
        Inbound::Event(ev) => assert!(ev.is_agent_settled()),
        other => panic!("应为 event: {other:?}"),
    }
}

#[test]
fn text_delta_extraction() {
    let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"你好"}}"#;
    match decode_line(line).unwrap() {
        Inbound::Event(ev) => assert_eq!(ev.text_delta(), Some("你好")),
        other => panic!("应为 event: {other:?}"),
    }
    // 非 delta 事件 → None
    match decode_line(r#"{"type":"agent_end"}"#).unwrap() {
        Inbound::Event(ev) => assert_eq!(ev.text_delta(), None),
        other => panic!("应为 event: {other:?}"),
    }
}

#[test]
fn invalid_json_is_an_error() {
    assert!(decode_line("not json").is_err());
    assert!(decode_line(r#"{"no_type":1}"#).is_err());
}
