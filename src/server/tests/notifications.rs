//! 通知端点单测（A2 · 通知系统）：列表/未读数/单条已读/全部已读。
//!
//! 独立成文件（ARC-R6 拆分口径）：本文件是「通知 API」主题。
//! 通知落库的域层语义（五类事件映射/裁剪/持久化）在 domain::store_tests。

#[allow(unused_imports)]
use super::*;

use crate::server::SharedState;
use crate::server::scheduler::{SchedulerConfig, tick as scheduler_tick};
use super::channel_breaker::{fail_run, make_task};

fn push_two(st: &SharedState) {
    crate::server::notify(st, "agent_failed", "error", "run 失败：t", Some("devforge/t"));
    crate::server::notify(st, "ui_pending", "warn", "agent 等裁决", None);
}

#[tokio::test]
async fn notifications_list_reports_items_and_unread_count() {
    let f = fixture();
    push_two(&f.state);
    let resp = send(&f.router, "GET", "/api/v1/notifications", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["unread"], 2, "两条未读：{body}");
    let items = body["items"].as_array().expect("items 应为数组");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["kind"], "agent_failed");
    assert_eq!(items[0]["read"], false);
    assert_eq!(items[1]["title"], "agent 等裁决");
}

#[tokio::test]
async fn notifications_unread_filter_returns_only_unread() {
    let f = fixture();
    push_two(&f.state);
    {
        let mut store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        store.mark_notification_read(1);
    } // guard 在 await 前丢弃（clippy: MutexGuard held across await）
    let resp = send(&f.router, "GET", "/api/v1/notifications?unread=true", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["unread"], 1);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "unread=true 只返回未读：{body}");
    assert_eq!(items[0]["id"], 2);
}

#[tokio::test]
async fn notifications_mark_read_by_ids_is_idempotent() {
    let f = fixture();
    push_two(&f.state);
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/notifications/read",
        Some(json!({"ids": [1, 1, 999]})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["marked"], 1, "重复 id 与不存在的 id 不计数：{body}");
    assert_eq!(body["unread"], 1);
    // 再标一次：已读不重复计数（幂等）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/notifications/read",
        Some(json!({"ids": [1]})),
    )
    .await;
    let body = body_json(resp).await;
    assert_eq!(body["marked"], 0);
}

