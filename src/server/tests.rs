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

/// 测试引擎：立刻完成并记录调用；hang_ms>0 时模拟长 run（响应取消/超时干预）；
/// fail_system_times>0 时先抛 N 次基础设施故障（验证自动重启）；quality_fail 时直接抛质量失败。
struct MockEngine {
    calls: Arc<Mutex<Vec<String>>>,
    hang_ms: u64,
    fail_system_times: Arc<Mutex<u32>>,
    quality_fail: bool,
}
impl RunEngine for MockEngine {
    fn execute(&mut self, task: &Task, _run: &Run, control: &RunControl) -> Result<RunOutcome> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}/{}", task.project, task.id));
        if self.quality_fail {
            return Err(crate::error::DevforgeError::Quality(
                "终审未通过（route=finish）：文档与代码不一致".into(),
            ));
        }
        {
            let mut left = self.fail_system_times.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return Err(crate::error::DevforgeError::System(
                    "模拟上游 500（测试注入）".into(),
                ));
            }
        }
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
    fixture_full(hang_ms, timeout_ms, token, 0, false)
}

/// 可注入基础设施故障次数的夹具变体（续跑/自动重启单测用）。
fn fixture_with_failures(fail_system_times: u32) -> Fixture {
    fixture_full(0, None, None, fail_system_times, false)
}

/// 质量失败（审查打回/终审不通过）的夹具：不得被自动重启。
fn fixture_with_quality_failure() -> Fixture {
    fixture_full(0, None, None, 0, true)
}

fn fixture_full(
    hang_ms: u64,
    timeout_ms: Option<u64>,
    token: Option<&str>,
    fail_system_times: u32,
    quality_fail: bool,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    // 观测根注入到该夹具的临时目录（Q4 路径 ①）：走真实 start 路径的单测把心跳写在这里，
    // 用户 HOME 的 `.devforge/heartbeat/` 不被写脏。
    // 注入到**本实例**（不是进程级全局）→ 与其它并行单测不抢 root（R13 的落点漂移）。
    let obs_root = dir.path().join("observability");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let failures = Arc::new(Mutex::new(fail_system_times));
    let engine_factory = Box::new(move || -> Box<dyn RunEngine + Send> {
        Box::new(MockEngine {
            calls: calls2.clone(),
            hang_ms,
            fail_system_times: failures.clone(),
            quality_fail,
        })
    });
    let bus = std::sync::Arc::new(crate::server::EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut state = ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        engine_factory,
        bus,
        ui,
        std::sync::Arc::new(crate::config::installer::FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // 单测不真睡：重试退避归零
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .retry_policy = crate::engine::retry::RetryPolicy::fast();
    // 观测根钉在本夹具的临时目录（路由构建前注入，Arc 共享后不可再改）
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .obs_root = Some(obs_root);
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

async fn wait_settled(f: &Fixture, project: &str, task: &str) {
    for _ in 0..200 {
        let resp = send(
            &f.router,
            "GET",
            &format!("/api/v1/projects/{project}/tasks/{task}/runs"),
            None,
        )
        .await;
        let body = body_json(resp).await;
        // 等**最新**那个 run 进终态：列表按插入序（`store.list_runs`），最后一条才是刚起的那个。
        // 只看第一条会在「同一任务起第二次 run」时误判已终态 → 下一次 start 撞 409（串行槽）。
        let st = body["runs"]
            .as_array()
            .and_then(|runs| runs.last())
            .and_then(|r| r["status"].as_str())
            .unwrap_or("");
        if st == "Settled" || st == "Failed" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("run 未在上限内进入终态：{project}/{task}");
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
async fn start_sop_override_switches_task_for_cross_sop_chain() {
    // P005 跨 SOP 触发：同任务先规划后编码——start 带可选 sop 覆盖
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;

    // 覆盖为 coding → 202，任务 sop 已切换
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t1/start",
        Some(json!({"sop": "coding"})),
    )
    .await;
    assert_eq!(resp.status(), 202);
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await,
    )
    .await;
    assert_eq!(body["tasks"][0]["sop"], "coding");

    // F18：等第一个 run 落定再继续——后续断言管的是 sop 切换语义，不是串行槽竞态
    // （mock 引擎完成与下一个 start 是纯时序竞态，偶发 409 会淹没断言本体；
    // resume6h 基线取证：本用例在 20 连跑里仍复发 1 次）
    let mut settled = false;
    for _ in 0..200 {
        let body = body_json(
            send(&f.router, "GET", "/api/v1/projects/p/tasks/t1/runs", None).await,
        )
        .await;
        if body["runs"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
            && body["runs"][0]["status"] == "Settled"
        {
            settled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(settled, "第一个 run 应在超时前 Settled");

    // 未知 sop → 400（注册表校验，先于任何 run 状态检查——无竞态）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t1/start",
        Some(json!({"sop": "ghost"})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // 无 body 的 start → 沿用任务注册的 sop（不报错）
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn start_waits_at_the_render_gate_and_releases_when_the_render_finishes() {
    // 错实现形态：闸门不存在（start 立即开跑 → 门卫判「profile 未渲染」→ 基础设施故障盲重启）
    // → 本用例必须变红
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "coding"})),
    )
    .await;
    // 模拟「清单重渲染在途」：登记在途，暂不释放
    f.state.render_gate.begin_render();
    let waiting = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await;
    assert!(
        waiting.is_err(),
        "渲染在途时 start 不得立即开跑（应等闸门放行）"
    );
    // 渲染结束 → 闸门放行 → start 立即 202
    f.state.render_gate.end_render();
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn start_returns_immediately_when_the_required_profile_is_ready_even_if_a_render_is_in_flight() {
    // 错实现形态：闸门等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
    let f = fixture();
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
    }
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "coding"})),
    )
    .await;
    // 渲染仍在途，但本 run 所需 profile 已就绪 → 不等整轮渲染
    f.state.render_gate.begin_render();
    let resp = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("所需 profile 已就绪时 start 不得等整轮渲染");
    assert_eq!(resp.status(), 202);
    f.state.render_gate.end_render();
}

