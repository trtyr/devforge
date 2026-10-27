//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;

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
            && graph["status"] == "completed"
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

    // 自审 minor（223）回归：干预面必须**在 202 之前**登记——紧跟 start 的第一次 cancel
    // 就得命中（旧口径 control 由 run 线程登记，这里只能轮询，且真实客户端会吃 404）。
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/cancel", None).await;
    assert_eq!(
        resp.status(),
        200,
        "202 之后立刻 cancel 必须命中（不得有登记窗口）"
    );

    // 引擎响应取消 → run Failed(已取消)，任务 Failed，不留悬空 Running
    let mut final_runs = Value::Null;
    for _ in 0..200 {
        let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
        final_runs = body_json(resp).await;
        if final_runs["runs"][0]["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(final_runs["runs"][0]["status"], "failed");
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["tasks"][0]["status"], "failed");
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
        if final_runs["runs"][0]["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(final_runs["runs"][0]["status"], "failed");
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["tasks"][0]["status"], "failed");
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
    use crate::server::RelayUserLink;
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
    // 自审 minor（208）回归：详情/列表响应不得回显主机绝对路径（目录布局不入口径）——
    // 逐字段扫字符串值，任何以 / 开头或含观测根/台账根绝对路径的都算泄露。
    let plans_root = f.state.plans_root.display().to_string();
    fn scan(v: &Value, plans_root: &str, out: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                if s.starts_with('/') || (!plans_root.is_empty() && s.contains(plans_root)) {
                    out.push(s.clone());
                }
            }
            Value::Array(a) => a.iter().for_each(|x| scan(x, plans_root, out)),
            Value::Object(o) => o.values().for_each(|x| scan(x, plans_root, out)),
            _ => {}
        }
    }
    let mut leaks = Vec::new();
    scan(&body, &plans_root, &mut leaks);
    assert!(leaks.is_empty(), "run 详情不得回显主机绝对路径：{leaks:?}");
    // 列表端点同样收口
    let resp = send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await;
    let list = body_json(resp).await;
    let mut leaks = Vec::new();
    scan(&list, &plans_root, &mut leaks);
    assert!(leaks.is_empty(), "run 列表不得回显主机绝对路径：{leaks:?}");
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

/// 自审 minor（221）：`abort_run`（按 run 精确解阻，server #3 并发隔离的关键行为）此前
/// 全仓零测试——只覆盖了 abort_all 的全局解阻。这里用真 run 上下文钉「只解自己那个 run」。
#[tokio::test]
async fn ui_abort_run_unblocks_only_its_own_run() {
    let f = fixture();
    let ui = f.state.ui.clone();
    let obs_a = tempfile::tempdir().unwrap();
    let obs_b = tempfile::tempdir().unwrap();
    let (root_a, root_b) = (obs_a.path().to_path_buf(), obs_b.path().to_path_buf());

    // 两个线程各在自己的 run 上下文里挂一个 dialog → 待答条目带上归属
    let ui_a = ui.clone();
    let a = std::thread::spawn(move || {
        crate::observability::begin_run_in(root_a, "run-a", "plan");
        let r = ui_a.ask_blocking("run-a 的提问", &["完成"]);
        crate::observability::end_run("run-a");
        r
    });
    let ui_b = ui.clone();
    let b = std::thread::spawn(move || {
        crate::observability::begin_run_in(root_b, "run-b", "plan");
        let r = ui_b.ask_blocking("run-b 的提问", &["完成"]);
        crate::observability::end_run("run-b");
        r
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while ui.pending_count() < 2 {
        assert!(std::time::Instant::now() < deadline, "两个 dialog 未挂起");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // 别的 run 不得误伤
    assert_eq!(ui.abort_run("run-other"), 0, "别的 run 不得解阻");
    assert_eq!(ui.pending_count(), 2, "两条待答都应原样保留");

    // 自己的 run 必须解阻，且只解自己那条
    assert_eq!(ui.abort_run("run-a"), 1, "run-a 的待答必须被解阻");
    let ra = a.join().unwrap();
    assert!(ra.is_err(), "被解阻的 ask 必须失败返回");
    assert_eq!(ui.pending_count(), 1, "只剩 run-b 那条（并发隔离）");

    let _ = ui.abort_all();
    let _ = b.join();
}
