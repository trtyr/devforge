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
}
