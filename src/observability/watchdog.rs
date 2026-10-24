//! 失活收割看门狗：SOP 执行包裹 + 取消/超时/停滞告警/会话指纹活性监测
//! （ARC-R2 自 server/mod.rs 迁入 observability——活性监测本质是观测面职责）。

use std::path::{Path, PathBuf};

use crate::domain::runner::RunControl;

/// 会话目录指纹：递归累加 (文件长度, mtime 纳秒) 的 FNV-1a 混合。
/// 任一文件被追加/触碰都会改变指纹——活性检测的「进展」信号
/// （006 decisions/002：会话增长=活，平线=失活）。
fn session_fingerprint(dirs: &[PathBuf]) -> u64 {
    fn walk(dir: &Path, hash: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, hash);
            } else {
                let nanos = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                for b in meta.len().to_le_bytes() {
                    *hash ^= b as u64;
                    *hash = hash.wrapping_mul(0x100000001b3);
                }
                for b in nanos.to_le_bytes() {
                    *hash ^= b as u64;
                    *hash = hash.wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for d in dirs {
        walk(d, &mut hash);
    }
    hash
}

/// SOP 执行 + 取消/超时/失活看门狗。
///
/// 契约：`f` 返回后立刻置位 done（在 scope 闭包内），watchdog ≤300ms 退出、scope join 才能返回——
/// 否则 done 置位不可达（在 scope 返回后）而 scope 等 watchdog，正常完成必死锁
/// （第 18/19/20 试 verdict 后挂起的真因）。
///
/// 失活收割（006 decisions/002）：`idle_ms` 为 Some 时，监控 `watch_dirs` 的会话文件指纹；
/// 任一变化即刷新进展时钟，平线超过阈值 → mark_stalled + 杀自家 pi——只杀「真死」的 run，
/// 慢而活着的 run（含长流式生成中的增长）永不误伤；None = 不做活性收割。
pub(crate) fn run_with_watchdog<T>(
    control: &RunControl,
    run_id: String,
    idle_ms: Option<u64>,
    watch_dirs: Vec<PathBuf>,
    f: impl FnOnce() -> T,
) -> T {
    use std::sync::atomic::{AtomicBool, Ordering};
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_wd = done.clone();
    let done_main = done.clone();
    // DR-3：停滞告警每回合只发一次（恢复后重置）
    let stall_warned = std::sync::Arc::new(AtomicBool::new(false));
    let stall_wd = stall_warned.clone();
    let run_id_final = run_id.clone();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut last_fp = session_fingerprint(&watch_dirs);
            let mut last_progress = crate::domain::now_ms();
            loop {
                if done_wd.load(Ordering::SeqCst) {
                    return;
                }
                // 心跳低频兜底刷新（节流在 observability 内）：run 停在同一阶段、长时间无事件时，
                // 时间戳仍向前走 → 能区分「卡死」与「已死」（六个 SOP 全经过本线程）。
                crate::observability::touch_run(&run_id);
                // DR-3：agent 停滞告警——心跳只证本线程活，agent 活性看最近 agent 事件。
                // 从未有 agent 事件（DR-1 型「根本没开轮」）按开跑时刻起算。
                if let Some(age) = crate::observability::stall_check(&run_id)
                    && age > crate::observability::stall_warn_ms()
                {
                    if !stall_wd.swap(true, Ordering::Relaxed) {
                        tracing::warn!(
                            component = "server",
                            event = "agent.stall",
                            run_id = %run_id,
                            age_ms = age,
                            threshold_ms = crate::observability::stall_warn_ms(),
                            "agent 停滞告警：超阈值无任何 agent 事件（turn_start/turn_end/失败）——run 可能假活"
                        );
                    }
                } else {
                    stall_wd.store(false, Ordering::Relaxed);
                }
                let now = crate::domain::now_ms();
                if control.is_cancelled() || control.timed_out(now) {
                    let killed = crate::engine::ledger::scan_and_kill_run(&run_id);
                    if killed > 0 {
                        tracing::warn!(
                            component = "planning-engine",
                            run_id = %run_id,
                            killed,
                            "watchdog 已清场（取消或超时）"
                        );
                    }
                    return;
                }
                if let Some(idle) = idle_ms {
                    let fp = session_fingerprint(&watch_dirs);
                    if fp != last_fp {
                        last_fp = fp;
                        last_progress = now;
                    } else if now.saturating_sub(last_progress) > idle {
                        control.mark_stalled();
                        tracing::warn!(
                            component = "planning-engine",
                            run_id = %run_id,
                            idle_ms = idle,
                            "会话无进展超过闲置阈值，失活收割"
                        );
                        let killed = crate::engine::ledger::scan_and_kill_run(&run_id);
                        if killed > 0 {
                            tracing::warn!(
                                component = "planning-engine",
                                run_id = %run_id,
                                killed,
                                "失活收割：SIGKILL 自家 run 进程"
                            );
                        }
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
        let out = f();
        // 修复点：f 返回后立刻置位（在 scope 闭包内），watchdog 才能退出、join 才能返回；
        // 若在 scope 返回后才置位则不可达，正常完成必死锁（verdict 后挂起真因）
        done_main.store(true, Ordering::SeqCst);
        // F18：收尾终态刷盘——run 的最后一次心跳（阶段/步骤定格在终态）。
        // 在 f() 之后（尾部），不占「开跑→首让」竞态窗口（agent 实验证尾部写不触发）。
        crate::observability::touch_run(&run_id_final);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn watchdog_scope_returns_after_closure_completes() {
        // 回归（verdict 后挂起）：f 正常返回后 scope join 必须解锁（旧实现此处死锁）
        let control = RunControl::new();
        let start = Instant::now();
        let out = run_with_watchdog(&control, "watchdog-regression".into(), None, vec![], || {
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
        run_with_watchdog(&control, run_id.into(), None, vec![], || {
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
        let out = run_with_watchdog(&control, "watchdog-cancel".into(), None, vec![], || 7);
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
            || std::thread::sleep(Duration::from_millis(900)),
        );
        assert!(!control.is_stalled(), "未配置闲置阈值时永不失活收割");
    }
}
