//! 服务器端点单测（内存态，oneshot，不碰真 pi/网络）。

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::config::ConfigManifest;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::task::Task;
use crate::domain::run::Run;
use crate::error::Result;
use crate::server::{build_router, ServerState, UiRelay};
use tokio_stream::StreamExt as _;

use std::sync::{Arc, Mutex};

/// 测试引擎：立刻完成并记录调用；hang_ms>0 时模拟长 run（响应取消/超时干预）。
struct MockEngine {
    calls: Arc<Mutex<Vec<String>>>,
    hang_ms: u64,
}
impl RunEngine for MockEngine {
    fn execute(&mut self, task: &Task, _run: &Run, control: &RunControl) -> Result<RunOutcome> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}/{}", task.project, task.id));
        if self.hang_ms > 0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(self.hang_ms);
            while std::time::Instant::now() < deadline {
                if control.is_cancelled() {
                    return Ok(RunOutcome::Failed {
                        reason: "已取消（cancel 端点）".into(),
                    });
                }
                if control.timed_out(crate::domain::now_ms()) {
                    return Ok(RunOutcome::Failed {
                        reason: "已超时（run 超时，被 watchdog 终止）".into(),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        Ok(RunOutcome::Completed)
    }
}

struct Fixture {
    router: axum::Router,
    state: ServerStateHandle,
    calls: Arc<Mutex<Vec<String>>>,
    _dir: tempfile::TempDir,
}
type ServerStateHandle = Arc<ServerState>;

fn fixture() -> Fixture {
    fixture_with_token(None)
}

fn fixture_with_token(token: Option<&str>) -> Fixture {
    fixture_with_engine(0, None, token)
}

/// 可注入长 run / 超时的夹具变体（取消与超时单测用）。
fn fixture_with_engine(hang_ms: u64, timeout_ms: Option<u64>, token: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let engine_factory = Box::new(move || -> Box<dyn RunEngine + Send> {
        Box::new(MockEngine {
            calls: calls2.clone(),
            hang_ms,
        })
    });
    let bus = std::sync::Arc::new(crate::server::EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut state = ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::planning_builtin(),
        engine_factory,
        bus,
        ui,
        std::sync::Arc::new(crate::config::installer::FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // token 在路由构建前注入（Arc 共享后不可再改）
    if let Some(t) = token {
        Arc::get_mut(&mut state).expect("路由未构建前应独占").token = Some(t.into());
    }
    if let Some(t) = timeout_ms {
        Arc::get_mut(&mut state).expect("路由未构建前应独占").run_timeout_ms = Some(t);
    }
    Fixture {
        router: build_router(state.clone()),
        state,
        calls,
        _dir: dir,
    }
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let builder = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    router.clone().oneshot(req).await.unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("响应应为 JSON")
}

#[tokio::test]
async fn projects_crud_with_local_dir_creation() {
    let f = fixture();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new-project");

    // 创建（201）且本地目录落地（decisions/001 手动指定 + T004 ensure）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "devforge", "local_path": target.to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 201);
    assert!(target.is_dir(), "项目目录应创建");

    // 重复 → 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "devforge", "local_path": target.to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    // 列表与详情
    let resp = send(&f.router, "GET", "/api/v1/projects", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["projects"].as_array().unwrap().len(), 1);

    let resp = send(&f.router, "GET", "/api/v1/projects/devforge", None).await;
    assert_eq!(resp.status(), 200);
    let resp = send(&f.router, "GET", "/api/v1/projects/ghost", None).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn task_creation_validates() {
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;

    // 未知 SOP → 400
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "coding"})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // 正常创建 → 201
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 201);

    // 重复 → 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    // 不存在项目下的任务 → 404
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/ghost/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn start_task_runs_engine_and_records() {
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;

    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);

    // 轮询至 run 结束（引擎是 mock，立即完成）
    let mut settled = false;
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        let body = body_json(resp).await;
        if body["runs"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
            && body["runs"][0]["status"] == "Settled"
        {
            settled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(settled, "run 应在超时前 Settled");

    // 引擎确实被调用
    assert_eq!(f.calls.lock().unwrap().len(), 1);

    // 再启动（任务已完成，可重跑）→ 新 run 留档
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn sse_pushes_events() {
    let f = fixture();
    // 延迟发事件（SSE 订阅发生在请求处理时，先请求后到事件）
    let st = f.state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        st.emit(json!({"type": "hello", "n": 1}));
    });

    let resp = send(&f.router, "GET", "/api/v1/events", None).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let mut stream = resp.into_body().into_data_stream();
    let mut got = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while got.is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "SSE 超时");
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
            .await
            .expect("chunk 超时");
        match chunk {
            Some(Ok(bytes)) => got.push_str(&String::from_utf8_lossy(&bytes)),
            _ => break,
        }
    }
    assert!(got.contains("hello"), "SSE 应收到事件: {got}");
}

#[test]
fn eventbus_assigns_sequential_ids_and_replays() {
    // A1 尾巴收口回归：序号单调、留档可回放、replay_after 过滤正确
    let bus = crate::server::EventBus::new();
    bus.emit(json!({"n": 1}));
    bus.emit(json!({"n": 2}));
    bus.emit(json!({"n": 3}));
    let all = bus.replay_after(0);
    let ids = all.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(ids, vec![1, 2, 3]);
    let after = bus.replay_after(1);
    let ids = after.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(ids, vec![2, 3]);
    assert_eq!(bus.last_id(), 3);
}

#[tokio::test]
async fn sse_last_event_id_replays_missed_events() {
    // A1 尾巴收口回归：断线期间错过的事件凭 Last-Event-ID 回放，并接上实时流
    let f = fixture();
    f.state.emit(json!({"type": "e1"}));
    f.state.emit(json!({"type": "e2"}));
    f.state.emit(json!({"type": "e3"}));

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/events")
        .header("last-event-id", "1")
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let mut stream = resp.into_body().into_data_stream();
    let mut got = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while !(got.contains("e2") && got.contains("e3")) {
        assert!(tokio::time::Instant::now() < deadline, "回放超时");
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
            .await
            .expect("chunk 超时");
        match chunk {
            Some(Ok(bytes)) => got.push_str(&String::from_utf8_lossy(&bytes)),
            _ => break,
        }
    }
    assert!(got.contains("e2") && got.contains("e3"), "应回放 2/3 号: {got}");
    assert!(!got.contains("e1"), "Last-Event-ID=1 不应回放 1 号: {got}");

    // 回放后接实时流（序号 4）
    f.state.emit(json!({"type": "e4"}));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while !got.contains("e4") {
        assert!(tokio::time::Instant::now() < deadline, "实时事件超时");
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
            .await
            .expect("chunk 超时");
        match chunk {
            Some(Ok(bytes)) => got.push_str(&String::from_utf8_lossy(&bytes)),
            _ => break,
        }
    }
    assert!(got.contains("e4"), "回放后应接上实时流: {got}");
}

#[tokio::test]
async fn sse_delivers_ui_request() {
    // P8-ISSUE-02 回归：ui.request 必须出现在 /events SSE 流上（复测误诊双通道，
    // 实测 UiRelay.requests 即共享 events 通道；此测试钉死该契约防回归）。
    let f = fixture();
    // 先订阅 SSE（broadcast 只投递订阅后的事件），再发起 ask——顺序不可倒置
    let resp = send(&f.router, "GET", "/api/v1/events", None).await;
    assert_eq!(resp.status(), 200);
    let mut stream = resp.into_body().into_data_stream();

    let st = f.state.clone();
    let handle = std::thread::spawn(move || {
        st.ui.ask_blocking("测试问题", &["选项A", "选项B"])
    });

    let mut got = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while !got.contains("ui.request") {
        assert!(tokio::time::Instant::now() < deadline, "SSE 未送出 ui.request");
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
            .await
            .expect("chunk 超时");
        match chunk {
            Some(Ok(bytes)) => got.push_str(&String::from_utf8_lossy(&bytes)),
            _ => break,
        }
    }
    assert!(got.contains("ui.request"), "SSE 应送出 ui.request: {got}");

    // 提取 id 并回填，释放阻塞的 ask 线程
    let v: Value = serde_json::from_str(
        got.split("data: ")
            .find(|l| l.contains("ui.request"))
            .and_then(|l| l.split('\n').next())
            .expect("data 行"),
    )
    .unwrap();
    let id = v["id"].as_str().unwrap().to_string();
    let resp = send(
        &f.router,
        "POST",
        &format!("/api/v1/ui/{id}/answer"),
        Some(json!({"value": "选项A"})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let answer = handle.join().unwrap().unwrap();
    assert_eq!(answer, "选项A");
}

#[tokio::test]
async fn auth_token_enforced_when_configured() {
    let f = fixture_with_token(Some("secret"));

    // 无头 → 401
    let resp = send(&f.router, "GET", "/api/v1/projects", None).await;
    assert_eq!(resp.status(), 401);

    // 对头 → 过
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/projects")
        .header("x-devforge-token", "secret")
        .body(Body::empty())
        .unwrap();
    let resp = f.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn manifest_get_put_roundtrip() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/config/manifest", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["model_pool"][0]["id"], "grok-4.7"); // 固定分配：审查 → grok-4.7（池序首）

    // 换模型（模型管理面的最简形态）
    let mut next = ConfigManifest::planning_builtin();
    next.model_pool[0].model_id = "deepseek-v3".into();
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/config/manifest",
        Some(serde_json::to_value(&next).unwrap()),
    )
    .await;
    assert_eq!(resp.status(), 202); // P7-ISSUE-03：渲染异步化，同步只生效真源

    let resp = send(&f.router, "GET", "/api/v1/config/manifest", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["model_pool"][0]["model_id"], "deepseek-v3");

    // P3-ISSUE-02 修复：真源同步落盘（manifest_path 注入后 PUT 即写盘）
    *f.state.manifest_path.lock().unwrap() = Some(f._dir.path().join("manifest.json"));
    let mut next2 = ConfigManifest::planning_builtin();
    next2.model_pool[1].model_id = "mokeywork/grok-4.7-tuned".into();
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/config/manifest",
        Some(serde_json::to_value(&next2).unwrap()),
    )
    .await;
    assert_eq!(resp.status(), 202);
    let saved = std::fs::read_to_string(f._dir.path().join("manifest.json")).unwrap();
    assert!(saved.contains("mokeywork/grok-4.7-tuned"), "真源应落盘: {saved}");
}

#[tokio::test]
async fn put_manifest_unknown_model_is_400() {
    // P3-ISSUE-01 修复：配置类错误 400（此前 500）
    let f = fixture();
    let mut next = ConfigManifest::planning_builtin();
    next.agents[0].model = "ghost-model".into();
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/config/manifest",
        Some(serde_json::to_value(&next).unwrap()),
    )
    .await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn project_create_rolls_back_when_local_dir_fails() {
    // P1-ISSUE-01 修复：目录落地失败回滚插入，不留半注册项目
    let f = fixture();
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "badpath", "local_path": "/dev/null/impossible/x"})),
    )
    .await;
    assert_eq!(resp.status(), 500);
    let resp = send(&f.router, "GET", "/api/v1/projects", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["projects"].as_array().unwrap().len(), 0, "应无半注册残留");
}

