//! 失活收割看门狗：SOP 执行包裹 + 取消/超时/停滞告警/会话指纹活性监测
//! （ARC-R2 自 server/mod.rs 迁入 observability——活性监测本质是观测面职责）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::runner::RunControl;

/// 会话目录指纹：递归累加 (文件长度, mtime 纳秒) 的 FNV-1a 混合。
/// 任一文件被追加/触碰都会改变指纹——活性检测的「进展」信号
/// （006 decisions/002：会话增长=活，平线=失活）。
///
/// 2026-10-27 自审 major：任一监视目录不可读（或未提供任何目录）= **不可观测**
/// → 返回 None。旧口径读失败直接 return（指纹停在 FNV 基准值且永不变化）→
/// 「无进展」误判 → 活着的 run 被失活收割。不可观测即不判定——宁可漏杀不可误杀。
fn session_fingerprint(dirs: &[PathBuf]) -> Option<u64> {
    // 2026-10-27 自审 minor（171/172）：收集→排序→再哈希。旧口径按 read_dir 产出顺序
    // 递推混合（顺序敏感 = 目录内容不变也可能算出不同指纹 → 假进展），且条目不含
    // 路径身份（同名同长的两个文件互换会让指纹不变 → 漏报）。改为先把
    // (路径, 长度, mtime) 全收上来再排序，并按路径+元数据一起哈希。
    // 符号链接目录用 fs::metadata 跟随（旧口径 DirEntry::metadata 不跟随，
    // 链接出去的会话子目录永远不计入进展 → 假平线 → 活 run 被误收割）。
    fn walk(dir: &Path, acc: &mut Vec<(String, u64, u128)>) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // fs::metadata 跟随符号链接（读取失败则跳过该条目，不当作不可观测）
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if !walk(&path, acc) {
                    return false;
                }
            } else {
                let nanos = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                acc.push((path.to_string_lossy().to_string(), meta.len(), nanos));
            }
        }
        true
    }
    if dirs.is_empty() {
        return None;
    }
    let mut acc: Vec<(String, u64, u128)> = Vec::new();
    for d in dirs {
        if !walk(d, &mut acc) {
            return None;
        }
    }
    acc.sort();
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for (path, len, nanos) in acc {
        for b in path.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for b in len.to_le_bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for b in nanos.to_le_bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    Some(hash)
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

/// 一轮监测的四个职责（自审 minor 85）。
///
/// 旧口径四路监测（心跳 / 停滞告警 / 取消·超时清场 / 失活收割）挤在同一个
/// 300ms 循环体里，共享 `last_fp` / `last_progress` / `idle_active` / 两个
/// Once 标记——想改任何一路都得通读整段，而四路的**失败语义完全不同**：
/// 心跳失败只降可观测、停滞只告警、清场要杀、收割杀完还得继续守。
/// 抽成四个方法后 `tick` 只剩「谁在前谁在后」（顺序是有语义的：先判取消再判失活）。
struct WatchdogTick<'a> {
    run_id: &'a str,
    control: &'a RunControl,
    idle_ms: Option<u64>,
    watch_dirs: &'a [PathBuf],
    ledger_path: Option<&'a Path>,
    on_intervention: Option<&'a UnblockHook>,
    stall_warned: std::sync::Arc<AtomicBool>,
    /// 失明告警只发一次（目录恢复可读后自然恢复判定）
    blind_warned: std::sync::Arc<AtomicBool>,
    last_fp: Option<u64>,
    last_progress: u64,
    /// 失活收割只判一次：收割后停失活判定，但心跳/取消/超时监测继续
    idle_active: bool,
}

