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

/// T207 回归：工具调用摘要必须能回答「它到底干了什么」。
///
/// 错实现形态：①args 里有 command 却不提取 → 摘要为空/整坨 JSON；
/// ②结果里有 exitCode 却不报 → 退出码丢失；③取不到退出码就编一个 → 撒谎。
#[test]
fn tool_call_detail_extracts_the_command() {
    let args = serde_json::json!({"command": "cd /repo && cargo test --lib"});
    assert_eq!(
        tool_call_detail(Some(&args)),
        "cd /repo && cargo test --lib"
    );
}

/// T207 回归：非 bash 工具（read/write/edit）按 path/file_path 提摘要。
#[test]
fn tool_call_detail_falls_back_to_the_path_key() {
    let args = serde_json::json!({"file_path": "src/server/mod.rs"});
    assert_eq!(tool_call_detail(Some(&args)), "src/server/mod.rs");
}

/// T207 回归：没有 args 时摘要为空（不编造）。
#[test]
fn tool_call_detail_is_empty_when_args_absent() {
    assert_eq!(tool_call_detail(None), "");
}

/// T207 回归：结果摘要 + 退出码都得出。
#[test]
fn tool_result_detail_reports_exit_code_and_tail() {
    let result = serde_json::json!({
        "content": [{"type": "text", "text": "running 1085 tests\ntest result: ok. 1085 passed"}],
        "details": {"exitCode": 0}
    });
    let (detail, code) = tool_result_detail(Some(&result));
    assert_eq!(code, 0);
    assert!(detail.contains("1085 passed"), "{detail}");
}

/// T207 回归：取不到退出码必须报 -1，**不许编**。
#[test]
fn tool_result_detail_never_invents_an_exit_code() {
    let result = serde_json::json!({"content": [{"type": "text", "text": "done"}]});
    let (_, code) = tool_result_detail(Some(&result));
    assert_eq!(code, -1, "取不到退出码必须报 -1，不许编");
}

/// T207 回归：摘要必须限长且单行（不许把日志糊成 transcript）。
#[test]
fn tool_detail_is_truncated_and_single_line() {
    let long = "x".repeat(1000);
    let args = serde_json::json!({"command": long});
    let detail = tool_call_detail(Some(&args));
    assert!(
        detail.chars().count() <= TOOL_DETAIL_MAX,
        "摘要超长：{}",
        detail.chars().count()
    );
    let result = serde_json::json!({"content": [{"type": "text", "text": "a\nb\nc"}]});
    let (detail, _) = tool_result_detail(Some(&result));
    assert!(!detail.contains('\n'), "摘要必须单行：{detail}");
}

/// T207 回归：退出码字段名兼容 exitCode / exit_code 两种写法。
#[test]
fn exit_code_key_variants_are_both_recognized() {
    let snake = serde_json::json!({"details": {"exit_code": 101}});
    assert_eq!(tool_result_detail(Some(&snake)).1, 101);
    let camel = serde_json::json!({"exitCode": 2});
    assert_eq!(tool_result_detail(Some(&camel)).1, 2);
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

/// R6-P58：墙钟判定纯函数——过线 deadline 必须叫停（Quality）、未到线/不限时放行。
/// 旧口径 deadline 在 loop 内每轮现算（now+timeout 永远追不上 now）= 死代码，
/// 该用例钉住「快照一次」语义的可失败性。
#[test]
fn wall_clock_check_fires_only_when_deadline_passed() {
    let past = std::time::Instant::now() - std::time::Duration::from_millis(5);
    let err = super::wall_clock_check(Some(past)).expect("过线 deadline 必须叫停");
    assert!(err.is_quality_failure(), "墙钟叫停 = Quality：{err}");
    let future = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    assert!(super::wall_clock_check(Some(future)).is_none(), "未到线放行");
    assert!(super::wall_clock_check(None).is_none(), "显式不限时放行");
}