#[tokio::test]
async fn other_endpoints_are_served_while_a_start_waits_at_the_render_gate() {
    // 错实现形态：闸门等待用阻塞式等待占住 tokio worker（如 std::thread::sleep 轮询）
    // → 等待期内其它端点的请求被拖到闸门放开之后才返回 → 本用例必须变红
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "coding"})),
    )
    .await;

    // 渲染在途 → start 挂在闸门上；兜底线程 1.5s 后放行（阻塞式实现下也不会把用例挂死）
    f.state.render_gate.begin_render();
    let router = f.router.clone();
    let waiter = tokio::spawn(async move {
        send(&router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    let released_at: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(None));
    let released_at2 = released_at.clone();
    let state = f.state.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        *released_at2.lock().unwrap() = Some(std::time::Instant::now());
        state.render_gate.end_render();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await; // 让 start 确实挂到闸门上

    // 等待期内：同一服务器的其它端点必须照常被处理
    let resp = send(&f.router, "GET", "/api/v1/projects", None).await;
    let done_at = std::time::Instant::now();
    assert_eq!(resp.status(), 200);
    assert!(
        !waiter.is_finished(),
        "start 此刻应仍挂在闸门上（若已返回，本用例就没测到等待期）"
    );
    // 闸门若已被放开过，则本请求必须在放开之前就返回（阻塞式实现会拖到放开之后 → 红）
    let released = *released_at.lock().unwrap();
    if let Some(released) = released {
        assert!(
            done_at < released,
            "闸门等待期间其它端点必须照常被处理（该请求被拖到闸门放开之后才返回）"
        );
    }

    // 闸门放开 → start 正常 202（证明上面不是「start 早已返回」的假绿）
    let resp = waiter.await.unwrap();
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn two_concurrent_starts_while_the_gate_waits_run_at_most_one() {
    // 错实现形态：闸门把两个并发 start 都放行并各自开跑（双跑）→ run 数 > 1、引擎被调两次；
    // 或：输的那一路静默消失（既没开跑、也没 run.error 事件）→ 事件里查不到 run.error
    let f = fixture_with_engine(1500, None, None); // hang_ms=1500：胜出的 run 保持运行中
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "coding"})),
    )
    .await;

    // 渲染在途且所需 profile 未就绪 → 两个 start 都挂在闸门上（都过了 HTTP 层的串行槽快判）
    f.state.render_gate.begin_render();
    let r1 = f.router.clone();
    let r2 = f.router.clone();
    let a = tokio::spawn(async move {
        send(&r1, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    let b = tokio::spawn(async move {
        send(&r2, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 放行：所需 profile 就绪（门卫要过）+ 渲染结束 → 闸门放开
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
    }
    f.state.render_gate.end_render();

    // 两个 start 都被受理（202）：串行槽的判定在后台线程里，不在 HTTP 层
    assert_eq!(a.await.unwrap().status(), 202);
    assert_eq!(b.await.unwrap().status(), 202);

    // 输的那一路必须以 run.error 事件明确失败（不产生双跑）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let run_errors = loop {
        let n = f
            .state
            .bus
            .replay_after(0)
            .iter()
            .filter(|(_, data)| data.contains("\"type\":\"run.error\""))
            .count();
        if n > 0 || std::time::Instant::now() > deadline {
            break n;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(
        run_errors, 1,
        "并发 start 只能有一个开跑，另一个必须以 run.error 事件明确失败"
    );

    // 至多一个 run 真正开跑、引擎只被调用一次
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t1/runs", None).await,
    )
    .await;
    assert_eq!(
        body["runs"].as_array().map(|r| r.len()),
        Some(1),
        "只能有一个 run 真正开跑：{body}"
    );
    // 引擎只被调用一次：赢家的后台线程与输家的 run.error 事件是并发关系，
    // 事件先到不代表调用已发生（全量跑下偶发读到 0）→ 有界等待后再计数
    let calls = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let n = f.calls.lock().unwrap().clone();
            if !n.is_empty() || std::time::Instant::now() > deadline {
                break n;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    assert_eq!(calls.len(), 1, "引擎只能被调用一次：{calls:?}");

    wait_settled(&f, "p", "t1").await;
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

    // 未知 SOP → 400（P005：coding 已入注册表，改用 ghost）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "ghost"})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // P005：coding 已注册 → 201
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t-coding", "sop": "coding"})),
    )
    .await;
    assert_eq!(resp.status(), 201);

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
async fn start_doc_from_zero_sop_returns_202() {
    // P008 T006：新 SOP 注册后能被 API 触发（引擎 mock，立即完成）
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "d", "sop": "doc-from-zero"}))).await;

    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);

    let mut settled = false;
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/d/runs", None).await;
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
}

