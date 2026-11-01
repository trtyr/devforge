//! server::auth 单测（atask-2）：签发/验证/滑动续期/过期清扫/cookie 解析/回环判定。
//!
//! 会话表是 ServerState 内的 RwLock——每个测试自建 state（Arc 独占期注入），不共享。

use super::*;
use crate::config::installer::FakeInstaller;
use crate::config::ConfigManifest;
use crate::domain::auth::{hash_password, Role};
use crate::domain::store::Store;
use crate::domain::SopRegistry;
use crate::observability::events::EventBus;
use crate::server::{ServerState, UiRelay};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

fn base_state(store: Store) -> Arc<ServerState> {
    let bus = Arc::new(EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    ServerState::new(
        store,
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        // auth 单测不触发 run（不 start 任务）——引擎工厂永不调用
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("auth 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        PathBuf::from("/tmp/auth-profiles"),
        PathBuf::from("/tmp/auth-plans"),
        PathBuf::from("/tmp/auth-sessions"),
    )
}

fn state_with_account(pw: &str) -> Arc<ServerState> {
    let mut store = Store::new();
    store.set_auth_account("admin", &hash_password(pw), Role::Admin, 1_000);
    base_state(store)
}

/// 未初始化账户 → NotInitialized（文案与密码错误刻意区分）。
#[test]
fn login_before_init_is_not_initialized() {
    let st = base_state(Store::new());
    assert_eq!(login(&st, "admin", "pw", 1), Err(LoginError::NotInitialized));
}

/// 正确口令签发会话；错口令/错用户名拒。
#[test]
fn login_issues_session_and_rejects_bad_credentials() {
    let st = state_with_account("s3cret");
    // R4-EX-3：失败进冷却窗——三个时间戳必须彼此错开各自的退避窗（500ms/1s/2s）
    assert_eq!(
        login(&st, "admin", "wrong", 1_000),
        Err(LoginError::BadPassword)
    );
    assert_eq!(
        login(&st, "root", "s3cret", 5_000),
        Err(LoginError::UnknownUser)
    );
    let (token, role) = login(&st, "admin", "s3cret", 10_000).expect("正确口令应签发");
    assert_eq!(role, Role::Admin);
    assert_eq!(
        validate_session(&st, &token, 1_100),
        Some(("admin".into(), Role::Admin)),
        "签发的会话立即可验"
    );
}

/// 滑动续期：验证即续；过期条目被清扫（不续、不可用）。
#[test]
fn sessions_slide_and_expire() {
    let st = state_with_account("pw");
    let (t1, _) = login(&st, "admin", "pw", 1_000).unwrap();
    let (t2, _) = login(&st, "admin", "pw", 2_000).unwrap();
    // t1 在 1_000+TTL 到期；在 999_000 验证 → 续到 999_000+TTL
    assert!(validate_session(&st, &t1, 999_000).is_some(), "续期成功");
    // 再过 TTL-1s：t1 刚续过 → 活着（注意 retain 是全表清扫，先验存活再验清扫）
    let late = 1_000_000 + SESSION_TTL_MS;
    assert!(
        validate_session(&st, &t1, late - 1_001).is_some(),
        "t1 的续期到期 = 999_000+TTL，在其前一秒仍活"
    );
    assert!(validate_session(&st, &t2, late).is_none(), "t2 未续已过期被清扫");
    // 未知 token → None
    assert!(validate_session(&st, "nope", 1_000).is_none());
}

/// 登出幂等：登出后 token 失效；再登出不炸。
#[test]
fn logout_invalidates_and_is_idempotent() {
    let st = state_with_account("pw");
    let (t, _) = login(&st, "admin", "pw", 1_000).unwrap();
    logout(&st, &t);
    assert!(validate_session(&st, &t, 1_100).is_none(), "登出后会话失效");
    logout(&st, &t); // 幂等
}

/// cookie 解析：多段、空格、空值容错。
#[test]
fn cookie_parsing_extracts_session_token() {
    assert_eq!(
        token_from_cookie("a=1; devforge_session=tok123; b=2"),
        Some("tok123".into())
    );
    assert_eq!(token_from_cookie("devforge_session=tok"), Some("tok".into()));
    assert_eq!(token_from_cookie("devforge_session="), None, "空值不算");
    assert_eq!(token_from_cookie("other=x"), None);
    assert_eq!(token_from_cookie(""), None);
}

/// Set-Cookie 形态：HttpOnly + SameSite=Lax + Path=/ + Max-Age；登出 cookie 清值；
/// R4-EX-11：非回环绑定时附 Secure（本机 http 登录不加——浏览器拒收回环 https-only cookie）。
#[test]
fn cookie_values_carry_security_attributes() {
    let v = session_cookie_value("tok", false);
    assert!(v.starts_with("devforge_session=tok;"));
    assert!(v.contains("HttpOnly"));
    assert!(v.contains("SameSite=Lax"));
    assert!(v.contains("Path=/"));
    assert!(v.contains(&format!("Max-Age={}", SESSION_TTL_MS / 1000)));
    assert!(!v.contains("Secure"), "回环绑定不加 Secure");
    let vs = session_cookie_value("tok", true);
    assert!(vs.contains("; Secure"), "非回环绑定附 Secure：{vs}");
    let c = cleared_cookie_value(false);
    assert!(c.contains("Max-Age=0"), "登出 cookie 立即过期");
    assert!(cleared_cookie_value(true).contains("; Secure"));
}

/// 回环判定：真实回环地址 true、非回环 false、ConnectInfo 缺席 = **不豁免**
/// （R4-EX-12/R2-15 fail-closed：无对端信息 = 无法证明本机；测试须显式插回环对端）。
#[test]
fn loopback_peer_detection() {
    assert!(is_loopback_peer(Some(
        "127.0.0.1:5000".parse::<SocketAddr>().unwrap()
    )));
    assert!(!is_loopback_peer(Some(
        "192.168.1.9:5000".parse::<SocketAddr>().unwrap()
    )));
    assert!(!is_loopback_peer(None), "fail-closed：无对端信息不豁免");
}

/// R4-EX-3：连败指数退避 + 达阈值锁定；锁定期正确口令也拒；期满成功并清零。
#[test]
fn login_throttle_backs_off_locks_and_resets() {
    let st = state_with_account("s3cret");
    // 4 次错口令（间隔避开各自冷却窗：backoff 500ms/1s/2s/4s）
    for now in [1_000u64, 11_000, 21_000, 31_000] {
        assert!(matches!(
            login(&st, "admin", "wrong", now),
            Err(LoginError::BadPassword)
        ));
    }
    // 第 5 次失败 → 锁定（41_000 + 15min = 941_000）；本次自身仍返回 BadPassword
    assert!(matches!(
        login(&st, "admin", "wrong", 41_000),
        Err(LoginError::BadPassword)
    ));
    // 锁定期：正确口令也拒（带剩余等待时间）
    assert!(matches!(
        login(&st, "admin", "s3cret", 42_000),
        Err(LoginError::Throttled { retry_after_ms: 899_000 })
    ));
    // 期满：正确口令成功（清零）
    assert!(login(&st, "admin", "s3cret", 941_001).is_ok());
    // 清零验证：再错一次 backoff 从 500ms 重新起（本次返回 BadPassword，
    // 冷却窗在下一欠才生效）
    assert!(matches!(
        login(&st, "admin", "wrong", 941_002),
        Err(LoginError::BadPassword)
    ));
    // 冷却窗内（941_002+500 之前）→ Throttled
    assert!(matches!(
        login(&st, "admin", "wrong", 941_100),
        Err(LoginError::Throttled { .. })
    ));
    assert!(login(&st, "admin", "s3cret", 941_502).is_ok());
}

/// R3-06 时序口径的行为面：错用户名路径也记失败（限速同一本账——
/// 哈希先跑，但失败记账不区分哪半边错）。
#[test]
fn unknown_user_failures_count_into_throttle() {
    let st = state_with_account("pw");
    assert_eq!(login(&st, "root", "x", 1_000), Err(LoginError::UnknownUser));
    // 冷却期内再试（任何凭据）→ Throttled
    assert!(matches!(
        login(&st, "root", "x", 1_100),
        Err(LoginError::Throttled { .. })
    ));
}
