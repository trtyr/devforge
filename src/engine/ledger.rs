//! spawn 台账与孤儿对账（decisions/007 进程身份红线 + Q005 孤儿直接杀）。
//!
//! 红线：清理只按台账精确执行（PID + 双验证），绝不扫全系统杀 pi。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, Result};

/// 台账条目：每个被 spawn 的子进程一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub pid: u32,
    pub role: String,
    pub run_id: String,
    pub started_at_ms: u64,
}

/// spawn 台账（JSON 持久化，实时落盘——预防孤儿）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpawnLedger {
    pub entries: Vec<LedgerEntry>,
}

/// 对账结论（重启时逐条判定）。
#[derive(Debug, Clone, PartialEq)]
pub enum Reconciliation {
    /// PID 存活且环境变量有自家标记 → 我们的孤儿 → 直接杀（Q005）
    Ours(LedgerEntry),
    /// PID 存活但无自家标记 → 别人的进程（极端复用场景）→ 绝不动
    Foreign(LedgerEntry),
    /// PID 已死 → 台账清账即可
    Dead(LedgerEntry),
}

/// 环境标记变量名。
pub const MANAGED_ENV: &str = "DEVFORGE_MANAGED";

impl SpawnLedger {
    pub fn record(&mut self, entry: LedgerEntry) {
        self.entries.push(entry);
    }

    /// 摘除一条登记（子进程正常退出后调用；自审 minor 132：旧口径从不摘除，
    /// `remove` 是生产死代码，长驻服务内台账随 run 数单调增长）。
    pub fn remove(&mut self, pid: u32) {
        self.entries.retain(|e| e.pid != pid);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).ctx("序列化台账")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx("创建台账目录")?;
        }
        // 自审 major：原子写（同目录唯一临时名 + rename）——台账是熔断与启动对账的
        // 事实源，truncate+write 的半截文件会让读方把它当「损坏」，孤儿从此漏杀。
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        (|| -> Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).ctx("建台账临时文件")?;
            f.write_all(json.as_bytes()).ctx("写台账临时文件")?;
            f.sync_all().ctx("刷台账临时文件")?;
            std::fs::rename(&tmp, path).ctx(format!("原子替换台账 {}", path.display()))?;
            Ok(())
        })()
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json =
            std::fs::read_to_string(path).ctx(format!("读台账 {}", path.display()))?;
        serde_json::from_str(&json).ctx("解析台账")
    }
}