#[tokio::test]
async fn graph_endpoint_shows_nodes() {
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;

    let mut graph = Value::Null;
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/graph", None).await;
        graph = body_json(resp).await;
        if graph["nodes"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
            && graph["status"] == "Completed"
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(graph["sop"], "planning");
    assert!(graph["nodes"].as_array().unwrap().iter().all(|n| n["kind"] == "run"));
}

#[tokio::test]
async fn cancel_endpoint_stops_running_task() {
    // P8-ISSUE-01 修复：取消端点干预长 run
    let f = fixture_with_engine(10_000, None, None);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);

    // control 在 begin_run 后登记；轮询到取消端点可用为止
    let mut cancelled = false;
    for _ in 0..100 {
        let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/cancel", None).await;
        if resp.status() == 200 {
            cancelled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(cancelled, "取消端点应命中运行中的 run");

    // 引擎响应取消 → run Failed(已取消)，任务 Failed，不留悬空 Running
    let mut final_runs = Value::Null;
    for _ in 0..200 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        final_runs = body_json(resp).await;
        if final_runs["runs"][0]["status"] == "Failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(final_runs["runs"][0]["status"], "Failed");
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["tasks"][0]["status"], "Failed");
}

#[tokio::test]
async fn run_timeout_fails_run() {
    // P8-ISSUE-01 修复：可选超时（显式开启，零硬限制哲学）
    let f = fixture_with_engine(10_000, Some(200), None);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);

    let mut final_runs = Value::Null;
    for _ in 0..200 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        final_runs = body_json(resp).await;
        if final_runs["runs"][0]["status"] == "Failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(final_runs["runs"][0]["status"], "Failed");
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["tasks"][0]["status"], "Failed");
}

