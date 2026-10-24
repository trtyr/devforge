//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{crud::*, http::*, retry::*, start::*};

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


