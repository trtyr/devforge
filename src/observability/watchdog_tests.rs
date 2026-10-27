//! （2026-10-26 测试分离：自 src/observability/watchdog.rs 内联平移，纯移动零改动。）
use super::*;
use std::time::{Duration, Instant};

#[test]
fn watchdog_scope_returns_after_closure_completes() {
    // 回归（verdict 后挂起）：f 正常返回后 scope join 必须解锁（旧实现此处死锁）
    let control = RunControl::new();
    let start = Instant::now();
    let out = run_with_watchdog(&control, "watchdog-regression".into(), None, vec![], None, None, || {
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
    // 错实现形态：watchdog 不刷心跳 → 长 agent 轮里心跳陈旧 →「卡死」与「已死」分不清。
    // 2026-10-27 自审 major（空洞修复）：旧口径取样点在 run **结束后**——收尾终态刷盘
    // 照样把 after 抬高，删掉周期 tick 也绿。现在在 run **进行中**（f 内部）取样：
    // 周期 tick 被删 → mid == before → 红。
    let dir = tempfile::tempdir().unwrap();
    let run_id = "watchdog-heartbeat";
    // 观测根按 run 注入到临时目录（不抢进程级全局 → 与并行单测不互相干扰）
    crate::observability::begin_run_in(dir.path().to_path_buf(), run_id, "plan");
    let before = crate::observability::heartbeat::read(dir.path(), run_id)
        .expect("心跳应已落盘")
        .timestamp_ms;
    let control = RunControl::new();
    // 停 1.6s：心跳节流 1s → watchdog 的周期 tick（~1.2s）必落在观察窗内；
    // mid 在 run 进行中取样，不落收尾刷盘的账。
    let mid = run_with_watchdog(&control, run_id.into(), None, vec![], None, None, || {
        std::thread::sleep(Duration::from_millis(1600));
        crate::observability::heartbeat::read(dir.path(), run_id)
            .expect("run 进行中心跳应可读")
            .timestamp_ms
    });
    let after = crate::observability::heartbeat::read(dir.path(), run_id)
        .expect("心跳应仍在")
        .timestamp_ms;
    crate::observability::end_run(run_id);
    assert!(
        mid > before,
        "run 进行中心跳必须已被 watchdog tick 刷新：before={before} mid={mid}"
    );
    assert!(after >= mid, "收尾终态刷不早于进行中样本：mid={mid} after={after}");
}

#[test]
fn cancel_path_unblocks_the_run_and_fires_the_intervention_hook() {
    // 2026-10-27 自审 major（空洞修复）：旧用例 f 立即返回 7——取消清场分支整段删掉
    // 也全绿。现在 f 阻塞等 hook 解阻：分支被删 → hook 永不触发 → recv 超时且
    // hooked=false → 红。
    let control = std::sync::Arc::new(RunControl::new());
    let hooked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hooked_h = hooked.clone();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let canceller = {
        let c = control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            c.cancel();
        })
    };
    let start = Instant::now();
    let out = run_with_watchdog(
        &control,
        "watchdog-cancel-real".into(),
        None,
        vec![],
        None,
        Some(std::sync::Arc::new(move |_rid: &str| {
            hooked_h.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(());
        })),
        || {
            // 阻塞直到 hook 解阻；3s 兜底（收不到 = 分支被删）
            let _ = rx.recv_timeout(Duration::from_secs(3));
            7
        },
    );
    canceller.join().unwrap();
    assert_eq!(out, 7);
    assert!(
        hooked.load(std::sync::atomic::Ordering::SeqCst),
        "取消清场必须触发 intervention hook（分支被删此断言红）"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "hook 解阻后 f 应立即收场：elapsed={:?}",
        start.elapsed()
    );
}

// ---------- 失活收割（006 decisions/002：默认不杀，显式闲置阈值才判死） ----------

#[test]
fn session_fingerprint_changes_on_append() {    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    let fp0 = session_fingerprint(std::slice::from_ref(&watch)).unwrap();
    std::fs::write(watch.join("s.jsonl"), b"hello").unwrap();
    let fp1 = session_fingerprint(std::slice::from_ref(&watch)).unwrap();
    assert_ne!(fp0, fp1, "新建文件应改变指纹");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(watch.join("s.jsonl"), b"hello world").unwrap();
    let fp2 = session_fingerprint(&[watch]).unwrap();
    assert_ne!(fp1, fp2, "追加增长应改变指纹");
}

