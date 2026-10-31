//! 渠道熔断（gap-B · 派发前渠道熔断）单测。
//!
//! 本文件按步长出：step 1 = 存储层（熔断表落盘 / 读回 / 到期清理），
//! step 2 = 签名与归因纯函数，step 3 = tick 扫描步，step 4 = 派发闸门与状态端点。
//!
//! 口径提醒（计划 §5）：
//! - 熔断条目存**绝对到期时刻**（unix ms）——读回后逐毫秒相等，不许漂；
//! - 旧版本 store.json（无 `channel_breakers` 键）必须能读回（数据迁移方向）。

use crate::domain::store::Store;

/// 记下的熔断条目在落盘 → 读回后渠道、到期时刻、原因摘要逐项相等。
#[test]
fn a_recorded_breaker_survives_a_store_roundtrip_with_the_same_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let expires = 1_700_000_000_000u64;
    {
        let mut store = Store::new().with_persist(path.clone());
        store.record_channel_breaker("deepseek-flash", expires, "402 Payment Required");
    }
    let loaded = Store::load(&path).unwrap();
    assert_eq!(loaded.channel_breakers.len(), 1, "熔断条目应随 store 落盘");
    let b = &loaded.channel_breakers[0];
    assert_eq!(b.channel, "deepseek-flash");
    assert_eq!(b.expires_at_ms, expires, "到期时刻是绝对时间戳，读回不漂");
    assert_eq!(b.last_seen_reason, "402 Payment Required");
}

/// 旧版本 store.json（没有 channel_breakers 键）必须能读回——否则升级即炸。
#[test]
fn a_store_json_without_the_breaker_field_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    std::fs::write(&path, r#"{"projects":[],"tasks":[],"runs":[]}"#).unwrap();
    let loaded = Store::load(&path).unwrap();
    assert!(
        loaded.channel_breakers.is_empty(),
        "旧文件读回后熔断表为空（不是报错，也不是凭空造条目）"
    );
}

/// 同一条渠道重复记熔断：条目仍是**一条**，到期取较晚者，原因摘要刷新为最新。
#[test]
fn recording_the_same_channel_twice_keeps_one_entry_with_the_later_expiry() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 2_000, "第一次");
    store.record_channel_breaker("deepseek-flash", 5_000, "第二次");
    store.record_channel_breaker("deepseek-flash", 3_000, "第三次");
    assert_eq!(store.channel_breakers.len(), 1, "同渠道只留一条");
    assert_eq!(
        store.channel_breakers[0].expires_at_ms, 5_000,
        "到期取较晚者（后一次来得早不缩短已定的冷却窗口）"
    );
    assert_eq!(store.channel_breakers[0].last_seen_reason, "第三次");
}

/// 剩余冷却毫秒按「到期时刻 − 现在」算；到期即不再算冷却。
#[test]
fn remaining_ms_counts_down_to_the_expiry_and_stops_at_zero() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 5_000, "402");
    assert_eq!(store.channel_breaker_remaining_ms("deepseek-flash", 4_000), 1_000);
    assert_eq!(store.channel_breaker_remaining_ms("deepseek-flash", 5_000), 0);
    assert_eq!(store.channel_breaker_remaining_ms("grok-4.7", 4_000), 0);
}

/// 过期条目被清掉（TTL 过后自然解除），未到期的留下。
#[test]
fn expired_breakers_are_purged_and_the_live_one_survives() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 1_000, "402");
    store.record_channel_breaker("grok-4.7", 5_000, "429");
    assert_eq!(store.purge_expired_channel_breakers(999), 0, "未到期不动手");
    assert_eq!(store.channel_breakers.len(), 2);
    assert_eq!(store.purge_expired_channel_breakers(1_000), 1, "到期那条被清");
    assert_eq!(store.channel_breakers.len(), 1);
    assert_eq!(store.channel_breakers[0].channel, "grok-4.7");
    assert_eq!(
        store.active_channel_breakers(1_000).len(),
        1,
        "活跃列表只含未到期条目"
    );
    assert_eq!(store.channel_breakers[0].expires_at_ms, 5_000);
}
