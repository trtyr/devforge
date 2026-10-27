//! 引擎 panic 兜底（全仓审计 #1）。
//!
//! 错实现形态：`run_in_background` 裸调 engine.execute，SOP 里一次 panic 展开 run 线程 →
//! `finish_run` 不执行 → run 恒为 Running → 串行槽位（`running_run_count`）永久占死，
//! 只能重启进程。

#[allow(unused_imports)]
use super::*;

/// panic 必须落到 Failed 终态，且不占死串行槽位（下一次 start 仍能进）。
#[tokio::test]
async fn engine_panic_ends_run_failed_and_frees_the_slot() {
    // 首次 panic，第二次正常完成——「槽位没卡死」的机械证据就是第二次能进
    let f = fixture_with_panic(1);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;

    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);

    wait_settled(&f, "p", "t").await;
    let body = body_json(send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await).await;
    let runs = body["runs"].as_array().expect("runs 应为数组");
    let last = runs.last().expect("至少一个 run");
    assert_eq!(
        last["status"], "failed",
        "panic 应被兜底成 Failed 终态（而不是永远 Running）：{last}"
    );

    // 槽位未被占用：再起一次能进（错实现：409/500 已到并发上限）
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(
        resp.status(),
        202,
        "panic 兜底后串行槽位应已释放（否则任务永久卡死）"
    );
    // 第二次 run 的 run 记录由后台线程创建，轮询到引擎真的被再调一次为止
    // （直接断言会有竞态：start 返回 202 时新 run 可能尚未落库）
    let mut ran_twice = false;
    for _ in 0..200 {
        if f.calls.lock().unwrap().len() >= 2 {
            ran_twice = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(ran_twice, "第二次 start 应真的跑到引擎（槽位未释放）");
    wait_settled(&f, "p", "t").await;
}

/// 自审 critical 回归：看门狗干预路径（取消/超时/失活收割）必须解阻待答——
/// run 线程阻塞在 ask_blocking 的 recv() 上时，只杀 pi 子进程解不了它；钩子丢待答
/// 后 run 才能落终态释放串行槽（否则所有后续 start 一律 409）。
#[test]
fn watchdog_intervention_unblocks_pending_ask() {
    let bus = std::sync::Arc::new(crate::server::EventBus::new());
    let relay = std::sync::Arc::new(UiRelay::new(bus));
    let control = RunControl::new();
    control.cancel();

    // 在 run 上下文里发问（生产路径：引擎线程在 run 内，owner 才归 abort_run 管）；
    // run 上下文是 thread_local，必须在 asker 线程内置位。
    let run_id = "unblock-reg".to_string();
    let obs_root = std::env::temp_dir().join(format!("devforge-unblock-reg-{run_id}"));
    let asker = {
        let relay = relay.clone();
        let run_id = run_id.clone();
        let obs_root = obs_root.clone();
        std::thread::spawn(move || {
            crate::observability::begin_run_in_deferred(obs_root, &run_id, "plan");
            let r = relay.ask_blocking("继续吗", &["是", "否"]);
            crate::observability::end_run(&run_id);
            r
        })
    };
    // 等 ask 登记进 pending（轮询，不固定 sleep）
    for _ in 0..200 {
        if relay.pending_count() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(relay.pending_count(), 1, "ask 应已登记");

    let hook: crate::observability::watchdog::UnblockHook = {
        let relay = relay.clone();
        std::sync::Arc::new(move |rid: &str| {
            relay.abort_run(rid);
        })
    };
    let start = std::time::Instant::now();
    let _out = crate::observability::watchdog::run_with_watchdog(
        &control,
        run_id.into(),
        None,
        vec![],
        None,
        Some(hook),
        || std::thread::sleep(std::time::Duration::from_millis(1500)),
    );
    let answer = asker.join().expect("ask 线程不得挂死");
    assert!(answer.is_err(), "干预必须解阻待答（recv 返回 Err）");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "run 线程必须及时收场，实际 {:?}",
        start.elapsed()
    );
    assert_eq!(relay.pending_count(), 0, "待答条目应被清掉");
}

/// 自审 critical 回归：ask_blocking 的显式墙钟——无人应答时按超时解阻，待答条目
/// 被清掉（迟到应答拿到干净的「无此待答」，不会发进虚空）。
#[test]
fn ask_blocking_timeout_expires_and_cleans_the_pending_entry() {
    let bus = std::sync::Arc::new(crate::server::EventBus::new());
    let relay = UiRelay::new(bus);
    let start = std::time::Instant::now();
    let out = relay.ask_blocking_with_timeout(
        "没人应答的问题",
        &["a"],
        Some(std::time::Duration::from_millis(80)),
    );
    assert!(out.is_err(), "超时应返回 Err");
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(70),
        "应真的等满超时窗，实际 {:?}",
        start.elapsed()
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(relay.pending_count(), 0, "超时后待答条目应被清掉");
}