#[tokio::test]
async fn start_change_sop_returns_202() {
    // P009 接线：change 已注册，start 应受理（引擎是 mock，不等真 pi）
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "c", "sop": "change"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/c/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["sop"], "change");
    wait_settled(&f, "p", "c").await;
    let runs = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/c/runs", None).await,
    )
    .await;
    assert_eq!(runs["runs"][0]["status"], "Settled");
}

#[tokio::test]
async fn start_change_resumes_from_blackboard_and_fresh_clears_it() {
    // task-6：change 的 start 续跑/fresh 与 010 同一套语义（按 SOP 分文件）
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "c", "sop": "change"})),
    )
    .await;

    let key = crate::runstate::StateKey::new("p", "c", "change");
    let mut st = crate::runstate::RunState::new(
        &key,
        json!({"change": {"phase": "coding", "baseline_sha": "abc", "roundtrips": 1}}),
    );
    crate::runstate::save(&f.state.plans_root, &key, &mut st).unwrap();

    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/c/start", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], true, "有断点必须续跑：{body}");
    wait_settled(&f, "p", "c").await;

    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/c/start",
        Some(json!({"fresh": true})),
    )
    .await;
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "fresh 必须冷启动：{body}");
    assert!(
        !matches!(
            crate::runstate::load(&f.state.plans_root, &key),
            crate::runstate::Loaded::Found(_)
        ),
        "fresh 后 change 断点必须清空"
    );
}

