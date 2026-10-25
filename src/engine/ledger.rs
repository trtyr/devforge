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

    pub fn remove(&mut self, pid: u32) {
        self.entries.retain(|e| e.pid != pid);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).ctx("序列化台账")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx("创建台账目录")?;
        }
        std::fs::write(path, json).ctx(format!("写台账 {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json =
            std::fs::read_to_string(path).ctx(format!("读台账 {}", path.display()))?;
        serde_json::from_str(&json).ctx("解析台账")
    }

    /// 台账文件的跨进程互斥（引擎批次：`max_concurrent_mutation` 默认 2，两个同项目
/// run 同时 spawn 时旧的「读-改-写」会互相覆写 PID 记录，后写者抹掉前者的登记）。
///
/// 实现：紧邻台账的 `spawn-ledger.json.lock` + `create_new` 抢占；拿不到就短睡重试，
/// 超过 deadline 或无法建锁（权限等）时**不加锁也继续**（台账写不进去只影响未来收割，
/// 不能把 spawn 路径卡死）；陈旧锁（持有者崩溃，>10s）会被清掉重抢。
pub fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let lock = path.with_extension("json.lock");
    if let Some(parent) = lock.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
    let mut acquired = false;
    while std::time::Instant::now() < deadline {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => {
                acquired = true;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|d| d.as_secs() > 10)
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(&lock);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
    let out = f();
    if acquired {
        let _ = std::fs::remove_file(&lock);
    }
    out
}

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
                let backup = path.with_extension("json.bak");
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
    /// 双验证：PID 存活 **且** /proc/<pid>/environ 有自家标记。
    pub fn reconcile(&self) -> Vec<Reconciliation> {
        self.entries
            .iter()
            .map(|e| {
                if !process_alive(e.pid) {
                    Reconciliation::Dead(e.clone())
                } else if has_devforge_marker(e.pid) {
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
        if e.run_id != run_id || !process_alive(e.pid) {
            continue;
        }
        // 双验证：environ 标记必须等于该 run-id（防 PID 复用误杀）
        if read_managed_marker(e.pid).as_deref() != Some(run_id) {
            continue;
        }
        // SAFETY: kill(2) 系统调用，仅传 pid 与 SIGKILL，无非空指针参与
        let rc = unsafe { libc::kill(e.pid as i32, libc::SIGKILL) };
        if rc == 0 {
            killed += 1;
        }
    }
    if killed > 0 {
        tracing::warn!(
            component = "scheduler",
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
                Reconciliation::Ours(e) => match kill_orphan(e.pid) {
                    Ok(()) => {
                        killed += 1;
                        tracing::warn!(
                            component = "engine-ledger",
                            event = "ledger.orphan_killed",
                            pid = e.pid,
                            run_id = %e.run_id,
                            role = %e.role,
                            "启动对账：上一轮遗留的自家子进程已 SIGTERM"
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
        if let Err(e) = kept.save(&path) {
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
        if read_managed_marker(pid).as_deref() == Some(run_id) {
            // SAFETY: kill(2)，SIGKILL 立决
            let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            if rc == 0 {
                killed += 1;
                tracing::warn!(
                    component = "ledger",
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
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn entry(pid: u32) -> LedgerEntry {
        LedgerEntry {
            pid,
            role: "plan-agent".into(),
            run_id: "run-1".into(),
            started_at_ms: 42,
        }
    }

    #[test]
    fn concurrent_registrations_do_not_overwrite_each_other() {
        // 引擎批次：max_concurrent_mutation 默认 2，两个同项目 run 同时 spawn 时
        // 旧的「读-改-写」会互相覆写——后写者抹掉前者的 PID 登记，删除时就成了孤儿。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn-ledger.json");
        let mut handles = Vec::new();
        for base in [1000u32, 2000u32] {
            let p = path.clone();
            handles.push(std::thread::spawn(move || {
                for i in 1..=5u32 {
                    SpawnLedger::with_file_lock(&p, || {
                        let mut l = SpawnLedger::load_for_record(&p);
                        l.record(entry(base + i));
                        l.save(&p).unwrap();
                    });
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let ledger = SpawnLedger::load(&path).unwrap();
        assert_eq!(
            ledger.entries.len(),
            10,
            "并发登记必须全部落盘，实际：{:?}",
            ledger.entries.iter().map(|e| e.pid).collect::<Vec<_>>()
        );
        assert!(!path.with_extension("json.lock").exists(), "锁文件必须收尾清掉");
    }

    #[test]
    fn missing_ledger_file_does_not_backup() {
        // 引擎批次 minor：ENOENT 是首次 spawn，不是「台账损坏」——不得生成 .bak
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spawn-ledger.json");
        assert!(SpawnLedger::load_for_record(&p).entries.is_empty());
        assert!(!p.with_extension("json.bak").exists(), "首次启动不该生成 .bak");
    }

    #[test]
    fn self_process_alive_and_dead_pid() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(u32::MAX - 100));
    }

    #[test]
    fn ledger_roundtrip_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.json");
        let mut ledger = SpawnLedger::default();
        ledger.record(entry(100));
        ledger.record(entry(200));
        ledger.save(&path).unwrap();
        let loaded = SpawnLedger::load(&path).unwrap();
        assert_eq!(ledger, loaded);
        let mut loaded = loaded;
        loaded.remove(100);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].pid, 200);
    }

    /// 轮询等待 /proc/<pid>/environ 出现（exec 竞态：spawn 后 environ 未必立即可读）。
    fn wait_for_marker(pid: u32, timeout_ms: u64) -> Option<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Some(v) = read_managed_marker(pid) {
                return Some(v);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn our_spawned_process_with_marker_is_ours() {
        let mut child = Command::new("sleep")
            .arg("30")
            .env(MANAGED_ENV, "run-orphan-1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(process_alive(pid));
        assert_eq!(
            wait_for_marker(pid, 2000).as_deref(),
            Some("run-orphan-1"),
            "environ 标记应在超时内可读"
        );
        assert!(has_devforge_marker(pid));

        let ledger = SpawnLedger {
            entries: vec![entry(pid)],
        };
        match ledger.reconcile().remove(0) {
            Reconciliation::Ours(_) => {}
            other => panic!("应判为自家孤儿: {other:?}"),
        }
        // Q005：直接杀
        kill_orphan(pid).unwrap();
        child.wait().unwrap(); // reap，防 zombie 占 /proc
        assert!(!process_alive(pid));
    }

    #[test]
    fn unmarked_live_process_is_foreign_never_killed() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(!has_devforge_marker(pid));
        let ledger = SpawnLedger {
            entries: vec![entry(pid)],
        };
        match ledger.reconcile().remove(0) {
            Reconciliation::Foreign(_) => {}
            other => panic!("无标记存活进程应判 Foreign: {other:?}"),
        }
        // 红线：Foreign 绝不 kill——这里只验证判定，不调用 kill_orphan
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn dead_pid_reconciles_dead() {
        let ledger = SpawnLedger {
            entries: vec![entry(u32::MAX - 100)],
        };
        match ledger.reconcile().remove(0) {
            Reconciliation::Dead(_) => {}
            other => panic!("死 PID 应判 Dead: {other:?}"),
        }
    }

    /// 引擎审计修复 #9 回归：台账精确命中（双验证）走台账路径杀真进程；
    /// 台账缺失/无路径时降级扫盘兕底不 panic（标记校验仍生效）。
    #[test]
    fn kill_run_hits_ledger_first_and_falls_back_with_warn() {
        // —— 台账命中：带标记真进程被台账路径精确击杀 ——
        let mut child = Command::new("sleep")
            .arg("30")
            .env(MANAGED_ENV, "run-1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-1"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn-ledger.json");
        let mut ledger = SpawnLedger::default();
        ledger.record(entry(pid)); // entry 的 run_id = "run-1"，与标记一致
        ledger.save(&path).unwrap();
        assert_eq!(kill_run(Some(&path), "run-1"), 1, "台账命中应精确击杀");
        child.wait().unwrap(); // reap，防 zombie 占 /proc
        assert!(!process_alive(pid));

        // —— 台账文件缺失：降级扫盘兕底（无匹配进程返 0，不 panic）——
        assert_eq!(kill_run(Some(&dir.path().join("nope.json")), "run-nope"), 0);
        // —— 无台账路径（watchdog 场景）：直接兕底扫盘 ——
        assert_eq!(kill_run(None, "run-never-existed"), 0);
    }

    /// 引擎审计修复 #6 回归：损坏台账备份 .bak 后空账起底——不许静默 unwrap_or_default
    /// 把历史 PID 丢成无痕。
    #[test]
    fn corrupt_ledger_is_backed_up_and_recovered_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn-ledger.json");
        std::fs::write(&path, "{not-json").unwrap();
        let ledger = SpawnLedger::load_for_record(&path);
        assert!(ledger.entries.is_empty(), "损坏后空账起底");
        let backup = path.with_extension("json.bak");
        assert!(backup.exists(), "坏文件应备份 .bak");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{not-json");
        assert!(!path.exists(), "原路径已让位给新账");
        // 新登记正常写入（不再被坏文件挡住）
        let mut ledger = SpawnLedger::default();
        ledger.record(entry(4242));
        ledger.save(&path).unwrap();
        assert_eq!(SpawnLedger::load(&path).unwrap().entries.len(), 1);
    }

    /// 引擎 #2 回归：启动对账清死条目、不碰 Foreign（无标记的活进程绝不动）。
    #[test]
    fn startup_reconcile_clears_dead_and_spares_foreign() {
        let plans = tempfile::tempdir().unwrap();
        let proj = plans.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("spawn-ledger.json");
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut ledger = SpawnLedger::default();
        ledger.record(entry(pid)); // 活但无标记 → Foreign
        ledger.record(entry(u32::MAX - 100)); // 已死 → Dead
        ledger.save(&path).unwrap();

        let (killed, cleared, foreign) = reconcile_all(plans.path());
        assert_eq!((killed, cleared, foreign), (0, 1, 1), "Foreign 不死、Dead 清掉");
        let after = SpawnLedger::load(&path).unwrap();
        assert_eq!(after.entries.len(), 1, "只留 Foreign：{:?}", after.entries);
        assert_eq!(after.entries[0].pid, pid);
        child.kill().unwrap();
        child.wait().unwrap();
    }

    /// 引擎 #2 回归：启动对账把上一轮遗留的**自家**子进程 SIGTERM 掉，并从台账清条目。
    #[test]
    fn startup_reconcile_kills_our_orphans() {
        let plans = tempfile::tempdir().unwrap();
        let proj = plans.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("spawn-ledger.json");
        let mut child = Command::new("sleep")
            .arg("30")
            .env(MANAGED_ENV, "run-1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-1"));
        let mut ledger = SpawnLedger::default();
        ledger.record(entry(pid));
        ledger.save(&path).unwrap();

        let (killed, _, _) = reconcile_all(plans.path());
        assert_eq!(killed, 1, "自家孤儿必须被杀");
        let _ = child.wait(); // SIGTERM 后退出，回收防 zombie
        assert!(!process_alive(pid));
        assert!(
            SpawnLedger::load(&path).unwrap().entries.is_empty(),
            "杀掉后条目必须从台账清掉"
        );
    }

    /// 引擎审计修复 #4 消费侧：台账存在但不可读时强杀安全失败（返 0 不 panic）——
    /// 真实挂死进程会漏杀，但 ERROR 日志已留痕（日志断言归 f6 域，此处锁行为面）。
    #[test]
    fn kill_run_processes_with_unreadable_ledger_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn-ledger.json");
        std::fs::write(&path, "{corrupt").unwrap();
        assert_eq!(kill_run_processes(&path, "run-x"), 0);
        // 台账不存在（冷启动）也返 0，但那是静默路径
        assert_eq!(kill_run_processes(&dir.path().join("nope.json"), "run-x"), 0);
    }
}
