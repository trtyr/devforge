//! （2026-10-26 测试分离：自 src/error.rs 内联平移，纯移动零改动。）
use super::*;
use std::error::Error as _;

#[test]
fn display_includes_context_and_source() {
    let err: DevforgeError = std::fs::read_to_string("/nonexistent/file")
        .ctx("读取 plan tree")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("读取 plan tree"), "缺上下文: {msg}");
    assert!(msg.contains("IO"), "缺源头: {msg}");
}

#[test]
fn source_chain_walkable() {
    let err: DevforgeError = serde_json::from_str::<String>("not json")
        .ctx("解析 RPC 记录")
        .unwrap_err();
    // 链：WithContext -> Json
    let source = err.source().expect("应有 source");
    assert!(source.to_string().contains("JSON"));
}

#[test]
fn agent_failure_is_classified() {
    let e = DevforgeError::agent("plan-review", "进程异常退出");
    assert!(e.is_agent_failure());
    assert!(e.to_string().contains("plan-review"));

    let sys = DevforgeError::System("台账损坏".into());
    assert!(!sys.is_agent_failure());
    assert!(!sys.is_quality_failure());

    let q = DevforgeError::Quality("审查连续违例".into());
    assert!(q.is_quality_failure());
    assert!(!q.is_agent_failure());
}

/// 地基批次 #1（与引擎批次 #6 同根）：`.ctx()` 包装后分类仍要认得出——
/// 旧实现用 `matches!(self, ...)` 只看顶层，Quality/Agent 一包就变「非质量错误」。
#[test]
fn classification_survives_context_wrapping() {
    let q = "审查连续违例".to_string();
    let wrapped: crate::error::Result<()> = Err(DevforgeError::Quality(q));
    let e = wrapped.ctx("Plan 对话").unwrap_err();
    assert!(e.is_quality_failure(), "包装后仍应判质量失败：{e:?}");

    let wrapped: crate::error::Result<()> =
        Err(DevforgeError::agent("plan-review", "进程异常退出"));
    let a = wrapped.ctx("Plan 对话").unwrap_err();
    assert!(a.is_agent_failure(), "包装后仍应判 agent 失败：{a:?}");

    let wrapped: crate::error::Result<()> = Err(DevforgeError::System("台账损坏".into()));
    let s = wrapped.ctx("写断点").unwrap_err();
    assert!(!s.is_quality_failure());
}

// ── 2026-10-27 自审 error 批回归 ──

/// 自审 ref 147：`root_cause()` 是全部分类的承重原语，但旧用例只钉了 Agent/Quality/System
/// 三条路径——`is_infra_failure` 直接依赖的 Io/Json 根因路径没有用例，回归会是静默的。
#[test]
fn root_cause_covers_io_and_json_paths() {
    let io_err: DevforgeError = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "x").into();
    let wrapped = Result::<()>::Err(io_err).ctx("读文件").unwrap_err();
    assert!(matches!(wrapped.root_cause(), DevforgeError::Io(_)));
    assert!(wrapped.is_restartable(), "Io 根因可重启（spawn 层瞬时故障）");
    assert_eq!(wrapped.code(), "io");
    assert_eq!(wrapped.context_layers(), vec!["读文件"]);

    let json_err: DevforgeError = serde_json::from_str::<serde_json::Value>("{bad")
        .unwrap_err()
        .into();
    assert!(matches!(json_err.root_cause(), DevforgeError::Json(_)));
    assert!(!json_err.is_restartable(), "数据坏了重启不修");
    assert_eq!(json_err.code(), "json");
}

/// 自审 ref 69：谓词族必须覆盖全部变体，且都走 root_cause（包装后仍认得出）。
#[test]
fn predicates_are_root_cause_based_and_cover_every_variant() {
    let cases: Vec<(DevforgeError, &str, bool)> = vec![
        (DevforgeError::System("x".into()), "system", true),
        (DevforgeError::Quality("x".into()), "quality", false),
        (DevforgeError::Config("x".into()), "config", false),
        (DevforgeError::Store("x".into()), "store", false),
        (DevforgeError::RpcProtocol("x".into()), "rpc", false),
        (DevforgeError::agent("plan-agent", "x"), "agent", false),
    ];
    for (err, code, restartable) in cases {
        let wrapped = Result::<()>::Err(err).ctx("外层").unwrap_err();
        assert_eq!(wrapped.code(), code, "错误码：{wrapped}");
        assert_eq!(wrapped.is_restartable(), restartable, "可重启判定：{wrapped}");
        assert_eq!(wrapped.context_layers(), vec!["外层"]);
    }
    // 具体谓词与 code 自洽
    let cfg = Result::<()>::Err(DevforgeError::Config("x".into())).ctx("c").unwrap_err();
    assert!(cfg.is_config_error() && !cfg.is_quality_failure());
    let store = DevforgeError::Store("x".into());
    assert!(store.is_store_error() && !store.is_protocol_error());
}

/// 自审 ref 146：`Agent.role` 与上下文层必须能机读（旧口径只有一个不透明 context 串，
/// 角色被压进 Display 文本，事后无法按角色过滤）。
#[test]
fn agent_role_and_context_layers_are_machine_readable() {
    let err = Result::<()>::Err(DevforgeError::agent("change-coding", "模型通道错误"))
        .ctx("跑 coding")
        .ctx("change run")
        .unwrap_err();
    assert_eq!(err.agent_role(), Some("change-coding"));
    assert_eq!(err.context_layers(), vec!["change run", "跑 coding"]);
    assert!(err.is_agent_failure());
}

/// 自审 ref 143：子 Agent 的不可信文本进人类可读层前必须净化（限长 + 去控制字符/ANSI）。
#[test]
fn untrusted_text_is_sanitized() {
    let noisy = format!("\u{1b}[31m红色\u{1b}[0m 坏\u{7}消息\n第二行 {}", "x".repeat(3000));
    let clean = sanitize_untrusted(&noisy);
    assert!(!clean.contains('\u{1b}'), "ANSI 必须去掉：{clean}");
    assert!(!clean.contains('\u{7}'), "控制字符必须去掉");
    assert!(!clean.contains('\n'), "必须压成单行");
    assert!(clean.chars().count() <= 2020, "必须限长");
    assert!(clean.contains("红色") && clean.contains("坏消息"), "{clean}");

    let err = DevforgeError::agent("r", "\u{1b}[1m炸了\u{1b}[0m");
    assert_eq!(err.sanitized_message(), "agent 失败 [r]: 炸了");
}