impl WatchdogTick<'_> {
    /// 跑一轮：返回 `true` = 看门狗该退出了（被取消/超时并被清场）。
    fn tick(&mut self) -> bool {
        self.heartbeat();
        self.warn_if_stalled();
        if self.intervene_if_cancelled() {
            return true;
        }
        self.reap_if_idle();
        false
    }

    /// 心跳低频兜底刷新（节流在 observability 内）：run 停在同一阶段、长时间无事件时，
    /// 时间戳仍向前走 → 能区分「卡死」与「已死」（六个 SOP 全经过本线程）。
    ///
    /// 刷失败不打断 run：节流到 debug（收尾终态才 WARN）。
    fn heartbeat(&self) {
        if let Err(e) = crate::observability::touch_run(self.run_id) {
            tracing::debug!(component = "watchdog", run_id = %self.run_id, error = %e, "周期心跳落盘失败");
        }
    }

    /// DR-3：agent 停滞告警——心跳只证本线程活，agent 活性看最近 agent 事件。
    /// 从未有 agent 事件（DR-1 型「根本没开轮」）按开跑时刻起算。每回合只发一次。
    fn warn_if_stalled(&self) {
        if let Some(age) = crate::observability::stall_check(self.run_id)
            && age > crate::observability::stall_warn_ms()
        {
            if !self.stall_warned.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    component = "watchdog",
                    event = "agent.stall",
                    run_id = %self.run_id,
                    age_ms = age,
                    threshold_ms = crate::observability::stall_warn_ms(),
                    "agent 停滞告警：超阈值无任何 agent 事件（turn_start/turn_end/失败）——run 可能假活"
                );
            }
        } else {
            self.stall_warned.store(false, Ordering::Relaxed);
        }
    }

    /// 取消/墙钟超时 → 杀自家进程 + 解阻待答 → 看门狗退出。
    fn intervene_if_cancelled(&self) -> bool {
        let now = crate::domain::now_ms();
        if !(self.control.is_cancelled() || self.control.timed_out(now)) {
            return false;
        }
        // 引擎审计修复 #9：统一杀进程入口；自审 minor（175）起带台账路径——
        // 精确命中优先，扫盘只在台账缺失时降级（标记校验仍生效）。
        let killed = crate::engine::ledger::kill_run(self.ledger_path, self.run_id);
        // 2026-10-27 自审 major：killed==0 是常态（进程已退出/从未 spawn），
        // 但「看门狗观测到取消」这件事必须无条件留痕——旧口径只在杀到人时记日志。
        tracing::warn!(
            component = "watchdog",
            event = "watchdog.cancel",
            run_id = %self.run_id,
            killed,
            "watchdog 清场（取消或超时）；killed=0 = 无存活进程可杀"
        );
        // 自审 critical：杀进程不解阻 recv() 中的 ask_blocking——丢待答，run 线程才能收场
        if let Some(hook) = self.on_intervention {
            hook(self.run_id);
        }
        true
    }

    /// 失活收割（006 decisions/002）：会话文件指纹平线超过阈值 → 杀自家 pi。
    fn reap_if_idle(&mut self) {
        let Some(idle) = self.idle_ms else { return };
        if !self.idle_active {
            return;
        }
        let now = crate::domain::now_ms();
        match session_fingerprint(self.watch_dirs) {
            None => {
                // 失明保护：任一监视目录不可读 = 不可观测 → 不判定失活。
                // 旧口径指纹停在基准值 → 平线误判 → 活 run 被误杀。
                if !self.blind_warned.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        component = "watchdog",
                        event = "watchdog.blind",
                        run_id = %self.run_id,
                        "会话目录不可读，失活收割暂停（不可观测即不判定，宁可漏杀不可误杀）"
                    );
                }
            }
            Some(fp) if Some(fp) != self.last_fp => {
                self.last_fp = Some(fp);
                self.last_progress = now;
            }
            Some(_) if now.saturating_sub(self.last_progress) > idle => {
                self.control.mark_stalled();
                tracing::warn!(
                    component = "watchdog",
                    event = "watchdog.reap",
                    run_id = %self.run_id,
                    idle_ms = idle,
                    "会话无进展超过闲置阈值，失活收割"
                );
                // 引擎审计修复 #9：统一杀进程入口（同上：台账精确路径优先）
                let killed = crate::engine::ledger::kill_run(self.ledger_path, self.run_id);
                tracing::warn!(
                    component = "watchdog",
                    event = "watchdog.reap.kill",
                    run_id = %self.run_id,
                    killed,
                    "失活收割完成（killed=0 = 无存活进程可杀）"
                );
                // 自审 critical：同取消路径——收割也要解阻待答
                if let Some(hook) = self.on_intervention {
                    hook(self.run_id);
                }
                // 2026-10-27 自审 major：收割后不退出——心跳/取消/超时监测
                // 继续守护到 run 收场，只停失活判定（不重复杀）。
                self.idle_active = false;
            }
            Some(_) => {}
        }
    }
}

pub(crate) fn run_with_watchdog<T>(
    control: &RunControl,
    run_id: String,
    idle_ms: Option<u64>,
    watch_dirs: Vec<PathBuf>,
    // 自审 minor（175）：台账路径——强杀优先走精确路径，扫盘只是降级兜底。
    // 旧口径恒传 None，看门狗的杀伤面因此总是「全机带同名标记的进程」，比台账宽。
    ledger_path: Option<PathBuf>,
    on_intervention: Option<UnblockHook>,
    f: impl FnOnce() -> T,
) -> T {
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_wd = done.clone();
    let done_main = done.clone();
    // DR-3：停滞告警每回合只发一次（恢复后重置）
    let stall_warned = std::sync::Arc::new(AtomicBool::new(false));
    // 自审 minor（40）：不再克隆 run_id——`std::thread::scope` 允许借用栈，
    // 看门狗闭包直接拿 `&run_id`，收尾路径也用同一个借用。
    let stall_wd = stall_warned.clone();
    std::thread::scope(|scope| {
        // 自审 minor（40）：只借用，不搬所有权——scope 允许借用栈，
        // 看门狗与收尾路径共用同一个 `&run_id`（旧口径为了一条收尾日志白克隆一次）。
        let run_id_ref: &str = &run_id;
        scope.spawn(move || {
            let run_id: &str = run_id_ref;
            let mut tick = WatchdogTick {
                run_id,
                control,
                idle_ms,
                watch_dirs: &watch_dirs,
                ledger_path: ledger_path.as_deref(),
                on_intervention: on_intervention.as_ref(),
                stall_warned: stall_wd,
                blind_warned: std::sync::Arc::new(AtomicBool::new(false)),
                last_fp: session_fingerprint(&watch_dirs),
                last_progress: crate::domain::now_ms(),
                idle_active: true,
            };
            loop {
                if done_wd.load(Ordering::SeqCst) {
                    return;
                }
                if tick.tick() {
                    return;
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
        if let Err(e) = crate::observability::touch_run(&run_id) {
            // 2026-10-27 自审 minor（177）：死亡现场的最后一次心跳落盘失败必须响——
            // 旧口径在 write_heartbeat 里只留 debug，现场证据静默缺失。
            tracing::warn!(
                component = "watchdog",
                event = "watchdog.heartbeat.final_failed",
                run_id = %run_id,
                error = %e,
                "收尾终态心跳落盘失败——run 死亡现场的心跳证据缺失"
            );
        }
        // 自审 minor（174）：节流表是进程级 map，run 收场必须摘条目（否则按 run 无界增长）
        crate::observability::forget_run_throttle(&run_id);
        out
    })
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