#[tokio::test]
async fn start_reports_resumed_and_fresh_clears_only_that_sop() {
    // plan 010 T003：start 默认续跑、fresh 只清该 SOP 的断点
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "d", "sop": "doc-from-zero"})),
    )
    .await;

    // 1) 无断点 → 冷启动
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "无断点必须冷启动：{body}");
    wait_settled(&f, "p", "d").await;

    // 2) 种断点（doc-from-zero + 另一个 SOP）→ 默认续跑，且只认自己那份
    let key = crate::runstate::StateKey::new("p", "d", "doc-from-zero");
    let mut st = crate::runstate::RunState::new(&key, json!({"done": ["unit-a"]}));
    crate::runstate::save(&f.state.plans_root, &key, &mut st).unwrap();
    let other = crate::runstate::StateKey::new("p", "d", "planning");
    let mut st2 = crate::runstate::RunState::new(&other, json!({"done": ["x"]}));
    crate::runstate::save(&f.state.plans_root, &other, &mut st2).unwrap();

    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], true, "有断点默认续跑：{body}");
    assert_eq!(body["sop"], "doc-from-zero");
    wait_settled(&f, "p", "d").await;

    // 3) fresh → 回冷启动且断点被清；另一个 SOP 的断点不受影响
    // F18：status=Settled 与串行槽真正释放之间有收尾窗口（end_run 终态写等），
    // 偶发 409 会淹没断言本体 → 409 有界重试，其余状态照常断言
    let mut resp;
    loop {
        resp = send(
            &f.router,
            "POST",
            "/api/v1/projects/p/tasks/d/start",
            Some(json!({"fresh": true})),
        )
        .await;
        if resp.status() != 409 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "fresh 必须冷启动：{body}");
    assert!(
        body["note"].as_str().unwrap_or("").contains("fresh"),
        "fresh 原因要回显：{body}"
    );
    assert!(
        !matches!(
            crate::runstate::load(&f.state.plans_root, &key),
            crate::runstate::Loaded::Found(_)
        ),
        "fresh 后该 SOP 的 done 必须为空"
    );
    assert!(
        matches!(
            crate::runstate::load(&f.state.plans_root, &other),
            crate::runstate::Loaded::Found(_)
        ),
        "不得动其他 SOP 的断点"
    );
}

#[tokio::test]
async fn quality_failure_path_is_not_retried_end_to_end() {
    // 走真实 run 路径（不是直接喂给 map_intervention）：引擎报 Quality → 一次就 Failed，attempt 恒为 1
    let f = fixture_with_quality_failure();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "doc-from-zero"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 1, "质量失败不得自动重启");
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(body["runs"][0]["status"], "Failed");
    let key = crate::runstate::StateKey::new("p", "t", "doc-from-zero");
    let attempt = crate::runstate::load(&f.state.plans_root, &key)
        .state()
        .map(|s| s.attempt)
        .unwrap_or(0);
    assert_eq!(attempt, 0, "没有重启就不会写 attempt>0");
}

