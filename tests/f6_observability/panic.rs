//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, fixture::*, guards::*, milestone::*, runlog::*, support::*};

/// panic hook 是进程级全局 → 这组用例串行跑。
pub(super) fn panic_hook_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
pub(super) fn a_panic_leaves_a_tombstone_with_the_text_the_location_and_the_phase() {
    // 错实现形态：panic 后没有墓碑 / 墓碑缺 panic 原文、位置或阶段 → 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-tombstone";
    obs::begin_run_in(root.clone(), run_id, "coding");
    obs::set_step("review:change-coding-review");
    let original = std::panic::take_hook();
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-panic-text");
    });
    handle.restore();
    std::panic::set_hook(original);
    obs::end_run(run_id);
    assert!(result.is_err(), "panic 应被捕获（不 abort）");
    let tombs: Vec<PathBuf> = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(tombs.len(), 1, "应恰好一个墓碑文件：{tombs:?}");
    assert!(
        tombs[0].file_name().unwrap().to_string_lossy().contains(run_id),
        "墓碑文件名要含 run id：{tombs:?}"
    );
    let text = std::fs::read_to_string(&tombs[0]).expect("读墓碑");
    assert!(text.contains("f6-boom-panic-text"), "墓碑要含 panic 原文：{text}");
    assert!(text.contains("location: "), "墓碑要含位置行：{text}");
    assert!(
        text.contains("panic.rs:"),
        "位置要指向 panic 现场（file:line）：{text}"
    );
    assert!(text.contains("phase: coding"), "墓碑要含当时阶段：{text}");
    assert!(
        text.contains("step: review:change-coding-review"),
        "墓碑要含当时步骤：{text}"
    );
}

#[test]
pub(super) fn a_panic_without_an_active_run_still_leaves_a_tombstone() {
    // 错实现形态：无活动 run 时不写墓碑（证据丢失）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let original = std::panic::take_hook();
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-no-run");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err());
    let tombs: Vec<PathBuf> = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(tombs.len(), 1, "无活动 run 也要留墓碑：{tombs:?}");
    let text = std::fs::read_to_string(&tombs[0]).expect("读墓碑");
    assert!(
        text.contains(&format!("run_id: {}", obs::tombstone::NO_RUN)),
        "无活动 run 时用占位 run id：{text}"
    );
    assert!(text.contains("f6-boom-no-run"), "墓碑要含 panic 原文：{text}");
}

#[test]
pub(super) fn the_previous_panic_hook_is_still_called() {
    // 错实现形态：装了新 hook 就把上一个 hook 顶掉（默认 stderr 输出丢）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let original = std::panic::take_hook();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    std::panic::set_hook(Box::new(move |_| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-chain");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err());
    assert!(
        called.load(std::sync::atomic::Ordering::SeqCst),
        "上一个 hook 必须被链回调用（链回语义）"
    );
    let tombs = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .count();
    assert_eq!(tombs, 1, "链回的同时也要写墓碑");
}

#[test]
pub(super) fn an_unwritable_root_does_not_abort_and_still_chains_the_hook() {
    // 错实现形态：写不进去时在 hook 里 panic（= abort，连 stderr 链回都丢）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let not_a_dir = dir.path().join("not-a-dir");
    std::fs::write(&not_a_dir, b"x").expect("造一个不可写的落点");
    let original = std::panic::take_hook();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    std::panic::set_hook(Box::new(move |_| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    let handle = obs::tombstone::install_hook(not_a_dir.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-unwritable");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err(), "panic 仍应正常传播（不被 hook 吞掉）");
    assert!(
        called.load(std::sync::atomic::Ordering::SeqCst),
        "写失败也必须链回上一个 hook"
    );
    assert!(
        !not_a_dir.join("tombstones").exists(),
        "写不进去时不应留下半成品目录"
    );
}

// ============================================================ 需求 3：run 心跳

#[test]
pub(super) fn the_heartbeat_follows_the_phase_and_carries_phase_step_and_timestamp() {
    // 错实现形态：阶段推进不更新心跳（文件内容不变）→ 死后答不出「死在哪个阶段」
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-heartbeat";
    obs::begin_run_in(root.clone(), run_id, "plan");
    let before = obs::heartbeat::read(&root, run_id).expect("心跳应已落盘");
    obs::set_phase("coding");
    obs::set_step("fix:plan:1");
    let after = obs::heartbeat::read(&root, run_id).expect("心跳应仍在");
    obs::end_run(run_id);
    assert_eq!(before.phase, "plan");
    assert_eq!(after.phase, "coding", "阶段推进后心跳内容必须随之变化");
    assert_eq!(after.step, "fix:plan:1", "心跳要带步骤");
    assert!(after.timestamp_ms > 0, "心跳要带时间戳");
    assert_eq!(after.run_id, run_id);
}

#[test]
pub(super) fn the_heartbeat_is_refreshed_by_the_watchdog_for_every_sop_including_change() {
    // 错实现形态：①低频兜底不刷新心跳（长时间无事件时心跳陈旧 → 「卡死」与「已死」分不清）
    //   ②某个 SOP 的入口不经过 watchdog（长 agent 轮里心跳陈旧）
    //   ③watchdog 线程不刷心跳 → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-touch";
    obs::begin_run_in(root.clone(), run_id, "plan");
    let before = obs::heartbeat::read(&root, run_id).expect("心跳应已落盘");
    std::thread::sleep(Duration::from_millis(1100));
    obs::touch_run(run_id);
    let after = obs::heartbeat::read(&root, run_id).expect("心跳应仍在");
    obs::end_run(run_id);
    assert!(
        after.timestamp_ms > before.timestamp_ms,
        "低频兜底刷新生效：before={} after={}",
        before.timestamp_ms,
        after.timestamp_ms
    );
    assert_eq!(after.phase, "plan", "兜底刷新不改阶段");

    // 兜底刷新要覆盖**六个 SOP**（change 也不例外）
    let server = std::fs::read_to_string(repo_root().join("src/engine/builtin.rs")).expect("读 server");
    let doc_wiring =
        std::fs::read_to_string(repo_root().join("src/sop/doc/wiring.rs")).expect("读接线");
    let change =
        std::fs::read_to_string(repo_root().join("src/server/change_engine.rs")).expect("读引擎");
    for (name, text) in [
        ("fn run_planning", server.as_str()),
        ("fn run_coding", server.as_str()),
        ("fn run_doc_from_zero", doc_wiring.as_str()),
        ("fn run_testing", server.as_str()),
        ("fn run_doc_fix", server.as_str()),
        ("fn run_change", change.as_str()),
    ] {
        let body = fn_body(text, name)
            .unwrap_or_else(|| panic!("源码里应能找到 {name}：心跳兜底要覆盖到它"));
        assert!(
            body.contains("run_with_watchdog("),
            "{name} 必须经过 watchdog（否则长 agent 轮里心跳陈旧）：{body}"
        );
    }
    // watchdog 线程本身就是心跳低频兜底的挂载点（ARC-R2：定义已迁 observability/watchdog.rs）
    let watchdog_src = std::fs::read_to_string(repo_root().join("src/observability/watchdog.rs"))
        .expect("读 watchdog");
    let watchdog = fn_body(&watchdog_src, "fn run_with_watchdog")
        .expect("源码里应能找到 run_with_watchdog（watchdog 是兜底刷新的挂载点）");
    assert!(
        watchdog.contains("touch_run("),
        "watchdog 线程要刷心跳（区分「卡死」与「已死」）：{watchdog}"
    );
}