#[tokio::test]
async fn ui_abort_all_unblocks_ask() {
    // task-3 回归（机制层）：cancel 的解阻动作 abort_all 必须让阻塞中的 ask 立刻失败
    let f = fixture();
    let ui = f.state.ui.clone();
    let handle = std::thread::spawn(move || ui.ask_blocking("继续吗", &["完成"]));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while f.state.ui.pending_count() == 0 {
        assert!(std::time::Instant::now() < deadline, "dialog 未挂起");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let aborted = f.state.ui.abort_all();
    assert_eq!(aborted, 1);
    let r = handle.join().unwrap();
    assert!(r.is_err(), "abort 后 ask 应失败解阻");
    assert_eq!(f.state.ui.pending_count(), 0);
}

#[tokio::test]
async fn dialog_pending_cancel_unwinds_sop() {
    // task-3 回归（链路层）：dialog 挂起 + cancel 解阻 → SOP 以错误解阻
    // （execute 层将 is_cancelled 映射为 Failed{已取消}，映射已有）
    use super::RelayUserLink;
    use crate::sop::output::COMPLETION_TAG;
    use crate::sop::planning::{
        PlanAgentSession, PlanningLoop, ReviewSession, ReviewSessionFactory, UserLink,
    };

    struct FakePlan;
    impl PlanAgentSession for FakePlan {
        fn converse(&mut self, _message: &str, ui: &mut dyn UserLink) -> crate::error::Result<String> {
            let _answer = ui.ask("继续吗", &["完成"])?; // 阻塞点：等用户应答
            Ok(format!("收到。\n{COMPLETION_TAG}"))
        }
    }
    struct PassFactory;
    impl ReviewSessionFactory for PassFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn ReviewSession>> {
            Ok(Box::new(PassReview))
        }
    }
    struct PassReview;
    impl ReviewSession for PassReview {
        fn review(&mut self, _instruction: &str) -> crate::error::Result<String> {
            Ok("通过 <verdict>PASS</verdict>".into())
        }
    }

    let f = fixture();
    let relay = f.state.ui.clone();
    let relay2 = relay.clone();
    let handle = std::thread::spawn(move || {
        let mut user = RelayUserLink { relay: relay2 };
        let mut lp = PlanningLoop::new(FakePlan, PassFactory);
        lp.run(&mut user)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while relay.pending_count() == 0 {
        assert!(std::time::Instant::now() < deadline, "dialog 未挂起");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    // cancel 端点的解阻动作（handlers::cancel_task 同款）
    relay.abort_all();
    let result = handle.join().unwrap();
    assert!(result.is_err(), "dialog 挂起被 abort 后 SOP 应解阻收场（execute 层映射为 Failed）");
}
