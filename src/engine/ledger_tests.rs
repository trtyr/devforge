//! （2026-10-26 测试分离：自 src/engine/ledger.rs 内联平移，纯移动零改动。）
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

/// 自审 major 回归：标记值≠台账条目 run_id 的活进程必须判 Foreign——
/// 旧口径只验「标记存在」，PID 被另一台 run 复用时会把它当 Ours 杀掉
///（杀别人的活进程 = 踩红线）。真子进程带另一 run 的标记验证。
#[test]
fn reconcile_never_claims_a_process_with_mismatched_run_id() {
    let mut child = Command::new("sleep")
        .arg("30")
        .env("DEVFORGE_MANAGED", "run-other")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    // exec 完成前 /proc/<pid>/environ 可能还没就绪——先等标记出现再对账，
    // 否则测的是竞态不是语义。
    assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-other"));
    let ledger = SpawnLedger {
        entries: vec![entry(pid)],
        ..Default::default()
    };
    let rec = ledger.reconcile();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        matches!(rec.as_slice(), [Reconciliation::Foreign(_)]),
        "标记值 run-other ≠ 条目 run-1，必须判 Foreign：{:?}",
        rec
    );
}

/// 正向配套：标记值 == 条目 run_id 才判 Ours（孤儿清理的真前提）。
#[test]
fn reconcile_claims_ours_only_when_marker_value_matches() {
    let mut child = Command::new("sleep")
        .arg("30")
        .env("DEVFORGE_MANAGED", "run-1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-1"));
    let ledger = SpawnLedger {
        entries: vec![entry(pid)],
        ..Default::default()
    };
    let rec = ledger.reconcile();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        matches!(rec.as_slice(), [Reconciliation::Ours(_)]),
        "标记值匹配条目 run-1，必须判 Ours：{:?}",
        rec
    );
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
    // 自审 major 语义升级：标记**值**必须等于条目 run_id（run-1）——
    // 旧口径只验标记存在，PID 被别的 run 复用时误杀。
    let mut child = Command::new("sleep")
        .arg("30")
        .env(MANAGED_ENV, "run-1")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    assert!(process_alive(pid));
    assert_eq!(
        wait_for_marker(pid, 2000).as_deref(),
        Some("run-1"),
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
