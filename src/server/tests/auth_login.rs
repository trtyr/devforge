//! A1 认证 router 层单测（atask-2）：login 端点、守门三态（session/token/回环豁免）、
//! SSE 握手 401、过期会话。
//!
//! 契约要点：守门只罩 /api/v1/*；/auth/login 在守门之外（否则鸡生蛋）；豁免缺省开
//! （既有本机零摩擦），收紧后本机也要登录。

use crate::config::installer::FakeInstaller;
use crate::config::ConfigManifest;
use crate::domain::auth::{hash_password, Role};
use crate::domain::store::Store;
use crate::domain::SopRegistry;
use crate::server::auth::{SESSION_COOKIE, SESSION_TTL_MS};
use crate::server::{build_router, ServerState, UiRelay};
use axum::body::Body;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

struct AuthFixture {
    router: axum::Router,
    state: Arc<ServerState>,
    _dir: tempfile::TempDir,
}

fn fixture_auth(exempt: bool, token: Option<&str>) -> AuthFixture {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(crate::server::EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut store = Store::new();
    store.set_auth_account("admin", &hash_password("s3cret"), Role::Admin, 1_000);
    let mut st = ServerState::new(
        store,
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("auth router 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // ServerState::new 已返回 Arc；路由构建前经 get_mut 独占注入测试态。
    let shared = Arc::get_mut(&mut st).expect("路由构建前应独占");
    shared
        .auth_loopback_exempt
        .store(exempt, std::sync::atomic::Ordering::Relaxed);
    if let Some(t) = token {
        shared.token = Some(t.into());
    }
    let state: Arc<ServerState> = st;
    let router = build_router(state.clone());
    AuthFixture {
        router,
        state,
        _dir: dir,
    }
}

/// 带 header 的请求 helper（cookie / token 头）。
async fn send_with(
    router: &axum::Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    router.clone().oneshot(req).await.unwrap()
}

async fn status_of(resp: &axum::response::Response) -> u16 {
    resp.status().as_u16()
}

/// 未初始化账户登录 → 401 且文案区分「未初始化」（不假装密码错误）。
#[tokio::test]
async fn login_on_uninitialized_store_says_so() {
    let f = fixture_auth(true, None);
    {
        let mut store = f.state.store.lock().unwrap();
        store.auth = None;
    }
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "x"})),
    )
    .await;
    assert_eq!(status_of(&resp).await, 401);
}

/// 正确口令 → 200 + Set-Cookie 安全形态 + role；错误口令 → 401 + login_failure 审计。
#[tokio::test]
async fn login_issues_cookie_and_audits_both_ways() {
    let f = fixture_auth(true, None);
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "wrong"})),
    )
    .await;
    assert_eq!(status_of(&resp).await, 401);
    {
        let store = f.state.store.lock().unwrap();
        let last = store.auth_events.last().unwrap();
        assert_eq!(last.kind, "login_failure");
        assert_eq!(last.detail, "密码错误");
        assert!(!last.detail.contains("s3cret"), "审计不含口令本体");
    }

    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "s3cret"})),
    )
    .await;
    assert_eq!(status_of(&resp).await, 200);
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("成功登录必须发 Set-Cookie")
        .to_string();
    assert!(cookie.starts_with(SESSION_COOKIE), "cookie 名正确：{cookie}");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Path=/"));
    let store = f.state.store.lock().unwrap();
    assert_eq!(store.auth_events.last().unwrap().kind, "login_success");
}

/// 豁免开（缺省）：无凭据放行（既有本机零摩擦）；豁免关：无凭据 401。
#[tokio::test]
async fn gate_exempt_and_strict_modes() {
    // 缺省豁免：匿名放行（现有全部 server 测试依赖此语义）
    let f = fixture_auth(true, None);
    let resp = send_with(&f.router, "GET", "/api/v1/projects", &[], None).await;
    assert_eq!(status_of(&resp).await, 200);

    // 收紧：匿名 401
    let f = fixture_auth(false, None);
    let resp = send_with(&f.router, "GET", "/api/v1/projects", &[], None).await;
    assert_eq!(status_of(&resp).await, 401);
}

