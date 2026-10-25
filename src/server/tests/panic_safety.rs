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
        last["status"], "Failed",
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
