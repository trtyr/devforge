//! （2026-10-26 测试分离：自 src/observability/watchdog.rs 内联平移，纯移动零改动。）
use super::*;
use std::time::{Duration, Instant};

#[test]
fn watchdog_scope_returns_after_closure_completes() {
    // 回归（verdict 后挂起）：f 正常返回后 scope join 必须解锁（旧实现此处死锁）
    let control = RunControl::new();
    let start = Instant::now();
    let out = run_with_watchdog(&control, "watchdog-regression".into(), None, vec![], None, || {
        std::thread::sleep(Duration::from_millis(400));
        42
    });
    assert_eq!(out, 42);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "scope join 死锁：elapsed={:?}",
        start.elapsed()
    );
}

#[test]
fn the_deferred_begin_does_not_write_the_heartbeat_on_the_critical_path() {
    // F18：延迟盘写 = 纯内存置位，调用线程不落盘（首刷由 watchdog tick，
    // 终态由 run_with_watchdog 收尾定格）——盘 I/O 彻底离开开始关键路径
    let dir = tempfile::tempdir().unwrap();
    crate::observability::begin_run_in_deferred(
        dir.path().to_path_buf(),
        "deferred-heartbeat",
        "plan",
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        crate::observability::heartbeat::read(dir.path(), "deferred-heartbeat").is_err(),
        "延迟盘写变体不应在调用线程同步落心跳"
    );
}

#[test]
fn the_watchdog_refreshes_the_heartbeat_while_the_run_is_stalled() {
    // 错实现形态：watchdog 不刷心跳（只有阶段转换才写）→ 长 agent 轮里心跳陈旧
    // → 「卡死」与「已死」分不清（本用例必须变红）
    let dir = tempfile::tempdir().unwrap();
    let run_id = "watchdog-heartbeat";
    // 观测根按 run 注入到临时目录（不抢进程级全局 → 与并行单测不互相干扰）
    crate::observability::begin_run_in(dir.path().to_path_buf(), run_id, "plan");
    let before = crate::observability::heartbeat::read(dir.path(), run_id)
        .expect("心跳应已落盘")
        .timestamp_ms;
    let control = RunControl::new();
    // 停 1.6s：心跳节流 1s → watchdog 的第 5 跳（~1.2s）必须落在观察窗**内**
    // （停在 1.2s 会与那一跳撞在同一毫秒 → 偶发读不到刷新）
    run_with_watchdog(&control, run_id.into(), None, vec![], None, || {
        std::thread::sleep(Duration::from_millis(1600));
    });
    let after = crate::observability::heartbeat::read(dir.path(), run_id)
        .expect("心跳应仍在")
        .timestamp_ms;
    crate::observability::end_run(run_id);
    assert!(
        after > before,
        "心跳时间戳应随之刷新（区分「卡死」与「已死」）：before={before} after={after}"
    );
}

#[test]
fn watchdog_scope_cancel_path_does_not_hang() {
    // cancel 置位 → watchdog 触发清场路径（scan_and_kill_run 对不存在进程为 no-op）→ 退出
    let control = RunControl::new();
    control.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    let start = Instant::now();
    let out = run_with_watchdog(&control, "watchdog-cancel".into(), None, vec![], None, || 7);
    assert_eq!(out, 7);
    assert!(start.elapsed() < Duration::from_secs(5));
}

// ---------- 失活收割（006 decisions/002：默认不杀，显式闲置阈值才判死） ----------

#[test]
fn session_fingerprint_changes_on_append() {
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    let fp0 = session_fingerprint(std::slice::from_ref(&watch));
    std::fs::write(watch.join("s.jsonl"), b"hello").unwrap();
    let fp1 = session_fingerprint(std::slice::from_ref(&watch));
    assert_ne!(fp0, fp1, "新建文件应改变指纹");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(watch.join("s.jsonl"), b"hello world").unwrap();
    let fp2 = session_fingerprint(&[watch]);
    assert_ne!(fp1, fp2, "追加增长应改变指纹");
}

#[test]
fn idle_configured_growth_keeps_run_alive() {
    // 增长=活：会话文件持续增长时，闲置阈值再小也不收割（慢而活着永不误伤）
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    let writer = watch.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_w = stop.clone();
    let grower = std::thread::spawn(move || {
        let mut i = 0u32;
        while !stop_w.load(std::sync::atomic::Ordering::SeqCst) {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(writer.join("s.jsonl"))
                .unwrap();
            use std::io::Write;
            let _ = writeln!(f, "line {i}");
            i += 1;
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    });
    let control = RunControl::new();
    run_with_watchdog(
        &control,
        "liveness-growth".into(),
        Some(600),
        vec![watch],
        None,
        || std::thread::sleep(Duration::from_millis(1200)),
    );
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    grower.join().unwrap();
    assert!(!control.is_stalled(), "持续增长的会话不应被失活收割");
}

#[test]
fn idle_configured_flatline_gets_harvested() {
    // 平线超阈=死：会话无进展超过闲置阈值 → watchdog 置 stalled 并收割
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    let control = RunControl::new();
    run_with_watchdog(
        &control,
        "liveness-flatline".into(),
        Some(400),
        vec![watch],
        None,
        || std::thread::sleep(Duration::from_millis(1500)),
    );
    assert!(control.is_stalled(), "平线超阈应被失活收割");
}

#[test]
fn idle_unconfigured_never_harvests() {
    // 未设置不超时：不做活性收割，静默会话也永不自动杀（零硬限制哲学）
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    let control = RunControl::new();
    run_with_watchdog(
        &control,
        "liveness-off".into(),
        None,
        vec![watch],
        None,
        || std::thread::sleep(Duration::from_millis(900)),
    );
    assert!(!control.is_stalled(), "未配置闲置阈值时永不失活收割");
}