/// 源码守卫：协议违例上限 / 终审不通过不得再被当成基础设施故障。
#[test]
fn quality_failure_sites_do_not_use_system_error() {
    // 逐条比对旧文本片段，而不是泛搜「基础设施故障」——审查空回复属于真基础设施故障，保留 System 是对的
    let stale = [
        ("coding", include_str!("../sop/coding.rs"), "stage-verdict/next 块），判定为基础设施故障"),
        ("testing", include_str!("../sop/testing.rs"), "stage-verdict/next），判定为基础设施故障"),
        (
            "doc_from_zero",
            include_str!("../sop/doc/runner.rs"),
            "doc-verdict/route 块），判定为基础设施故障",
        ),
    ];
    for (name, src, needle) in stale {
        assert!(
            !src.contains(needle),
            "{name} 的协议违例上限仍走 System，会被自动重启"
        );
    }
    let doc_fix = include_str!("../sop/doc/fix.rs");
    let at = doc_fix
        .find("缺 stage-verdict），判定为")
        .expect("doc_fix 协议违例消息应存在");
    let near = &doc_fix[at..(at + 120).min(doc_fix.len())];
    assert!(near.contains("质量失败"), "doc_fix 应判质量失败：{near}");
    // D026：违例上限与实例失败上限的分层（迁到共享门后，这三个文件里不再有旧文案，
    // 守卫必须扩到新模块，否则「绿但没守任何东西」）。
    let gate = include_str!("../sop/review_gate.rs");
    for (anchor, needle, why) in [
        (
            "DevforgeError::Quality",
            "输出协议违例",
            "协议违例上限必须判 Quality（不自动重启）",
        ),
        (
            "DevforgeError::System",
            "判定为基础设施故障",
            "审查实例失败上限必须判 System（可重启）",
        ),
    ] {
        let at = gate.find(anchor).expect("共享门里应有对应的错误构造");
        let near = &gate[at..(at + 200).min(gate.len())];
        assert!(near.contains(needle), "{why}：{near}");
    }
    // ARC-R2：终审文案随 run_doc_from_zero 迁入 engine/doc_wiring.rs，守卫扫描面跟迁移走
    let doc_wiring = include_str!("../engine/doc_wiring.rs");
    for anchor in ["终审未通过", "终审输出协议违例"] {
        let at = doc_wiring
            .find(anchor)
            .unwrap_or_else(|| panic!("找不到 {anchor}"));
        let near = &doc_wiring[at.saturating_sub(200)..(at + 200).min(doc_wiring.len())];
        assert!(
            near.contains("DevforgeError::Quality"),
            "{anchor} 必须返回 Quality（不被重试），附近：{near}"
        );
    }
}

#[test]
fn quality_error_becomes_failed_and_is_not_retried() {
    // plan 010 T006/D004：质量失败→明确的 Failed（不进重启分支）；System 仍向上抛（可重启）
    let engine = crate::engine::builtin::BuiltinRpcEngine {
        profile_root: std::path::PathBuf::from("/tmp"),
        session_root: std::path::PathBuf::from("/tmp"),
        plans_root: std::path::PathBuf::from("/tmp"),
        ui: std::sync::Arc::new(UiRelay::new(std::sync::Arc::new(crate::server::EventBus::new()))),
        codegraph_bin: std::path::PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let control = RunControl::new();
    let outcome = engine
        .map_intervention(
            &control,
            "doc-engine",
            crate::error::DevforgeError::Quality("审查连续 4 轮输出协议违例".into()),
        )
        .expect("质量失败不得作为错误上抛");
    match outcome {
        RunOutcome::Failed { reason } => assert!(reason.contains("违例"), "{reason}"),
        other => panic!("应为 Failed，实际 {other:?}"),
    }

    let infra = engine.map_intervention(
        &control,
        "doc-engine",
        crate::error::DevforgeError::System("上游 500".into()),
    );
    assert!(infra.is_err(), "系统错误必须上抛给重试层");
    assert!(
        crate::engine::retry::is_infra_failure(&infra),
        "System 才是可重启故障"
    );
}

#[tokio::test]
async fn infra_failures_are_retried_up_to_three_times() {
    // plan 010 T005：前两次 System → 第三次成功，run 终态 Settled，attempt 落盘为 3
    let f = fixture_with_failures(2);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 3, "两次故障 + 一次成功");
    let key = crate::runstate::StateKey::new("p", "t", "planning");
    let st = crate::runstate::load(&f.state.plans_root, &key);
    assert_eq!(
        st.state().map(|s| s.attempt),
        Some(3),
        "attempt 必须落盘（续跑审计）"
    );
}

