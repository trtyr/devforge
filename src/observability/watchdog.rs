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
/// 干预解阻钩子（自审 critical）：看门狗在杀进程路径（取消/墙钟超时/失活收割）上
/// 调用——run 线程可能正阻塞在 UiRelay::ask_blocking 的 recv() 上，只杀 pi 子进程
/// 解不了它；钩子把该 run 的待答请求丢掉（abort_run），recv 才会返回、run 才能落
/// 终态释放串行槽。由引擎调用方注入（传 UiRelay::abort_run 的闭包）。
pub type UnblockHook = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

pub(crate) fn run_with_watchdog<T>(
    control: &RunControl,
    run_id: String,
    idle_ms: Option<u64>,
    watch_dirs: Vec<PathBuf>,
    on_intervention: Option<UnblockHook>,
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
                    // 引擎审计修复 #9：统一杀进程入口（watchdog 无台账路径上下文 → 直接兕底扫盘，标记校验仍生效）
                    let killed = crate::engine::ledger::kill_run(None, &run_id);
                    if killed > 0 {
                        tracing::warn!(
                            component = "planning-engine",
                            run_id = %run_id,
                            killed,
                            "watchdog 已清场（取消或超时）"
                        );
                    }
                    // 自审 critical：杀进程不解阻 recv() 中的 ask_blocking——丢待答，run 线程才能收场
                    if let Some(hook) = &on_intervention {
                        hook(&run_id);
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
                        // 引擎审计修复 #9：统一杀进程入口（同上：直接兕底扫盘）
                        let killed = crate::engine::ledger::kill_run(None, &run_id);
                        if killed > 0 {
                            tracing::warn!(
                                component = "planning-engine",
                                run_id = %run_id,
                                killed,
                                "失活收割：SIGKILL 自家 run 进程"
                            );
                        }
                        // 自审 critical：同取消路径——收割也要解阻待答
                        if let Some(hook) = &on_intervention {
                            hook(&run_id);
                        }
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
        // 自审 critical：f() 一旦 panic，下面的置位会被展开跳过 → done 永假 → 看门狗
        // 循环不自退 → std::thread::scope 的 Drop 在展开期 join 看门狗线程 → run 线程
        // 永久挂死，server 层 catch_unwind 根本拿不到控制权。用 RAII 守卫保证
        // **正常返回与展开两条路都会置位**。
        struct DoneGuard(std::sync::Arc<AtomicBool>);
        impl Drop for DoneGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _done_guard = DoneGuard(done_main.clone());
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
#[path = "watchdog_tests.rs"]
mod tests;