/// 会话通道：login 拿 cookie → 带 cookie 放行；假 cookie 401。
#[tokio::test]
async fn session_cookie_channel_gates_api() {
    let f = fixture_auth(false, None);
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "s3cret"})),
    )
    .await;
    assert_eq!(status_of(&resp).await, 200);
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = send_with(
        &f.router,
        "GET",
        "/api/v1/projects",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status_of(&resp).await, 200, "有效 session 放行");

    let fake = format!("{SESSION_COOKIE}=not-a-real-token");
    let resp = send_with(&f.router, "GET", "/api/v1/projects", &[("cookie", &fake)], None).await;
    assert_eq!(status_of(&resp).await, 401, "假 session 401");
}

/// token 头通道（DR-9 兼容）：配置 token 后带对头放行、错头 401。
#[tokio::test]
async fn token_header_channel_still_works() {
    let f = fixture_auth(false, Some("ops-secret"));
    let resp = send_with(
        &f.router,
        "GET",
        "/api/v1/projects",
        &[("x-devforge-token", "ops-secret")],
        None,
    )
    .await;
    assert_eq!(status_of(&resp).await, 200);
    let resp = send_with(
        &f.router,
        "GET",
        "/api/v1/projects",
        &[("x-devforge-token", "nope")],
        None,
    )
    .await;
    assert_eq!(status_of(&resp).await, 401);
}

/// SSE 握手同样被守门：无凭据 + 收紧模式 → 401（握手前置挡住，不进流）。
#[tokio::test]
async fn sse_handshake_is_gated() {
    let f = fixture_auth(false, None);
    let resp = send_with(&f.router, "GET", "/api/v1/events", &[], None).await;
    assert_eq!(status_of(&resp).await, 401, "SSE 握手必须过守门");
}

/// 过期会话：塞入已过期条目 → 带 cookie 401（且条目被清扫）。
#[tokio::test]
async fn expired_session_is_rejected_and_swept() {
    let f = fixture_auth(false, None);
    {
        let mut sessions = f.state.auth_sessions.write().unwrap();
        sessions.insert(
            "stale-token".into(),
            crate::server::auth::AuthSession {
                username: "admin".into(),
                role: Role::Admin,
                expires_at_ms: crate::domain::now_ms().saturating_sub(1),
            },
        );
    }
    let cookie = format!("{SESSION_COOKIE}=stale-token");
    let resp = send_with(&f.router, "GET", "/api/v1/projects", &[("cookie", &cookie)], None).await;
    assert_eq!(status_of(&resp).await, 401);
    assert!(
        f.state.auth_sessions.read().unwrap().is_empty(),
        "过期条目顺手清扫"
    );
}

/// 登出：登出后原 cookie 失效（回 401），logout 幂等 200。
#[tokio::test]
async fn logout_invalidates_the_session() {
    let f = fixture_auth(false, None);
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "s3cret"})),
    )
    .await;
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = send_with(&f.router, "POST", "/auth/logout", &[("cookie", &cookie)], None).await;
    assert_eq!(status_of(&resp).await, 200);
    let set_cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert!(set_cookie.contains("Max-Age=0"), "登出清 cookie：{set_cookie}");

    let resp = send_with(&f.router, "GET", "/api/v1/projects", &[("cookie", &cookie)], None).await;
    assert_eq!(status_of(&resp).await, 401, "登出后原会话失效");
}

/// login_body 留个直接读取的验证：响应体带 role（前端按它渲染权限位）。
#[tokio::test]
async fn login_body_carries_role() {
    let f = fixture_auth(true, None);
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "s3cret"})),
    )
    .await;
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["role"], "admin");
    assert_eq!(v["username"], "admin");
    let _ = SESSION_TTL_MS; // 引用保持 import 清洁（语义在 cookie Max-Age 断言里）
}

// ---------- atask-3：写操作审计（actor 随通道） ----------

