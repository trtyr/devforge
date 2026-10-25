//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{crud::*, http::*, observe::*, retry::*};

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

    // 准入在响应之前（server #1 修复后）：只能有一路拿 202，另一路当场 409——
    // 旧契约「两个 start 都回 202，串行槽判定在后台线程」正是被审出来的缺陷：
    // 调用方永远不知道自己的 run 没起来。
    let s1 = a.await.unwrap().status().as_u16();
    let s2 = b.await.unwrap().status().as_u16();
    let mut got = vec![s1, s2];
    got.sort_unstable();
    assert_eq!(
        got,
        vec![202, 409],
        "并发 start 只能有一路拿到 202（另一路 409 冲突），实际：{s1}/{s2}"
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


