//! 事件总线回归（2026-10-27 自审 minor 77/78/157/158）。

use super::*;

/// 序号与留档顺序必须一致：并发 emit 之后，replay 出来的 id 必须严格递增
/// （旧口径序号在锁外分配、入队在锁内 → 留档乱序 → SSE 重连重复投递已收事件）。
#[test]
fn archive_order_matches_sequence_under_concurrency() {
    let bus = std::sync::Arc::new(EventBus::new());
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let bus = bus.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    bus.emit(serde_json::json!({"thread": t, "i": i}));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let all = bus.replay_after(0);
    assert_eq!(all.len(), 400, "全部事件都应留档（窗口 1024 足够）");
    for w in all.windows(2) {
        assert!(
            w[0].0 < w[1].0,
            "留档必须按序号升序：{} 之后是 {}",
            w[0].0,
            w[1].0
        );
    }
    // 序号连续无空洞（同一临界区内分配）
    let ids: Vec<u64> = all.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, (1..=400).collect::<Vec<u64>>());
}

/// replay_after 的过滤语义：只回 last_id 之后的事件（含被挤出窗口的不会回）。
#[test]
fn replay_after_returns_only_newer_events() {
    let bus = EventBus::new();
    bus.emit(serde_json::json!({"n": 1}));
    bus.emit(serde_json::json!({"n": 2}));
    let tail = bus.replay_after(1);
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].0, 2);
    assert!(bus.replay_after(99).is_empty());
    assert_eq!(bus.last_id(), 2);
}

/// 窗口挤出后，回放方看到的缺口可以被识别（last_id 大于窗口内最大 id）。
#[test]
fn evicted_events_leave_an_identifiable_gap() {
    let bus = EventBus::new();
    for i in 0..(EventBus::CAP + 10) {
        bus.emit(serde_json::json!({"i": i}));
    }
    let window = bus.replay_after(0);
    assert_eq!(window.len(), EventBus::CAP, "窗口内只保留 CAP 条");
    assert!(
        window[0].0 > 1,
        "最旧的事件应已被挤出（首条 id {}），回放方据此识别缺口",
        window[0].0
    );
    assert_eq!(bus.last_id() as usize, EventBus::CAP + 10);
}
