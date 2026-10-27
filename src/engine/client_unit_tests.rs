//! （2026-10-26 测试分离：自 src/engine/client.rs 内联平移，纯移动零改动。）
use super::*;
use crate::engine::protocol::EventRecord;

/// 引擎 #7 回归：瞬态上游/限流错误归 System（可重启），配置类归 Config；
/// 旧实现一律 Config，一次 502 就判成「配置永不修好」。
#[test]
fn command_failure_splits_transient_from_config() {
    assert!(RpcClient::transient_failure("upstream 502 Bad Gateway"));
    assert!(RpcClient::transient_failure("HTTP 429: too many requests"));
    assert!(RpcClient::transient_failure("read ECONNRESET"));
    assert!(RpcClient::transient_failure("request timed out after 30s"));
    // 配置类（缺 key / 模型名错）不得被当成瞬态
    assert!(!RpcClient::transient_failure("missing api key"));
    assert!(!RpcClient::transient_failure("unknown model `foo`"));
    // 「500000 tokens」里的数字不算状态码
    assert!(!RpcClient::transient_failure("context window 500000 tokens"));
}

fn tool_end_event(is_error: bool, result_text: &str) -> EventRecord {
    serde_json::from_value(serde_json::json!({
        "type": "tool_execution_end",
        "toolCallId": "call_1",
        "toolName": "write",
        "isError": is_error,
        "result": {"content": [{"type": "text", "text": result_text}], "details": {}},
    }))
    .expect("事件构造合法")
}

#[test]
fn path_guard_denial_is_detected_by_prefix() {
    let e = tool_end_event(true, "[path-guard] /x/y 不在 write 允许列表内（允许：src/**）");
    assert!(RpcClient::is_path_guard_denial(&e));
}

#[test]
fn ordinary_tool_error_is_not_misreported_as_denial() {
    // 无前缀的普通工具失败不得误报（isError 但 reason 不是插件给的）
    let e = tool_end_event(true, "permission denied: /etc/passwd");
    assert!(!RpcClient::is_path_guard_denial(&e));
}

#[test]
fn non_error_tool_result_is_never_a_denial() {
    // 成功结果即使文本里出现前缀字样也不算（isError 才是门槛）
    let e = tool_end_event(false, "前缀字样出现在正常输出里 [path-guard] ");
    assert!(!RpcClient::is_path_guard_denial(&e));
}

/// 2026-10-27 自审 major 回归：分类分支的**端到端**钉子——
/// 旧口径只测 transient_failure 谓词，command_failure 里的分流分支本身够不看
/// （变异成一律 Config 后全部仍绿）。现在钉返回的错误类别 + 可重启性。
#[test]
fn command_failure_classification_pins_system_vs_config() {
    let sys = RpcClient::classify_command_failure("review", "prompt", "upstream 502 Bad Gateway");
    assert!(
        matches!(sys, DevforgeError::System(_)),
        "瞬态必须归 System（policy::is_infra_failure 只认裸 System，可重启）：{sys}"
    );
    let cfg = RpcClient::classify_command_failure("review", "prompt", "missing api key");
    assert!(
        matches!(cfg, DevforgeError::Config(_)),
        "认不出的归 Config（不盲重启）：{cfg}"
    );
    assert_ne!(
        std::mem::discriminant(&sys),
        std::mem::discriminant(&cfg),
        "两类必须可区分（瞬态≠配置）"
    );
}
