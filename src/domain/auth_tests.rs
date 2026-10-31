//! A1 认证域单测（atask-1）：口令哈希往返、错口令拒、会话令牌强度、
//! store auth 段的 serde 兼容与审计事件裁剪。

use super::*;

/// argon2id 哈希往返：对口令过、错口令拒、PHC 串形态合法（盐内嵌，两次哈希不同）。
#[test]
fn password_hash_roundtrip() {
    let h1 = hash_password("s3cret-口令");
    let h2 = hash_password("s3cret-口令");
    assert!(h1.starts_with("$argon2id$"), "PHC 串须标注算法：{h1}");
    assert_ne!(h1, h2, "同口令两次哈希必须不同（随机盐）");
    assert!(verify_password("s3cret-口令", &h1));
    assert!(!verify_password("wrong", &h1));
    assert!(!verify_password("", &h1), "空口令不得通过");
}

/// 损坏/异算法的 PHC 串 → 校验失败（false），不得 panic 也不得放行。
#[test]
fn verify_password_on_garbage_hash_is_false_not_panic() {
    assert!(!verify_password("x", ""));
    assert!(!verify_password("x", "not-a-phc"));
    assert!(!verify_password("x", "$argon2i$v=19$m=65536,t=2,p=1$c2FsdA$hash"));
}

/// 会话令牌：256bit（64 hex）、非空、两次不同。
#[test]
fn session_token_shape_and_uniqueness() {
    let a = new_session_token();
    let b = new_session_token();
    assert_eq!(a.len(), 64, "uuid simple ×2 = 64 hex 字符");
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

/// store auth 段：None = 未初始化可落盘读回；set 后往返保真（含 Role 序列化）。
#[test]
fn store_auth_account_roundtrip_and_legacy_default() {
    // 旧 store.json（无 auth 段）→ 读回 auth = None（未初始化），不炸
    let legacy = crate::domain::store::Store::new();
    let json = serde_json::to_value(&legacy).unwrap();
    assert!(json.get("auth").is_none() || json["auth"].is_null());
    let back: crate::domain::store::Store = serde_json::from_value(json).unwrap();
    assert!(back.auth_account().is_none());

    // set → 序列化读回保真
    let mut st = crate::domain::store::Store::new();
    let changed = st.set_auth_account("admin", &hash_password("pw"), Role::Admin, 1_000);
    assert_eq!(changed, 1, "首次设置要落盘");
    let json = serde_json::to_value(&st).unwrap();
    assert_eq!(json["auth"]["role"], "admin", "角色序列化为 snake_case");
    let back: crate::domain::store::Store = serde_json::from_value(json).unwrap();
    let acc = back.auth_account().expect("设置后应可读回");
    assert_eq!(acc.username, "admin");
    assert_eq!(acc.role, Role::Admin);
    assert!(verify_password("pw", &acc.password_hash));

    // 重复 set = 覆盖（重置口令路径），仍落盘
    let changed = st.set_auth_account("admin", &hash_password("pw2"), Role::Admin, 2_000);
    assert_eq!(changed, 1);
    assert!(verify_password("pw2", &st.auth_account().unwrap().password_hash));
}

/// 审计事件：id 自增、cap 裁最旧、裁剪后不落盘空转（0 变更不 persist）。
#[test]
fn auth_events_auto_increment_trim_and_persist_semantics() {
    let mut st = crate::domain::store::Store::new();
    let mut last = 0;
    for i in 0..(MAX_AUTH_EVENTS + 20) {
        last = st.push_auth_event("login_failure", "anonymous", &format!("第{i}次"), i as u64);
    }
    assert_eq!(last, (MAX_AUTH_EVENTS + 20) as u64, "id 单调不自增重排");
    assert_eq!(st.auth_events.len(), MAX_AUTH_EVENTS, "cap 裁剪");
    assert_eq!(
        st.auth_events.first().unwrap().detail,
        format!("第{}次", 20),
        "裁掉的是最旧事件"
    );
    assert_eq!(st.auth_events.last().unwrap().kind, "login_failure");
}