#[tokio::test]
async fn persistent_infra_failures_fail_after_four_attempts() {
    // 不无限：连续故障只试 4 次（首次 + 3 重试），终态 Failed
    let f = fixture_with_failures(10);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 4, "首次 + 3 次重试，不得无限重跑");
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(body["runs"][0]["status"], "Failed");
    let key = crate::runstate::StateKey::new("p", "t", "planning");
    let st = crate::runstate::load(&f.state.plans_root, &key);
    assert_eq!(st.state().map(|s| s.attempt), Some(4));
}

/// 验收（计划 §5）· 「先记错误、再决定重启」：第 1 次基础设施故障的 ERROR 条目必须出现在
/// 「基础设施故障，自动重启 run」WARN **之前**，且条目带 `attempt=1`。
///
/// 错实现形态：把重试层的逐次落盘挪到终态（只在尝试耗尽后落一条）→ 现场只剩「重启了」，
/// 看不出「为什么重启」→ 本用例必须变红。
///
/// 为什么要跑**真实 run 路径**且**跑在本线程**：「自动重启 run」这条 WARN 只在服务端落
/// （`src/server/mod.rs`），ERROR 条目只在重试层落（`src/engine/retry.rs`）——两处落点必须
/// 出现在**同一条日志流**里才有先后可言；而捕获器按线程分流（`observability::test_support`），
/// 经 HTTP `start` 的那条路会把 run 扔进别的线程 → 两处落点就散在不同缓冲里，看不到顺序。
#[tokio::test]
async fn the_infra_error_is_logged_before_the_restart_warning() {
    let f = fixture_with_failures(1);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    // 引擎：第 1 次抛基础设施故障，第 2 次成功 → 必然发生一次「自动重启」
    let engine: crate::server::EngineBox = Box::new(MockEngine {
        calls: Arc::new(Mutex::new(Vec::new())),
        hang_ms: 0,
        fail_system_times: Arc::new(Mutex::new(1)),
        quality_fail: false,
    });
    let text = crate::observability::test_support::capture_jsonl(|| {
        crate::server::run_in_background(f.state.clone(), "p".into(), "t".into(), engine);
    });
    let lines: Vec<&str> = text.lines().collect();
    let err_at = lines
        .iter()
        .position(|l| l.contains("run.attempt_failed") && l.contains("\"attempt\":1"))
        .unwrap_or_else(|| panic!("第 1 次失败应落一条 ERROR 条目：{text}"));
    let restart_at = lines
        .iter()
        .position(|l| l.contains("自动重启 run"))
        .unwrap_or_else(|| panic!("基础设施故障应触发自动重启：{text}"));
    assert!(
        err_at < restart_at,
        "错误条目必须先于「自动重启 run」落盘（先记错误、再决定重启）：{text}"
    );
    let err_line = lines[err_at];
    assert!(err_line.contains("\"level\":\"ERROR\""), "非质量类失败落 ERROR：{err_line}");
    assert!(err_line.contains("\"component\":\"run-retry\""), "{err_line}");
    let restart_line = lines[restart_at];
    assert!(restart_line.contains("\"level\":\"WARN\""), "{restart_line}");
    assert!(
        restart_line.contains("\"attempt\":2"),
        "重启的是第 2 次尝试：{restart_line}"
    );
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
async fn the_run_writes_a_heartbeat_into_the_fixtures_dir_and_keeps_it() {
    // 错实现形态：①run 不落心跳（观测根没注入 / run 上下文没置位）→ 死亡现场无证据
    // ②run 结束把心跳删掉 → 「最后状态」丢失
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

    // 心跳落在本夹具的观测根下（用户 HOME 不被写脏），且 run 结束后**保留**
    let dir = f._dir.path().join("observability").join("heartbeat");
    let entry = std::fs::read_dir(&dir)
        .expect("夹具的观测根下应有心跳目录")
        .filter_map(|e| e.ok())
        .next()
        .expect("夹具的观测根下应有心跳文件");
    let text = std::fs::read_to_string(entry.path()).unwrap();
    let hb: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(hb["phase"], "plan", "心跳的阶段 = 该 SOP 的阶段词汇：{text}");
    assert!(hb["timestamp_ms"].as_u64().unwrap_or(0) > 0, "心跳要带时间戳：{text}");
    assert!(!hb["run_id"].as_str().unwrap_or("").is_empty(), "心跳要带 run id：{text}");
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
    let mut next = ConfigManifest::builtin();
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
    let mut next2 = ConfigManifest::builtin();
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
    let mut next = ConfigManifest::builtin();
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

#[tokio::test]
async fn run_detail_aggregates_observability() {
    // DR-7：纯 API 回答「在干嘛/干到哪/卡没卡」——详情端点聚合 run/心跳/agent 活性/进度
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    let mut run_id = String::new();
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        let body = body_json(resp).await;
        if let Some(id) = body["runs"].as_array().and_then(|a| a.first()).and_then(|r| r["id"].as_str()) {
            run_id = id.to_string();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!run_id.is_empty(), "应有 run");

    let resp = send(&f.router, "GET", &format!("/api/v1/projects/p/tasks/t/runs/{run_id}"), None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["run"]["id"], run_id.as_str(), "run id 要回显: {body}");
    assert!(
        body["heartbeat"].is_object() || body["heartbeat"].is_null(),
        "心跳聚合位要在: {body}"
    );
    assert!(
        body["agent"]["stall_warn_ms"].as_u64().unwrap_or(0) > 0,
        "agent 活性聚合位要在: {body}"
    );
    assert!(body["progress"].is_null(), "planning sop 无进度段: {body}");
    // 未知 run → 404
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs/nope", None).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn run_events_endpoint_returns_paged_log() {
    // DR-7：历史事件流水端点——文件只是实现细节，API 是主入口；日志缺席也不得 5xx
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs/nope/events", None).await;
    assert_eq!(resp.status(), 404, "未知 run 必须归属校验");
    let mut run_id = String::new();
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        let body = body_json(resp).await;
        if let Some(id) = body["runs"].as_array().and_then(|a| a.first()).and_then(|r| r["id"].as_str()) {
            run_id = id.to_string();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let resp = send(
        &f.router,
        "GET",
        &format!("/api/v1/projects/p/tasks/t/runs/{run_id}/events?limit=10"),
        None,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["limit"], 10, "limit 回显: {body}");
    assert!(body["total"].as_u64().is_some(), "total 要在: {body}");
    assert!(body["events"].is_array(), "events 数组要在: {body}");
}

#[tokio::test]
async fn run_transcript_endpoint_returns_paged_entries() {
    // DR-9 L4：会话转录纯 API 可查——文件是存储，API 是唯一入口；无文件也 200 空列表
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs/nope/transcript", None).await;
    assert_eq!(resp.status(), 404, "未知 run 必须归属校验");
    let mut run_id = String::new();
    for _ in 0..100 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        let body = body_json(resp).await;
        if let Some(id) = body["runs"].as_array().and_then(|a| a.first()).and_then(|r| r["id"].as_str()) {
            run_id = id.to_string();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let resp = send(
        &f.router,
        "GET",
        &format!("/api/v1/projects/p/tasks/t/runs/{run_id}/transcript?limit=5"),
        None,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["limit"], 5, "limit 回显: {body}");
    assert!(body["total"].as_u64().is_some(), "total 要在: {body}");
    assert!(body["entries"].is_array(), "entries 数组要在: {body}");
    assert!(body["files"].is_array(), "files 数组要在: {body}");
}

#[test]
fn stall_check_returns_age_even_without_agent_events() {
    // DR-3：从未有 agent 事件（DR-1 型假活）也要能给出年龄——按开跑时刻起算，不返回 None
    let dir = tempfile::tempdir().unwrap();
    crate::observability::begin_run_in(dir.path().to_path_buf(), "stall-probe", "doc");
    let age = crate::observability::stall_check("stall-probe");
    assert!(
        age.is_some(),
        "无 agent 事件时必须按开跑时刻起算，不得 None"
    );
    crate::observability::end_run("stall-probe");
}
