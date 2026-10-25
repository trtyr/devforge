//! RPC JSONL 协议编解码（pi docs rpc.md）。
//!
//! 三类记录（严格 LF 分帧）：
//! - 出站命令：`{"id":"x","type":"prompt","message":"..."}`（message 可省略）
//! - 入站响应：`{"id":"x","type":"response","command":"prompt","success":true,...}`
//! - 入站事件：`{"type":"agent_settled",...}`（含 message_update 等）

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

/// 出站命令记录。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandRecord {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl CommandRecord {
    pub fn prompt(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            r#type: "prompt".into(),
            message: Some(message.into()),
        }
    }

    pub fn get_state(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            r#type: "get_state".into(),
            message: None,
        }
    }

    /// 编码为一行 JSONL（含结尾 LF）。
    pub fn encode(&self) -> Result<String> {
        let mut line = serde_json::to_string(self).ctx("序列化命令记录")?;
        line.push('\n');
        Ok(line)
    }
}

/// 入站响应记录。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ResponseRecord {
    pub id: Option<String>,
    pub r#type: String,
    pub command: String,
    pub success: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// 入站事件记录（type + 任意附加字段）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EventRecord {
    pub r#type: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EventRecord {
    /// 是否为 agent_settled（pi 不会再自动继续 = 暂停保活点，decisions/005）。
    pub fn is_agent_settled(&self) -> bool {
        self.r#type == "agent_settled"
    }

    /// 从 message_update 事件中抽取 text_delta 增量（流式文本）。
    pub fn text_delta(&self) -> Option<&str> {
        if self.r#type != "message_update" {
            return None;
        }
        self.extra
            .get("assistantMessageEvent")?
            .get("type")?
            .as_str()
            .filter(|t| *t == "text_delta")?;
        self.extra
            .get("assistantMessageEvent")?
            .get("delta")
            .and_then(|d| d.as_str())
    }
}

/// 入站记录分流。
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Response(ResponseRecord),
    Event(EventRecord),
}

/// 解码失败时的行摘要（引擎审计修复 #8）：只露 type/id 字段与长度——
/// 整行 JSONL 可能含凭据/token，不得原样进日志。
fn summarize_line(line: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => format!(
            "type={:?}, id={:?}, len={}",
            v.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
            v.get("id").and_then(|i| i.as_str()).unwrap_or("?"),
            line.len()
        ),
        Err(_) => format!("（整行不可解析，len={}）", line.len()),
    }
}

/// 解码一行 JSONL（不含 LF）。
pub fn decode_line(line: &str) -> Result<Inbound> {
    let v: serde_json::Value = serde_json::from_str(line)
        .ctx(format!("解析 RPC 记录失败: {}", summarize_line(line)))?;
    let r#type = v
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            DevforgeError::RpcProtocol(format!(
                "记录缺 type 字段: {}",
                summarize_line(line)
            ))
        })?
        .to_string();
    match r#type.as_str() {
        "response" => {
            let r: ResponseRecord = serde_json::from_value(v).ctx("解析 response 记录")?;
            Ok(Inbound::Response(r))
        }
        _ => {
            let e: EventRecord = serde_json::from_value(v).ctx("解析 event 记录")?;
            Ok(Inbound::Event(e))
        }
    }
}

/// 编码一行 JSONL。
pub fn encode_line(record: &CommandRecord) -> Result<String> {
    record.encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_prompt_exact_shape() {
        let line = encode_line(&CommandRecord::prompt("req-2", "Review this")).unwrap();
        assert_eq!(
            line,
            r#"{"id":"req-2","type":"prompt","message":"Review this"}
"#
        );
    }

    #[test]
    fn encode_get_state_omits_message() {
        let line = encode_line(&CommandRecord::get_state("req-3")).unwrap();
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
}
