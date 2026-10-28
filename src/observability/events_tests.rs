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

/// 外部审计 #8：窗口滚动后，带旧 last_id 重连的客户端必须拿到缺口区间
/// （缺失段 = (last_id, 窗口首条) 之间被挤出的部分），且用缺口上界再连时收敛。
#[test]
fn replay_gap_reports_the_evicted_range_and_converges_on_reconnect() {
    let bus = EventBus::new();
    for i in 0..(EventBus::CAP + 10) {
        bus.emit(serde_json::json!({"i": i}));
    }
    let last_id: u64 = 3; // 客户端停在 3 号
    let (replay, gap) = bus.replay_after_with_gap(last_id);
    assert!(!replay.is_empty());
    let first = replay[0].0;
    assert_eq!(
        gap,
        Some((last_id + 1, first - 1)),
        "缺口 = (last_id+1, 窗口首条-1)"
    );
    assert!(replay.iter().all(|(id, _)| *id > last_id));
    // 合成事件的 id 取缺口上界：客户端用它重连，幸存集合不变且不再报缺口（收敛）。
    let (_, to) = gap.unwrap();
    let (again, gap2) = bus.replay_after_with_gap(to);
    assert_eq!(gap2, None, "用缺口上界重连不应再报缺口");
    assert_eq!(again.len(), replay.len(), "幸存回放集合不变");
}

/// 无挤出（窗口全覆盖）或新客户端（last_id = 0）都不报缺口。
#[test]
fn replay_gap_is_none_without_eviction_or_for_fresh_clients() {
    let bus = EventBus::new();
    for i in 0..5 {
        bus.emit(serde_json::json!({"n": i}));
    }
    assert_eq!(bus.replay_after_with_gap(2).1, None, "窗口覆盖 → 无缺口");
    assert_eq!(bus.replay_after_with_gap(0).1, None, "新客户端无基线 → 不报缺口");
}
