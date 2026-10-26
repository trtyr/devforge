//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{crud::*, http::*, observe::*, retry::*};

#[tokio::test]
async fn rejected_fresh_start_does_not_wipe_the_checkpoint() {
    // server #1 补正回归：fresh 会删该 SOP 的断点——顺序必须是「准入 → start_gate」，
    // 否则一次被拒的 start 就把断点删了（现场丢失，且调用方还以为只是冲突）。
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-fresh"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "change"})),
    )
    .await;

    // 占满串行槽（同时把任务置为 Running → 后续准入必拒）
    {
        let mut store = f.state.store.lock().expect("store 锁");
        store.max_concurrent_runs = 1;
        store.begin_run("p", "t1").unwrap();
    }
    // 预置一份断点（fresh 会删它——但要在准入通过之后才允许删）
    let state_path = f.state.plans_root.join("p/t1/change/state.json");
    std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    std::fs::write(
        &state_path,
        r#"{"schema_version":1,"sop":"change","project":"p","task":"t1","updated_at_ms":1,"attempt":0,"data":{}}"#,
    )
    .unwrap();

    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t1/start",
        Some(json!({"fresh": true})),
    )
    .await;
    assert_eq!(resp.status(), 409, "并发槽已满 → 准入必须当场拒");
    assert!(
        state_path.is_file(),
        "被拒的 start 不得先删断点（fresh 清断点必须在准入之后）"
    );
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
    // 自审 major：只认 Settled 的等待是竞态源——终态有 Settled/Failed 两种，
    // 本用例断言的是 sop 切换语义，任一终态都解除串行槽占用。
    let mut settled = false;
    for _ in 0..200 {
        let body = body_json(
            send(&f.router, "GET", "/api/v1/projects/p/tasks/t1/runs", None).await,
        )
        .await;
        if body["runs"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
            && (body["runs"][0]["status"] == "Settled" || body["runs"][0]["status"] == "Failed")
        {
            settled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(settled, "第一个 run 应在超时前落终态");

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
    // 自审 major：收尾窗口（终态已写、串行槽未放）仍可能 409——同款有界重试，
    // 200 次 × 20ms = 4s 上限，超限失败而非挂死。
    let mut resp = None;
    for _ in 0..200 {
        let r = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
        if r.status() != 409 {
            resp = Some(r);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let resp = resp.expect("收尾 start 在 4s 内持续 409——串行槽未释放");
    assert_eq!(resp.status(), 202);
}

/// 自审 major 回归：引擎审计修复的排序不变量承诺「准入被拒 → 回滚任务身份」
/// （sop 覆盖先落、准入后行）——此前零覆盖。长 run 占住串行槽 → 带 sop 覆盖的
/// start 必须 409，且任务的 sop 保持原值（不得先切后拒）。
#[tokio::test]
async fn rejected_start_with_sop_override_restores_the_previous_sop() {
    let f = fixture_with_engine(60_000, None, None); // 长 run 稳占串行槽
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
        Some(json!({"id": "t0", "sop": "planning"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;

    // t0 占槽（hang 60s，收尾前 cancel 释放）
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t0/start", None).await;
    assert_eq!(resp.status(), 202, "t0 应正常开跑占槽");

    // t1 带 sop 覆盖 → 槽满 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t1/start",
        Some(json!({"sop": "coding"})),
    )
    .await;
    assert_eq!(resp.status(), 409, "串行槽被占必须拒");

    // 关键断言：被拒的 sop 覆盖必须已回滚——t1 仍是 planning
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks", None).await,
    )
    .await;
    let t1 = body["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "t1")
        .expect("t1 应在列表");
    assert_eq!(
        t1["sop"], "planning",
        "被拒的 start 不得留下 sop 覆盖（先切后拒=身份被污染）：{t1}"
    );

    // 清理：cancel 释放长 run（hang 引擎响应取消）
    // 自审纠偏：control 在 run 线程内登记，202 返回与登记之间有窗口——有界重试。
    let mut cancelled = false;
    for _ in 0..100 {
        let resp = send(
            &f.router,
            "POST",
            "/api/v1/projects/p/tasks/t0/cancel",
            None,
        )
        .await;
        if resp.status() == 200 {
            cancelled = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(cancelled, "t0 应在 2s 内可取消（control 登记）");
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
    // 自审 major：先等 run 落定再计数——旧口径在落定前取样，3s 内读到 1 次
    // 也可能是双跑的第二笔还没落账（有界等待只保证读到 ≥1）。落定后计数是
    // 终局事实。
    wait_settled(&f, "p", "t1").await;
    let calls = f.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "引擎只能被调用一次：{calls:?}");
}


