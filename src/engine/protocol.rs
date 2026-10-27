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

impl ResponseRecord {
    /// 上游错误文本的**可入日志形**（自审 minor 139）：`error` 字段原样承载上游正文，
    /// 上游错误体里回显的凭据片段会一字不差进日志。日志/事件面一律走本方法（限长 +
    /// 刷掉 ANSI/控制字符），需要原文的调用方取 `error` 字段自己负责。
    pub fn safe_error(&self) -> Option<String> {
        self.error.as_deref().map(crate::error::sanitize_untrusted)
    }
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
        self.assistant_event()?.get("delta")?.as_str()
    }

    /// 权威终结文本（自审 minor 140）：pi 协议里 `text_end.content` /
    /// `message_end.message` 才是**权威**完整文本。旧口径只拼 delta，漏事件或分块
    /// （contentIndex）错位时，累积文本会与真实回复静默不一致。
    /// 返回（文本, 是否是权威全文）。
    pub fn authoritative_text(&self) -> Option<&str> {
        if self.r#type != "message_update" {
            return None;
        }
        let ev = self.assistant_event()?;
        match ev.get("type")?.as_str()? {
            "text_end" => ev.get("content")?.as_str(),
            "message_end" => ev
                .get("message")?
                .get("content")
                .and_then(|c| c.as_str()),
            _ => None,
        }
    }

    /// 消息块序号（contentIndex）：多块消息时用于按块归类，旧口径完全忽略。
    pub fn content_index(&self) -> Option<u64> {
        self.assistant_event()?.get("contentIndex")?.as_u64()
    }

    fn assistant_event(&self) -> Option<&serde_json::Value> {
        self.extra.get("assistantMessageEvent")
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
    // 自审 major：解码失败归 System（对端失能口径，与 client.rs 的 EOF 归一
    // 同一条先例：记录损坏/流异常是基础设施故障，可重启）——旧口径 Json/RpcProtocol
    // 各奔东西，都不进重试策略，run 一次即判死。
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
        DevforgeError::System(format!(
            "解析 RPC 记录失败（对端失能）: {} · {e}",
            summarize_line(line)
        ))
    })?;
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

/// 编码一行 JSONL（自审 minor 36：旧口径的 `encode_line` 包装已删——它是
/// `CommandRecord::encode` 的重复公开面，全仓零调用方）。

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
