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
                crate::engine::ledger::with_file_lock(&p, || {
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
    // 2026-10-27 自审 minor（33）：锁改 flock 后锁文件**常驻不删**（删文件会引入 inode
    // 竞态：两个进程各持不同 inode 上的锁，互斥形同虚设）；互斥由内核在释放/崩溃时解除。
    assert!(
        path.with_extension("json.lock").exists(),
        "flock 锁文件应常驻（不删才没有 inode 竞态）"
    );
}

#[test]
fn missing_ledger_file_does_not_backup() {
    // 引擎批次 minor：ENOENT 是首次 spawn，不是「台账损坏」——不得生成 .bak
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("spawn-ledger.json");
    assert!(SpawnLedger::load_for_record(&p).entries.is_empty());
    assert!(
        !p.with_extension("json.bak").exists(),
        "首次启动不该生成 .bak"
    );
    assert!(
        !std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("spawn-ledger.json.bak")),
        "首次启动不该生成任何代际备份"
    );
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

/// 夹具层剥离 `DEVFORGE_MANAGED`：构造「未标记进程」时标记必须由夹具保证，
/// 不能靠调用者的父环境恰好没设这个变量——测试跑在 devforge 受管树里时父环境**会**带它
/// （生产注入在 `src/engine/process.rs` 的 spawn 命令里，那一行不动）。
fn strip_managed_marker(cmd: &mut Command) -> &mut Command {
    cmd.env_remove(MANAGED_ENV)
}

/// 探针变量名：夹具显式注入子进程，用来证明 `/proc/<pid>/environ` 已经可读（exec 已完成）。
const PROBE_ENV: &str = "DEVFORGE_TEST_PROBE";

/// 轮询等待子进程 environ 里出现探针变量 → 证明 exec 完成、environ 可读。
///
/// 负向断言（「没有标记」）只有在 environ 可读时才有意义：exec 完成前 environ 读不到，
/// `has_devforge_marker` 会先返回 false——断言成了空断言（怎么改都绿，变异探测会空过）。
fn wait_for_environ_ready(pid: u32, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let ready = match std::fs::read(format!("/proc/{pid}/environ")) {
            Ok(raw) => raw.split(|b| *b == 0).any(|kv| {
                kv.starts_with(PROBE_ENV.as_bytes()) && kv.get(PROBE_ENV.len()) == Some(&b'=')
            }),
            Err(_) => false,
        };
        if ready {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 回归：夹具自己保证「未标记」——父环境带 `DEVFORGE_MANAGED` 时子进程也不该带标记。
#[test]
fn the_fixture_strips_the_managed_marker_even_when_the_parent_env_carries_one() {
    // 错实现形态：夹具不剥离（子进程带着「模拟父环境」的标记）→ 断言读到标记值 → 必须变红
    let mut cmd = Command::new("sleep");
    cmd.arg("30")
        // 模拟「父环境带了标记」：显式注入一个假标记值
        .env(MANAGED_ENV, "run-from-parent-env")
        // 探针：证明 environ 已可读，负向断言不是空断言
        .env(PROBE_ENV, "1")
        .stdout(Stdio::null());
    strip_managed_marker(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    assert!(
        wait_for_environ_ready(pid, 2000),
        "子进程 environ 应在超时内可读（探针 {PROBE_ENV} 可见）"
    );
    assert_eq!(
        read_managed_marker(pid),
        None,
        "夹具剥离后子进程不得带 {MANAGED_ENV} 标记（读到的是继承/注入进来的标记值）"
    );
    child.kill().unwrap();
    child.wait().unwrap();
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

/// 未标记的活进程必须判 Foreign、绝不被杀。
///
/// 「未标记」这个前提**由夹具剥离保证**（`strip_managed_marker`），不依赖父环境：
/// 测试跑在 devforge 受管树里时父环境带着 `DEVFORGE_MANAGED`，靠父环境「恰好没设」是巧合。
#[test]
fn unmarked_live_process_is_foreign_never_killed() {
    let mut cmd = Command::new("sleep");
    cmd.arg("30")
        .env(PROBE_ENV, "1")
        .stdout(Stdio::null());
    strip_managed_marker(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    // 先证明 environ 可读（exec 已完成），否则「无标记」是空断言：exec 前 environ 读不到，
    // `has_devforge_marker` 会先返回 false——剥离与否都绿。
    assert!(
        wait_for_environ_ready(pid, 2000),
        "子进程 environ 应在超时内可读（探针 {PROBE_ENV} 可见）"
    );
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
    // 2026-10-27 自审 minor（134）：备份名带时间戳（`json.bak.<ms>`）——固定 `.bak`
    // 会被下一代损坏覆盖，被抹掉的那一代 PID 永久失联。
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("spawn-ledger.json.bak"))
        .collect();
    assert_eq!(backups.len(), 1, "坏文件应备份一份：{backups:?}");
    let backup = dir.path().join(&backups[0]);
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{not-json");
    assert!(!path.exists(), "原路径已让位给新账");
    // 新登记正常写入（不再被坏文件挡住）
    let mut ledger = SpawnLedger::default();
    ledger.record(entry(4242));
    ledger.save(&path).unwrap();
    assert_eq!(SpawnLedger::load(&path).unwrap().entries.len(), 1);
}

/// 引擎 #2 回归：启动对账清死条目、不碰 Foreign（无标记的活进程绝不动）。
///
/// 「活但无标记」的前提**由夹具剥离保证**（`strip_managed_marker`），不依赖父环境。
#[test]
fn startup_reconcile_clears_dead_and_spares_foreign() {
    let plans = tempfile::tempdir().unwrap();
    let proj = plans.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let path = proj.join("spawn-ledger.json");
    let mut cmd = Command::new("sleep");
    cmd.arg("30")
        .env(PROBE_ENV, "1")
        .stdout(Stdio::null());
    strip_managed_marker(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    // 先证明 environ 可读：Foreign 判定必须建立在「读到了 environ 且其中没有标记」上，
    // 否则是空断言（exec 完成前 environ 读不到 → 必然判 Foreign）。
    assert!(
        wait_for_environ_ready(pid, 2000),
        "子进程 environ 应在超时内可读（探针 {PROBE_ENV} 可见）"
    );
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
    // 5s 而非 2s：全量并行跑时 spawn→exec 的 environ 就绪可能被拖到 2s 之外（flake 源）
    assert_eq!(wait_for_marker(pid, 5000).as_deref(), Some("run-1"));
    let mut ledger = SpawnLedger::default();
    ledger.record(entry(pid));
    ledger.save(&path).unwrap();

    let (killed, cleared, foreign) = reconcile_all(plans.path());
    // 诊断属性：这条曾多次在并行全量里碎（flake），失败时必须带出三分类计数，
    // 否则只看到 left: 0 无法区分「判成 Foreign」与「杀不动」。
    assert_eq!(
        killed, 1,
        "自家孤儿必须被杀（killed={killed} cleared={cleared} foreign={foreign}）"
    );
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

// ── 2026-10-27 自审 minor（64/34/135）：红线负向断言 + 自杀护栏 ──

/// 负向断言（旧测试全绿即使把 run_id 比对删掉）：
/// 台账条目的 run_id 与进程标记**不一致**时，kill_run_processes 一个都不许杀。
#[test]
fn kill_run_processes_never_kills_a_different_run() {
    let mut child = Command::new("sleep")
        .arg("30")
        .env("DEVFORGE_MANAGED", "run-other")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-other"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spawn-ledger.json");
    let mut ledger = SpawnLedger::default();
    // 台账声称这个 pid 属于 run-1，但进程实际标记是 run-other → 必须 Foreign
    ledger.record(LedgerEntry {
        pid,
        role: "plan-agent".into(),
        run_id: "run-1".into(),
        started_at_ms: 1,
    });
    ledger.save(&path).unwrap();

    assert_eq!(
        kill_run_processes(&path, "run-1"),
        0,
        "标记值不符者绝不许杀（红线）"
    );
    assert!(process_alive(pid), "被误杀的进程必须还活着");

    let _ = child.kill();
    let _ = child.wait();
}

/// 负向断言：扫盘兕底路径同样只动标记值 == run_id 的进程（改回「标记存在即杀」必须变红）。
#[test]
fn scan_and_kill_run_only_touches_matching_marker_values() {
    let mut child = Command::new("sleep")
        .arg("30")
        .env("DEVFORGE_MANAGED", "run-other")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    assert_eq!(wait_for_marker(pid, 2000).as_deref(), Some("run-other"));
    // run_id 用本测试独有值：scan 是全局 /proc 扫描，用 "run-1" 会误伤并发测试的子进程
    let my_run = format!("run-scan-only-{}", std::process::id());
    assert_eq!(scan_and_kill_run(&my_run), 0, "别人的 run 一个都不许杀");
    assert!(process_alive(pid));
    let _ = child.kill();
    let _ = child.wait();
}

/// 自杀/杀祖先护栏（自审 minor 34）：护栏不该只靠环境标记值。
#[test]
fn self_and_ancestor_are_guarded() {
    assert!(is_self_or_ancestor(std::process::id()), "本进程必须被护栏");
    assert!(
        is_self_or_ancestor(std::os::unix::process::parent_id()),
        "父进程必须被护栏"
    );
    // 无标记的进程：is_ours 必须为假（旧口径「标记存在即 ours」会误判）
    assert!(!is_ours(std::process::id(), "run-1") || has_devforge_marker(std::process::id()));
}

/// 132：收场摘除台账条目（旧口径 remove 是生产死代码，台账单调增长）。
#[test]
fn deregister_removes_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spawn-ledger.json");
    let mut ledger = SpawnLedger::default();
    ledger.record(entry(4242));
    ledger.record(entry(4343));
    ledger.save(&path).unwrap();

    deregister(&path, 4242);
    let after = SpawnLedger::load(&path).unwrap();
    assert_eq!(
        after.entries.iter().map(|e| e.pid).collect::<Vec<_>>(),
        vec![4343],
        "只摘指定 pid"
    );

    // 幂等：再摘一次不炸、不改变结果
    deregister(&path, 9999);
    assert_eq!(SpawnLedger::load(&path).unwrap().entries.len(), 1);

    // 台账不存在时不建文件
    let missing = dir.path().join("nope.json");
    deregister(&missing, 1);
    assert!(!missing.exists());
}

/// 2026-10-28 flake 根因回归："已死待收尸"（zombie）必须算「走人」。
///
/// 旧口径 `process_gone` 的判定写反了（`!starts_with('Z')`）——zombie 被判「还活着」，
/// `kill_orphan_wait` 于是白等满整份预算后报「孤儿杀不掉」，台账条目被保留。
/// 全量测试里子进程先自行退出（变 zombie）的那一次就成了随机红。
#[test]
fn a_zombie_child_counts_as_gone() {
    let mut child = Command::new("true").stdout(Stdio::null()).spawn().unwrap();
    // 等它退出成 zombie（父进程不 wait 就不会被收尸）：看 state 位变成 Z
    let pid = child.id();
    let mut zombied = false;
    for _ in 0..100 {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && stat.rsplit_once(')').is_some_and(|(_, r)| r.trim_start().starts_with('Z'))
        {
            zombied = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(zombied, "子进程应已退出待收尸");
    let started = std::time::Instant::now();
    assert!(
        kill_orphan_wait_within(pid, 1000).unwrap(),
        "已死待收尸必须判已退出（旧口径会白等满预算后报杀失败）"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1000),
        "判定要立刻出结论，不能把预算耗光：{:?}",
        started.elapsed()
    );
    child.wait().unwrap();
}

/// 反向回归：忽略 SIGTERM 的进程不得被报成「已退出」。
///
/// 旧口径（判定取反）会对**活着的**进程第一个 tick 就返 Ok(true)——刚好把这一类
/// 错报成杀成功，而调用方会把台账条目删掉，孤儿从此失联。
/// 已死待收尸（zombie）的自家条目必须归 Dead（清账），不得归 Foreign（绝不动）。
///
/// zombie 的 `/proc/<pid>/environ` 不可读 → 标记读不到 → 旧口径（只看 /proc 存在性）
/// 把自家死孤儿归 Foreign：条目永久保留、孤儿永不清理。收尸由内核完成，
/// 「environ 读不到」不等于「不是我们的」。
#[test]
fn a_zombie_entry_is_dead_not_foreign() {
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
    // 直接 SIGKILL 且**不收尸**：子进程变 zombie，/proc 条目还在
    child.kill().unwrap();
    let mut zombied = false;
    for _ in 0..100 {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && stat.rsplit_once(')').is_some_and(|(_, r)| r.trim_start().starts_with('Z'))
        {
            zombied = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(zombied, "子进程应已退出待收尸");

    let (killed, cleared, foreign) = reconcile_all(plans.path());
    assert_eq!(
        (killed, cleared, foreign),
        (0, 1, 0),
        "zombie 条目必须归 Dead 清账，不得当 Foreign 永久保留"
    );
    assert!(SpawnLedger::load(&path).unwrap().entries.is_empty());
    child.wait().unwrap();
}

#[test]
fn a_process_that_ignores_term_is_not_reported_as_killed() {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("trap '' TERM; sleep 30")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    // 等 trap 装上（否则 SIGTERM 会落在 exec 之前，测的就变成默认处置）
    std::thread::sleep(std::time::Duration::from_millis(300));
    let pid = child.id();
    assert!(
        !kill_orphan_wait_within(pid, 300).unwrap(),
        "忽略 SIGTERM 的活进程不得报成已退出"
    );
    assert!(process_alive(pid), "它确实还活着");
    child.kill().unwrap();
    child.wait().unwrap();
}