#[tokio::test]
async fn notifications_mark_all_reads_everything() {
    let f = fixture();
    push_two(&f.state);
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/notifications/read",
        Some(json!({"all": true})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["marked"], 2);
    assert_eq!(body["unread"], 0);
    // 列表确认全已读
    let resp = send(&f.router, "GET", "/api/v1/notifications?unread=true", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

// ---------- ntask-3+：真实事件源接线（审计驳回补线） ----------

/// 渠道熔断命中 → 通知落库（kind=channel_breaker，error 级）且去重（同一失败 run
/// 连 tick 三次也只一条）。
#[tokio::test]
async fn a_tripped_breaker_lands_a_notification_once() {
    let f = fixture();
    make_task(&f, "devforge", "chan-notify", "change");
    fail_run(
        &f,
        "devforge",
        "chan-notify",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    scheduler_tick(&f.state, &SchedulerConfig::default());
    scheduler_tick(&f.state, &SchedulerConfig::default());
    {
        let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            store.notifications.len(),
            1,
            "熔断通知恰一条（不随 tick 刷屏）：{:?}",
            store.notifications
        );
        let n = &store.notifications[0];
        assert_eq!(n.kind, "channel_breaker");
        assert_eq!(n.level, "error");
        assert!(n.title.contains("deepseek-flash"), "{n:?}");
        assert_eq!(n.link.as_deref(), Some("devforge/chan-notify"));
        assert!(!n.read);
    }
    // 总线也有 notification.created（铃铛订阅源）
    let events = f.state.bus.replay_after(0);
    assert!(
        events
            .iter()
            .any(|(_, v)| v.contains("notification.created")),
        "总线应有通知事件"
    );
}

/// ui 待答久等 → 通知（阈值走 DEVFORGE_UI_PENDING_WARN_MS 旋钮，共享锁防串味）；
/// 同一待答只通知一次。
#[tokio::test]
async fn pending_ui_scan_notifies_once_per_ask_after_threshold() {
    let f = fixture();
    let _guard = crate::server::ui_warn_env_lock();
    let prev = std::env::var("DEVFORGE_UI_PENDING_WARN_MS").ok();
    // SAFETY: 已持 ui_warn_env_lock（进程级独占该键）。
    unsafe {
        std::env::set_var("DEVFORGE_UI_PENDING_WARN_MS", "50");
    }

    let ui = f.state.ui.clone();
    let handle = std::thread::spawn(move || {
        ui.ask_blocking_with_timeout("等裁决的问题", &["选项A"], Some(std::time::Duration::from_secs(30)))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while f.state.ui.pending_count() < 1 {
        assert!(std::time::Instant::now() < deadline, "待答未登记");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(120)); // 年龄超过阈值

    crate::server::scheduler::notify_pending_uis(&f.state);
    {
        let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(store.notifications.len(), 1, "久等待答应产生一条通知");
        assert_eq!(store.notifications[0].kind, "ui_pending");
        assert_eq!(store.notifications[0].level, "warn");
    }
    // 再扫：同一待答去重
    crate::server::scheduler::notify_pending_uis(&f.state);
    {
        let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(store.notifications.len(), 1, "同一待答不重复通知");
    }

    // 收尾：应答解锁线程 + 恢复 env
    let id = f.state.ui.pending_snapshot()[0].id.clone();
    let _ = f.state.ui.answer(&id, "选项A");
    let _ = handle.join().unwrap();
    // SAFETY: 仍持锁。
    unsafe {
        match prev {
            Some(v) => std::env::set_var("DEVFORGE_UI_PENDING_WARN_MS", v),
            None => std::env::remove_var("DEVFORGE_UI_PENDING_WARN_MS"),
        }
    }
}

/// 集成队列 NeedsRepair → 通知（队列文件是事实源：不管谁改的状态，tick 都看得见）；
/// 同一候选只通知一次。
#[tokio::test]
async fn integration_repair_scan_notifies_once_per_candidate() {
    let f = fixture();
    // 扫描按 store.projects 遍历——先建项目行
    {
        let mut store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .create_project(crate::domain::Project::new(
                "devforge",
                f.state.plans_root.parent().unwrap().join("repo-int"),
                crate::domain::now_ms(),
            ))
            .unwrap();
    }
    // 写一条 NeedsRepair 候选进队列文件
    let mut q =
        crate::integration::MergeQueue::load(&f.state.plans_root, "devforge").unwrap();
    q.enqueue(crate::integration::Candidate {
        task_id: "task-bad".into(),
        frozen_sha: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
        base_rev: "base".into(),
        owned_paths: vec![],
        status: crate::integration::CandidateStatus::NeedsRepair {
            reason: "组合验证失败：模拟".into(),
        },
        evidence_paths: vec![],
    });
    q.save().unwrap();

    crate::server::scheduler::notify_integration_repairs(&f.state);
    {
        let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(store.notifications.len(), 1, "NeedsRepair 应产生一条通知");
        let n = &store.notifications[0];
        assert_eq!(n.kind, "integration_needs_repair");
        assert_eq!(n.level, "warn");
        assert!(n.title.contains("task-bad"), "{n:?}");
        assert_eq!(n.link.as_deref(), Some("devforge/task-bad"));
    }
    // 再扫：同一候选去重
    crate::server::scheduler::notify_integration_repairs(&f.state);
    let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(store.notifications.len(), 1, "同一候选不重复通知");
}

// ---------- ntask-3：通知 → SSE 总线 ----------

/// 通知产生时事件总线必须收到 notification.created（前端铃铛的订阅源）。
/// 错实现形态：只落库不广播（deaf）或只广播不落库（重启即蒸发）。
#[tokio::test]
async fn notification_creation_emits_bus_event() {
    let f = fixture();
    // 先发一条哨兵事件：证明总线本身活着，同时拿到真实基线 id。
    // （fixture 刚建时 last_id 可能为 0，若 push_notification 的 emit 完全没发生，
    // replay_after(0) 也会是空——哨兵把「总线没走」与「emit 没发生」两种失败分开。）
    f.state.emit(json!({"type": "sentinel"}));
    let before = f.state.bus.last_id();
    assert!(before > 0, "哨兵事件后 last_id 应 > 0");
    push_two(&f.state);
    let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(store.notifications.len(), 2, "落库必须先成立（deaf 检测的前置）");
    drop(store);
    let events = f.state.bus.replay_after(before);
    let created: Vec<_> = events
        .iter()
        .filter(|(_, v)| v.contains("notification.created"))
        .collect();
    assert_eq!(created.len(), 2, "两次 push 应各发一条总线事件：{events:?}");
    let (_, v) = created[0];
    let parsed: serde_json::Value = serde_json::from_str(v).unwrap();
    assert_eq!(parsed["type"], "notification.created");
    assert_eq!(parsed["id"], 1);
    assert_eq!(parsed["kind"], "agent_failed");
    assert_eq!(parsed["level"], "error");
    assert_eq!(parsed["link"], "devforge/t");
}

/// run 终态 → 通知：settled 发 info、失败发 error/warn（质量/系统分级）。
/// 直接调 notify 的落库+广播面，run_in_background 的完整链路归端到端（不在本层）。
#[tokio::test]
async fn run_lifecycle_notify_lands_and_broadcasts() {
    let f = fixture();
    let before = f.state.bus.last_id();
    crate::server::notify(
        &f.state,
        "run_settled",
        "info",
        "p/t settled（1 次尝试）",
        Some("p/t"),
    );
    crate::server::notify(
        &f.state,
        "agent_failed",
        "error",
        "p/t2 run 失败（2 次尝试）：质量失败：回归红",
        Some("p/t2"),
    );
    // 落库
    let store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(store.notifications.len(), 2);
    assert_eq!(store.unread_notification_count(), 2);
    drop(store);
    // 广播
    let events = f.state.bus.replay_after(before);
    let created: Vec<_> = events
        .iter()
        .filter(|(_, v)| v.contains("notification.created"))
        .collect();
    assert_eq!(created.len(), 2);
}
