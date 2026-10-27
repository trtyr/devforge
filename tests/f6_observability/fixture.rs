//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, guards::*, milestone::*, panic::*, runlog::*, support::*};

/// 造一个服务器夹具（观测根钉在夹具的临时目录里 → 不落用户 HOME）。
pub(super) fn server_fixture(hang_ms: u64, fail_system_times: u32) -> ServerFixture {
    let dir = tempfile::tempdir().expect("临时目录");
    let obs_root = dir.path().join("observability");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let failures = Arc::new(Mutex::new(fail_system_times));
    let engine_factory = Box::new(move || -> Box<dyn RunEngine + Send> {
        Box::new(MockEngine {
            calls: calls2.clone(),
            hang_ms,
            fail_system_times: failures.clone(),
        })
    });
    let bus = Arc::new(devforge::observability::events::EventBus::new());
    let ui = Arc::new(devforge::server::UiRelay::new(bus.clone()));
    let mut state = devforge::server::ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        devforge::config::ConfigManifest::builtin(),
        engine_factory,
        bus,
        ui,
        Arc::new(devforge::config::installer::FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // 单测不真睡：重试退避归零
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .retry_policy = RetryPolicy::fast();
    // 观测根钉在夹具的临时目录
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .obs_root = Some(obs_root.clone());
    let router = devforge::server::build_router(state.clone());
    ServerFixture {
        router,
        state,
        calls,
        obs_root,
        _dir: dir,
    }
}

pub(super) async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    use tower::ServiceExt as _;
    let builder = axum::http::Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(v.to_string()))
            .expect("请求构造"),
        None => builder.body(axum::body::Body::empty()).expect("请求构造"),
    };
    router.clone().oneshot(req).await.expect("端点调用")
}

pub(super) async fn body_json(resp: axum::response::Response) -> Value {
    use http_body_util::BodyExt as _;
    let bytes = resp.into_body().collect().await.expect("取响应体").to_bytes();
    serde_json::from_slice(&bytes).expect("响应应为 JSON")
}

/// 建项目 + 建任务（走真实 HTTP 面）。
pub(super) async fn make_task(f: &ServerFixture, project: &str, task: &str, sop: &str) {
    let local = f
        .state
        .plans_root
        .parent()
        .expect("根")
        .join(format!("ws-{project}"));
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": project, "local_path": local.to_string_lossy()})),
    )
    .await;
    send(
        &f.router,
        "POST",
        &format!("/api/v1/projects/{project}/tasks"),
        Some(json!({"id": task, "sop": sop})),
    )
    .await;
}

/// 等 run 进终态。
pub(super) async fn wait_settled(f: &ServerFixture, project: &str, task: &str) {
    for _ in 0..300 {
        let body = body_json(
            send(
                &f.router,
                "GET",
                &format!("/api/v1/projects/{project}/tasks/{task}/runs"),
                None,
            )
            .await,
        )
        .await;
        let st = body["runs"]
            .as_array()
            .and_then(|runs| runs.last())
            .and_then(|r| r["status"].as_str())
            .unwrap_or("");
        if st == "settled" || st == "failed" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("run 未在上限内进入终态：{project}/{task}");
}

/// 事件总线里的全部事件（回放缓冲）。
pub(super) fn events(f: &ServerFixture) -> Vec<String> {
    f.state
        .bus
        .replay_after(0)
        .into_iter()
        .map(|(_, data)| data)
        .collect()
}

// ============================================================ 需求 1：原始错误全量可见