/// 台账（及并入方 queue.json）的跨进程互斥。
///
/// 实现（2026-10-27 自审 minor 33 重写）：`flock(LOCK_EX | LOCK_NB)` 抢**常驻**锁文件。
/// 旧口径是 `create_new` 抢锁 + 「mtime > 10s 视为陈旧就删」——陈旧判定与无条件删锁之间
/// 有竞态：持有者只是跑久了（或机器挂起）就会被别人删锁，自己恢复后又会删掉**别人**的锁，
/// 互斥静默降级为无。flock 由内核在进程退出/崩溃时自动释放，既没有陈旧判定也不存在
/// 「删别人的锁」；因此锁文件**不再删除**（删文件会引入 inode 竞态：两个进程各自持有
/// 不同 inode 上的锁，互斥形同虚设）。
///
/// 拿不到锁（超时/无法建文件）时**不加锁也继续**并 WARN——台账写不进去只影响未来收割，
/// 不能把 spawn 路径卡死。
pub fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    use std::os::unix::io::AsRawFd;

    let lock = path.with_extension("json.lock");
    if let Some(parent) = lock.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock);
    let mut acquired = false;
    match &opened {
        Ok(fh) => {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
            loop {
                // SAFETY: flock(2)，仅传打开的 fd 与常量，无非空指针参与
                let rc = unsafe { libc::flock(fh.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
                if rc == 0 {
                    acquired = true;
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        Err(e) => {
            tracing::warn!(
                component = "engine-ledger",
                lock = %lock.display(),
                error = %e,
                "台账锁文件打不开，无锁继续（丢更新风险自担）"
            );
        }
    }
    if !acquired && opened.is_ok() {
        // 自审 major：抢锁失败静默降级 = 把锁要防的丢更新竞态原样放回且无迹可查。
        tracing::warn!(
            component = "engine-ledger",
            lock = %lock.display(),
            "台账锁超时未获取，无锁继续（丢更新风险自担）"
        );
    }
    let out = f();
    if acquired {
        if let Ok(fh) = &opened {
            // SAFETY: flock(2) 解锁，同 fd
            unsafe {
                libc::flock(fh.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
    out
}

/// 子进程收场后从台账摘除（自审 minor 132：旧口径条目不摘除、`remove` 是生产死代码，
/// 长驻服务内台账随 run 数单调增长）。失败只 WARN——台账残留会导致下次对账多判一轮
/// （已死 = Dead，自动清），不影响正确性。
pub fn deregister(ledger_path: &Path, pid: u32) {
    if !ledger_path.exists() {
        return;
    }
    with_file_lock(ledger_path, || {
        let mut ledger = SpawnLedger::load_for_record(ledger_path);
        let before = ledger.entries.len();
        ledger.remove(pid);
        if ledger.entries.len() == before {
            return;
        }
        match ledger.save(ledger_path) {
            Ok(()) => tracing::debug!(
                component = "engine-ledger",
                event = "ledger.deregistered",
                pid,
                "子进程已从 spawn 台账摘除"
            ),
            Err(e) => tracing::warn!(
                component = "engine-ledger",
                event = "ledger.deregister_failed",
                path = %ledger_path.display(),
                pid,
                error = %e,
                "从 spawn 台账摘除失败（下次对账会按 Dead 清账）"
            ),
        }
    });
}

/// 台账文件的标准位置（自审 minor 133：路径契约不再靠各调用方各自推导）。
pub fn ledger_path(project_plans_root: &Path) -> PathBuf {
    project_plans_root.join("spawn-ledger.json")
}

impl SpawnLedger {
/// 登记前的装载（引擎审计修复 #6）：文件不存在 = 冷启动静默；存在但损坏 =
    /// WARN + 坏文件备份 .bak + 空账起底（与 store.json 口径一致）——
    /// 不许 unwrap_or_default 静默吞掉历史 PID 再覆写成空账。
    pub fn load_for_record(path: &Path) -> Self {
        // 引擎批次 minor：文件根本不存在 = 首次 spawn（冷启动），静默返回空账——
        // 旧实现把 ENOENT 也当「台账损坏」，首次开跑就 WARN + 备份失败日志，噪声掩盖真损坏。
        if !path.exists() {
            return Self::default();
        }
        match Self::load(path) {
            Ok(l) => l,
            Err(e) => {
                // 自审 minor（134）：备份名带时间戳——旧口径固定 `.bak`，第二代损坏会把
                // 上一代备份覆盖掉，被抹掉的 PID 那一代永久失联。
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let backup = path.with_extension(format!("json.bak.{stamp}"));
                let backed_up = std::fs::rename(path, &backup).is_ok();
                tracing::warn!(
                    component = "engine-ledger",
                    path = %path.display(),
                    backup = %backup.display(),
                    recovered = backed_up,
                    error = %e,
                    "spawn 台账损坏，备份后空账起底（历史 PID 丢失，孤儿对账能力受限）"
                );
                Self::default()
            }
        }
    }

    /// 重启对账：逐条判定 Ours（孤儿，可杀）/ Foreign（绝不动）/ Dead（清账）。
    /// 双验证：PID 存活 **且** /proc/<pid>/environ 的 DEVFORGE_MANAGED 值 == 台账条目的
    /// run_id——自审 major：旧口径只验「标记存在」，PID 被另一台 devforge run 复用时
    /// 会被误判 Ours 杀掉（杀别人的活进程 = 踩红线）。
    pub fn reconcile(&self) -> Vec<Reconciliation> {
        self.entries
            .iter()
            .map(|e| {
                if !process_alive(e.pid) {
                    Reconciliation::Dead(e.clone())
                } else if read_managed_marker(e.pid).as_deref() == Some(e.run_id.as_str()) {
                    Reconciliation::Ours(e.clone())
                } else {
                    Reconciliation::Foreign(e.clone())
                }
            })
            .collect()
    }
}

/// PID 是否存活（Linux /proc 探测）。
pub fn process_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

/// 读 /proc/<pid>/environ，返回 DEVFORGE_MANAGED 的值（无标记 → None）。
pub fn read_managed_marker(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    raw.split(|b| *b == 0)
        .filter(|kv| !kv.is_empty())
        .find_map(|kv| {
            let kv = std::str::from_utf8(kv).ok()?;
            kv.strip_prefix(MANAGED_ENV).and_then(|rest| {
                rest.strip_prefix('=').map(|v| v.to_string())
            })
        })
}

/// 红线判定的**单一事实源**（自审 minor 135）：PID 存活 **且** environ 标记值 == run_id。
/// 三条杀路径（reconcile / kill_run_processes / scan_and_kill_run）必须都走这里，不许各写一套。
pub fn is_ours(pid: u32, run_id: &str) -> bool {
    process_alive(pid) && read_managed_marker(pid).as_deref() == Some(run_id)
}

/// 自杀/杀祖先护栏（自审 minor 34）：本进程与父进程永不进杀路径，
/// 无论环境标记长什么样——标记值是可被继承/伪造的，护栏不该只靠它。
pub fn is_self_or_ancestor(pid: u32) -> bool {
    let me = std::process::id();
    pid == me || pid == std::os::unix::process::parent_id()
}

/// PID 是否带自家标记。
pub fn has_devforge_marker(pid: u32) -> bool {
    read_managed_marker(pid).is_some()
}

/// 复验熔断归因修复：按 run_id 强杀台账登记的进程。
/// 双验证（存活 + environ 标记 = 该 run-id），SIGKILL（挂死进程 TERM 不响应）。
/// 台账条目保留（Dead 由下次 reconcile 清）；无台账/无命中返回 0。
/// 红线不破：只按台账精确执行，绝不扫全系统。
/// 统一杀进程入口（引擎审计修复 #9：两套范式收敛成一条路径）。
/// 优先台账精确命中（双验证：存活 + environ 标记 = run-id）；台账缺失/不可读/零命中时
/// 降级 /proc 扫描兕底（标记校验仍生效）并打 WARN——扫盘不再是与台账并列的第二套范式，
/// 只是同一入口内的降级分支。
pub fn kill_run(ledger_path: Option<&Path>, run_id: &str) -> usize {
    if let Some(path) = ledger_path {
        let killed = kill_run_processes(path, run_id);
        if killed > 0 {
            return killed;
        }
        // 台账存在但零命中（PID 已死/条目陈旧）——不扫盘（避免误杀 PID 复用的新进程），
        // 台账不可读的 ERROR 已在 kill_run_processes 内留痕。
        if path.exists() {
            return 0;
        }
    }
    // 无台账路径或台账文件缺失：降级扫盘兕底（标记校验仍生效）
    tracing::warn!(
        component = "engine-ledger",
        run_id = %run_id,
        ledger = ?ledger_path.map(|p| p.display().to_string()),
        "台账未命中或不可用，降级 /proc 扫描兕底（DEVFORGE_MANAGED 标记校验仍生效）"
    );
    scan_and_kill_run(run_id)
}

pub fn kill_run_processes(ledger_path: &Path, run_id: &str) -> usize {
    let Ok(ledger) = SpawnLedger::load(ledger_path) else {
        // 引擎审计修复 #4 消费侧：熔断/对账读不到台账 = 精确收割失效，必须 ERROR 留痕——
        // 静默返 0 会让「挂死进程杀不到」变成无迹可寻。
        if ledger_path.exists() {
            tracing::error!(
                component = "engine-ledger",
                path = %ledger_path.display(),
                run_id = %run_id,
                "台账存在但不可读，按 run 强杀无法执行（挂死进程将漏杀）"
            );
        }
        return 0;
    };
    let mut killed = 0;
    for e in &ledger.entries {
        if e.run_id != run_id {
            continue;
        }
        // 双验证走单一事实源（存活 + 标记值 == run_id）+ 自杀护栏
        if !is_ours(e.pid, run_id) || is_self_or_ancestor(e.pid) {
            continue;
        }
        // SAFETY: kill(2) 系统调用，仅传 pid 与 SIGKILL，无非空指针参与
        let rc = unsafe { libc::kill(e.pid as i32, libc::SIGKILL) };
        if rc == 0 {
            killed += 1;
        } else {
            // 自审 major：kill 失败静默 = 把「没杀掉」报成「已强杀」。
            tracing::error!(
                component = "engine-ledger",
                pid = e.pid,
                run_id = %e.run_id,
                error = %std::io::Error::last_os_error(),
                "熔断 SIGKILL 失败，进程仍存活"
            );
        }
    }
    if killed > 0 {
        tracing::warn!(
            component = "engine-ledger",
            run_id,
            killed,
            "budget 熔断强杀：台账精确命中并 SIGKILL 挂死进程"
        );
    }
    killed
}

/// 启动对账（引擎 #2）：进程重启后扫 `plans/<project>/spawn-ledger.json`，把上一轮遗留的
/// **自家**子进程（Ours = 存活且带 DEVFORGE_MANAGED 标记）SIGTERM 清场，Dead 条目从台账
/// 清掉，Foreign 绝不杀（红线）。此前 reconcile/kill_orphan 只在单测里被调，崩溃孤儿会继续
/// 改目标仓。返回（killed, cleared, foreign）。
pub fn reconcile_all(plans_root: &Path) -> (usize, usize, usize) {
    let mut killed = 0usize;
    let mut cleared = 0usize;
    let mut foreign = 0usize;
    let Ok(rd) = std::fs::read_dir(plans_root) else {
        return (0, 0, 0);
    };
    for ent in rd.flatten() {
        let path = ent.path().join("spawn-ledger.json");
        if !path.is_file() {
            continue;
        }
        let Ok(ledger) = SpawnLedger::load(&path) else {
            // 坏台账不在这里重置——spawn 路径的 load_for_record 已做备份起底，
            // 启动对账只负责活进程，不碰账本格式。
            tracing::warn!(
                component = "engine-ledger",
                path = %path.display(),
                "启动对账跳过不可读台账"
            );
            continue;
        };
        let mut kept = SpawnLedger::default();
        for r in ledger.reconcile() {
            match r {
                Reconciliation::Ours(e) => match kill_orphan_wait(e.pid) {
                    Ok(true) => {
                        killed += 1;
                        tracing::warn!(
                            component = "engine-ledger",
                            event = "ledger.orphan_killed",
                            pid = e.pid,
                            run_id = %e.run_id,
                            role = %e.role,
                            "启动对账：上一轮遗留的自家子进程已 SIGTERM 且已退出"
                        );
                    }
                    Ok(false) => {
                        // 自审 major：SIGTERM 送达 ≠ 退出——忽略 TERM 的孤儿
                        // 不得从台账删痕，否则永久失联。
                        kept.record(e.clone());
                        tracing::error!(
                            component = "engine-ledger",
                            pid = e.pid,
                            run_id = %e.run_id,
                            "启动对账：孤儿 SIGTERM 后仍存活，保留台账条目待下次处理"
                        );
                    }
                    Err(err) => {
                        // 杀不掉就留着——下次启动再试，绝不从台账里删痕迹
                        kept.record(e);
                        tracing::error!(
                            component = "engine-ledger",
                            error = %err,
                            "启动对账：孤儿进程 SIGTERM 失败，保留台账条目"
                        );
                    }
                },
                Reconciliation::Foreign(e) => {
                    foreign += 1;
                    kept.record(e);
                }
                Reconciliation::Dead(_) => cleared += 1,
            }
        }
        // 自审 major：回写必须持台账锁，且按「处理前已知 pid 集」判重——
        // 直接拿盘上旧账并集会把本已清掉的 Dead 条目复活。
        let known: std::collections::BTreeSet<u32> =
            ledger.entries.iter().map(|e| e.pid).collect();
        if let Err(e) = crate::engine::ledger::with_file_lock(&path, || {
            let mut merged = kept.clone();
            if let Ok(later) = SpawnLedger::load(&path) {
                for e in later.entries {
                    if !known.contains(&e.pid) && !merged.entries.iter().any(|x| x.pid == e.pid) {
                        // 对账期间并发登记的新条目，不能丢
                        merged.record(e);
                    }
                }
            }
            merged.save(&path)
        }) {
            tracing::error!(
                component = "engine-ledger",
                path = %path.display(),
                error = %e,
                "启动对账：回写台账失败"
            );
        }
    }
    (killed, cleared, foreign)
}

/// 杀掉自家孤儿（Q005：直接杀，不收养）。SIGTERM 后短等退出。
/// 返回 Ok(true) = 已退出；Ok(false) = 送达但仍在跑（调用方应保留台账条目）。
pub fn kill_orphan_wait(pid: u32) -> Result<bool> {
    kill_orphan(pid)?;
    // 最多等 2 秒让它自己退；忽略 SIGTERM 的硬骨头留给下次（台账条目保留）。
    for _ in 0..40 {
        if process_gone(pid) {
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(process_gone(pid))
}

/// 进程是否已经走人（不存在**或 zombie**）。
/// 自审 major：SIGTERM 后子进程变 zombie（父进程未 wait 收尸时 /proc 条目还在），
/// 只看 /proc/<pid> 存在性会把「已死待收尸」当「还活着」，白等 2 秒后误报杀失败。
/// 生产里孤儿会被 init 收养自动收尸，但判定本身必须对 Z 态诚实。
fn process_gone(pid: u32) -> bool {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s,
        Err(_) => return true,
    };
    // stat 格式：pid (comm) state ...——comm 可含任意字符（含空格与括号），
    // 取最后一个 ')' 之后的第一个非空白字符才是 state。
    match stat.rsplit_once(')') {
        Some((_, rest)) => !rest.trim_start().starts_with('Z'),
        None => false,
    }
}

/// 杀掉自家孤儿（Q005：直接杀，不收养）。SIGTERM。
pub fn kill_orphan(pid: u32) -> Result<()> {
    // SAFETY: kill(2) 系统调用，仅传 pid 与 SIGTERM，无非空指针参与
    let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .ctx(format!("kill 孤儿进程 {pid}"));
    }
    Ok(())
}

/// 按 run-id 精确清场（P8-ISSUE-01 修复：取消/超时的 kill 通道）。
/// 扫 /proc 找 DEVFORGE_MANAGED == run_id 的进程（自家标记，非全盘扫杀——decisions/007
/// 红线不破：只动带本 run 标记的进程），SIGKILL 之（取消要立决，SIGTERM 可能被忽略）。
/// 返回杀掉的进程数。
pub fn scan_and_kill_run(run_id: &str) -> usize {
    let mut killed = 0usize;
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue; // 只看数字目录（进程）
        }
        let pid: u32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if is_ours(pid, run_id) && !is_self_or_ancestor(pid) {
            // SAFETY: kill(2)，SIGKILL 立决
            let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            if rc == 0 {
                killed += 1;
                tracing::warn!(
                    component = "engine-ledger",
                    run_id = %run_id,
                    pid,
                    "已取消/超时：SIGKILL 自家 run 进程"
                );
            }
        }
    }
    killed
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
