//! knobs 单测（ftask-1）：三层链（CLI>file>缺省 的 file/缺省两层 + Cli 标注）、
//! config.json 读回、非法值回落、生效值带来源。
//!
//! 测试纪律：全局旋钮是进程单例——改旋钮的用例必须持 [`super::with_mut`] 并在
//! 闭包内完成断言（与 ui_warn_env_lock 同款串行纪律）。

use super::*;

/// 未 init（或全缺省）时，typed getter 返回声明表缺省。
#[test]
fn defaults_hold_when_never_initialized() {
    // 不调 init——故意走「未初始化」路径
    let k = RuntimeKnobs::default();
    assert_eq!(k.ui_pending_warn_ms, 120_000);
    assert_eq!(k.review_timeout_ms, Some(1_800_000));
    assert_eq!(k.ui_ask_timeout_ms, None, "ask 缺省不限（零硬限制）");
    assert_eq!(k.doc_level_change, "none", "change 侧缺省 none");
    assert_eq!(k.doc_level_docgen, "complete", "docgen 侧缺省 complete");
    assert_eq!(k.max_concurrent, 2);
}

/// config.json 的旋钮段覆盖缺省；非法值回落缺省并可见（不炸启动）。
#[test]
fn config_json_overrides_defaults_and_bad_values_fall_back() {
    let json = serde_json::json!({
        "ui_pending_warn_ms": 5_000,
        "max_concurrent": 1,
        "doc_level_change": "detailed",
        "stall_warn_ms": 0,
        "audit_concurrency": 999
    });
    let (knobs, sources) = RuntimeKnobs::from_config_json(&json);
    assert_eq!(knobs.ui_pending_warn_ms, 5_000);
    assert_eq!(knobs.max_concurrent, 1);
    assert_eq!(knobs.doc_level_change, "detailed");
    // 非法值回落：
    assert_eq!(knobs.stall_warn_ms, 900_000, "0 不是合法告警阈值，回落缺省");
    assert_eq!(
        knobs.audit_concurrency, 8,
        "超上限按既有生产语义 clamp（1..=8），不拒收"
    );
    let keys: Vec<_> = sources.iter().map(|(k, _)| *k).collect();
    assert!(keys.contains(&"ui_pending_warn_ms"));
    assert!(!keys.contains(&"stall_warn_ms"), "回落的键不进来源表（来源=Default）");
}

/// 文件缺失（None）= 全缺省。
#[test]
fn missing_config_json_means_all_defaults() {
    let (knobs, sources) = RuntimeKnobs::from_config_json(&serde_json::Value::Null);
    assert_eq!(knobs, RuntimeKnobs::default());
    assert!(sources.is_empty());
}

/// 0 = 不限的映射口径：timeout 类旋钮 0 → None。
#[test]
fn zero_means_unlimited_for_timeout_knobs() {
    let json = serde_json::json!({
        "review_timeout_ms": 0,
        "ui_ask_timeout_ms": 0,
        "run_timeout_ms": 0,
        "install_timeout_ms": 0,
        "audit_batch_timeout_ms": 0
    });
    let (knobs, _) = RuntimeKnobs::from_config_json(&json);
    assert_eq!(knobs.review_timeout_ms, None);
    assert_eq!(knobs.ui_ask_timeout_ms, None);
    assert_eq!(knobs.run_timeout_ms, None);
    assert_eq!(knobs.install_timeout_ms, None);
    assert_eq!(knobs.audit_batch_timeout_ms, None);
}

/// 全局 init + typed getter：init 后 getter 照新值；with_mut 可独占改。
#[test]
fn global_init_then_getters_reflect_file_layer() {
    let _guard = test_lock(); // 全局单例：并行用例整段串行
    let json = serde_json::json!({"ui_pending_warn_ms": 42_000});
    init(Some(&json));
    assert_eq!(ui_pending_warn_ms(), 42_000);
    // with_mut 独占改（模拟测试注入；来源不变——来源表只记 file/cli 层）
    with_mut(|k| k.max_concurrent = 3);
    assert_eq!(max_concurrent(), 3);
    // 恢复缺省（不污染其他用例）
    init(None);
    assert_eq!(ui_pending_warn_ms(), 120_000);
    assert_eq!(max_concurrent(), 2);
}

/// 生效值端点数据：key/来源/单位/语义齐全，file 覆盖可见。
#[test]
fn effective_values_carry_source_labels() {
    let _guard = test_lock();
    let json = serde_json::json!({"max_concurrent": 1});
    init(Some(&json));
    let eff = effective_values();
    assert_eq!(eff.len(), KNOB_TABLE.len(), "生效值必须覆盖声明表全量");
    let mc = eff.iter().find(|e| e.key == "max_concurrent").unwrap();
    assert_eq!(mc.value, 1);
    assert_eq!(mc.source, KnobSource::File);
    assert!(!mc.doc.is_empty());
    let stall = eff.iter().find(|e| e.key == "stall_warn_ms").unwrap();
    assert_eq!(stall.source, KnobSource::Default, "未覆盖的旋钮来源=default");
    init(None);
}

/// DOC_LEVEL 收编裁决的回归钉：两个旋钮缺省各异、互不串味。
#[test]
fn doc_level_split_knobs_keep_their_own_defaults() {
    let (knobs, _) = RuntimeKnobs::from_config_json(&serde_json::json!({
        "doc_level_change": "detailed"
    }));
    assert_eq!(knobs.doc_level_change, "detailed");
    assert_eq!(
        knobs.doc_level_docgen, "complete",
        "change 侧的覆盖不得串到 docgen 侧（漂移收编后的核心回归）"
    );
}