/// 2026-10-27 自审 minor（171）：指纹必须**与目录遍历顺序无关**，且含文件身份。
/// 错实现形态：按 read_dir 产出顺序递推混合（或只哈希长度+mtime 不含路径）→ 红。
#[test]
fn fingerprint_is_order_independent_and_identity_aware() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    // 同内容、同大小的两个文件，交换名字 = 不同快照（含路径身份）
    for (dir, name) in [(&a, "x.jsonl"), (&b, "y.jsonl")] {
        std::fs::write(dir.path().join(name), b"same").unwrap();
    }
    let fa = session_fingerprint(&[a.path().to_path_buf()]).unwrap();
    let fb = session_fingerprint(&[b.path().to_path_buf()]).unwrap();
    assert_ne!(fa, fb, "路径字不同必须得到不同指纹（否则换名＝漏报进展）");
    // 同一目录反复算 = 稳定（顺序无关）
    for _ in 0..5 {
        assert_eq!(
            fa,
            session_fingerprint(&[a.path().to_path_buf()]).unwrap(),
            "同一目录的指纹必须稳定"
        );
    }
}

/// 2026-10-27 自审 minor（172）：符号链接目录必须跟随（否则链接出去的子目录
/// 永远不计入进展 → 假平线 → 活 run 被误收割）。
#[cfg(unix)]
#[test]
fn fingerprint_follows_symlinked_directories() {
    let outer = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();
    std::fs::write(inner.path().join("s.jsonl"), b"hello").unwrap();
    std::os::unix::fs::symlink(inner.path(), outer.path().join("link")).unwrap();
    let fp_before = session_fingerprint(&[outer.path().to_path_buf()]).unwrap();
    std::fs::write(inner.path().join("s.jsonl"), b"hello world").unwrap();
    let fp_after = session_fingerprint(&[outer.path().to_path_buf()]).unwrap();
    assert_ne!(fp_before, fp_after, "链接目录内的增长必须改变指纹");
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
            // 2026-10-27 自审 minor（178）：不能再 `let _ =`——写失败会让「持续增长=活」
            // 的样本静默停长而断言仍绿（空洞化）。
            writeln!(f, "line {i}").expect("增长样本写入必须成功");
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
        None,
        || std::thread::sleep(Duration::from_millis(900)),
    );
    assert!(!control.is_stalled(), "未配置闲置阈值时永不失活收割");
}

/// 2026-10-27 自审 major：监视目录不可读 = 不可观测 → 不得判定失活。
/// 旧口径：指纹停在 FNV 基准值 → 平线误判 → 活 run 被收割（stalled=true）。
#[test]
fn unreadable_watch_dir_disables_idle_harvest() {
    let dir = tempfile::tempdir().unwrap();
    let control = RunControl::new();
    run_with_watchdog(
        &control,
        "watchdog-blind".into(),
        Some(400),
        vec![dir.path().join("never-created")], // 目录不存在 → 不可观测
        None,
        None,
        || std::thread::sleep(Duration::from_millis(1200)),
    );
    assert!(
        !control.is_stalled(),
        "不可观测时不得判定失活（旧口径误杀活 run）"
    );
}

/// 2026-10-27 自审 major：收割后旧实现 return → 心跳/取消/超时监测全掉线。
/// 新口径只停失活判定。验证：收割触发 hook#1，随后 cancel 触发 hook#2——
/// hook 计到 2 = 收割后取消监测仍在岗（旧实现只会有 1 → 红）。
#[test]
fn after_harvest_cancel_is_still_monitored() {
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("coding");
    std::fs::create_dir_all(&watch).unwrap();
    std::fs::write(watch.join("s.jsonl"), b"flat").unwrap(); // 永不增长 → 平线
    let control = std::sync::Arc::new(RunControl::new());
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let canceller = {
        let c = control.clone();
        std::thread::spawn(move || {
            // 收割（idle=300ms，~0.6s 内触发）在先，取消在后
            std::thread::sleep(Duration::from_millis(1000));
            c.cancel();
        })
    };
    let hooked_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hc = hooked_count.clone();
    let out = run_with_watchdog(
        &control,
        "watchdog-post-harvest".into(),
        Some(300),
        vec![watch],
        None,
        Some(std::sync::Arc::new(move |_rid: &str| {
            hc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(());
        })),
        move || {
            let _ = rx.recv_timeout(Duration::from_secs(3)); // hook#1（收割解阻）
            let _ = rx.recv_timeout(Duration::from_secs(3)); // hook#2（取消解阻）
            9
        },
    );
    canceller.join().unwrap();
    assert_eq!(out, 9);
    assert!(control.is_stalled(), "平线应已收割");
    let n = hooked_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        n >= 2,
        "收割后取消监测必须仍在岗（hook#2 由 cancel 触发）；实际 {n}"
    );
}