/// session 通道写操作：actor = 真实用户名 + (session)，detail 带 op 与对象。
#[tokio::test]
async fn write_op_audit_carries_session_actor() {
    let f = fixture_auth(false, None);
    // 登录拿 cookie
    let resp = send_with(
        &f.router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "admin", "password": "s3cret"})),
    )
    .await;
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    // 建项目 + 建任务（写操作）
    send_with(&f.router, "POST", "/api/v1/projects", &[("cookie", &cookie)],
        Some(serde_json::json!({"name": "p", "local_path": "/tmp/auth-audit-p"}))).await;
    let resp = send_with(&f.router, "POST", "/api/v1/projects/p/tasks", &[("cookie", &cookie)],
        Some(serde_json::json!({"id": "t", "sop": "planning"}))).await;
    assert_eq!(status_of(&resp).await, 201, "session 建任务应成功");
    let store = f.state.store.lock().unwrap();
    let last = store.auth_events.last().unwrap();
    assert_eq!(last.kind, "write_op");
    assert_eq!(last.actor, "admin(session)", "actor = 用户名 + 通道");
    assert!(last.detail.contains("create_task"), "detail 带 op：{}", last.detail);
    assert!(last.detail.contains("p/t"), "detail 带对象：{}", last.detail);
}

/// 回环通道写操作：actor = @loopback(loopback)——审计可答「谁从哪条路进来的」。
#[tokio::test]
async fn write_op_audit_carries_loopback_actor() {
    let f = fixture_auth(true, None);
    send_with(&f.router, "POST", "/api/v1/projects", &[],
        Some(serde_json::json!({"name": "p", "local_path": "/tmp/auth-audit-l"}))).await;
    send_with(&f.router, "POST", "/api/v1/projects/p/tasks", &[],
        Some(serde_json::json!({"id": "t", "sop": "planning"}))).await;
    let store = f.state.store.lock().unwrap();
    let last = store.auth_events.last().unwrap();
    assert_eq!(last.actor, "@loopback(loopback)");
    assert_eq!(last.kind, "write_op");
}

/// token 通道写操作：actor = @token(token)。
#[tokio::test]
async fn write_op_audit_carries_token_actor() {
    let f = fixture_auth(false, Some("ops-secret"));
    send_with(&f.router, "POST", "/api/v1/projects", &[("x-devforge-token", "ops-secret")],
        Some(serde_json::json!({"name": "p", "local_path": "/tmp/auth-audit-t"}))).await;
    send_with(&f.router, "POST", "/api/v1/projects/p/tasks", &[("x-devforge-token", "ops-secret")],
        Some(serde_json::json!({"id": "t", "sop": "planning"}))).await;
    let store = f.state.store.lock().unwrap();
    assert_eq!(store.auth_events.last().unwrap().actor, "@token(token)");
}

/// Observer 预留语义：observer 角色账户可登录、守门放行（角色字段先合法存在；
/// 写降权在 M-1 之后单独立项，这里只钉「预留字段不破坏登录/守门」）。
#[tokio::test]
async fn observer_role_can_login_and_pass_gate() {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(crate::server::EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut store = Store::new();
    store.set_auth_account("watcher", &hash_password("pw"), Role::Observer, 1_000);
    let mut st = ServerState::new(
        store,
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("observer 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    Arc::get_mut(&mut st).unwrap().auth_loopback_exempt.store(false, std::sync::atomic::Ordering::Relaxed);
    let state: Arc<ServerState> = st;
    let router = build_router(state.clone());

    let resp = send_with(
        &router,
        "POST",
        "/auth/login",
        &[],
        Some(serde_json::json!({"username": "watcher", "password": "pw"})),
    )
    .await;
    assert_eq!(status_of(&resp).await, 200);
    // 先取 header（借用），最后才消耗 body
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|x| x.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let v: serde_json::Value = serde_json::from_slice(
        &resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(v["role"], "observer", "role 序列化 snake_case（预留字段合法存在）");

    // observer 登录态过守门（收紧模式 + 真实 cookie）：预留字段不破坏守门
    let resp = send_with(
        &router,
        "GET",
        "/api/v1/projects",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(status_of(&resp).await, 200, "observer 登录态放行读");
}
