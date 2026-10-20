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
        let st = body["runs"][0]["status"].as_str().unwrap_or("");
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
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/d/start",
        Some(json!({"fresh": true})),
    )
    .await;
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
            include_str!("../sop/doc_from_zero.rs"),
            "doc-verdict/route 块），判定为基础设施故障",
        ),
    ];
    for (name, src, needle) in stale {
        assert!(
            !src.contains(needle),
            "{name} 的协议违例上限仍走 System，会被自动重启"
        );
    }
    let doc_fix = include_str!("../sop/doc_fix.rs");
    let at = doc_fix
        .find("缺 stage-verdict），判定为")
        .expect("doc_fix 协议违例消息应存在");
    let near = &doc_fix[at..(at + 120).min(doc_fix.len())];
    assert!(near.contains("质量失败"), "doc_fix 应判质量失败：{near}");
    let server = include_str!("mod.rs");
    for anchor in ["终审未通过", "终审输出协议违例"] {
        let at = server.find(anchor).unwrap_or_else(|| panic!("找不到 {anchor}"));
        let near = &server[at.saturating_sub(200)..(at + 200).min(server.len())];
        assert!(
            near.contains("DevforgeError::Quality"),
            "{anchor} 必须返回 Quality（不被重试），附近：{near}"
        );
    }
}

#[test]
fn quality_error_becomes_failed_and_is_not_retried() {
    // plan 010 T006/D004：质量失败→明确的 Failed（不进重启分支）；System 仍向上抛（可重启）
    let engine = super::BuiltinRpcEngine {
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
