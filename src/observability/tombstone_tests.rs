//! （2026-10-26 测试分离：自 src/observability/tombstone.rs 内联平移，纯移动零改动。）
use super::*;

/// 三个 hook 用例都要替换**进程级全局** panic hook（R6 风险成真）：
/// 并行测试线程互相踩（resume6h 基线取证：20 连跑红 4——panic 逃出
/// catch_unwind / 读到兄弟用例墓碑 / 链回 flag 被换）→ 互斥串行化，
/// 调度不再影响断言。
static HOOK_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hook_guard() -> std::sync::MutexGuard<'static, ()> {
    HOOK_TESTS.lock().unwrap_or_else(|p| p.into_inner())
}

/// 装 hook → 触发一次 panic → 还原 hook → 查墓碑文件。
/// （hook 是进程级全局，故本组用例合并在一个 #[test] 里，避免与其它用例并发互扰。）
#[test]
fn tombstone_records_panic_text_and_location_and_chains_the_previous_hook() {
    let _guard = hook_guard();
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    // 先装一个「上一个 hook」：它的副作用（置位）必须仍然发生 → 证链回语义
    let original = std::panic::take_hook();
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    std::panic::set_hook(Box::new(move |_| {
        flag.store(true, Ordering::SeqCst);
    }));
    let handle = install_hook(dir.path().to_path_buf());
    let result = std::panic::catch_unwind(|| {
        panic!("boom-{}", 42);
    });
    assert!(result.is_err(), "panic 应被 catch_unwind 捕获");
    assert!(
        called.load(Ordering::SeqCst),
        "上一个 hook 必须被链回调用（链回语义）"
    );
    // 还原上一个 hook，再还原最初的默认 hook（不留进程级副作用）
    handle.restore();
    std::panic::set_hook(original);

    let dir_entries: Vec<PathBuf> = std::fs::read_dir(dir.path().join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(dir_entries.len(), 1, "应恰好一个墓碑文件：{dir_entries:?}");
    let text = std::fs::read_to_string(&dir_entries[0]).unwrap();
    assert!(text.contains("boom-42"), "墓碑要含 panic 原文：{text}");
    let loc = text
        .lines()
        .find_map(|l| l.strip_prefix("location: "))
        .unwrap_or_else(|| panic!("墓碑要含位置行：{text}"));
    assert!(
        loc.contains("tombstone_tests.rs:"),
        "位置要指向 panic 现场的文件（file:line，panic 在本测试文件内触发）：{loc}"
    );
    let line_no: u32 = loc
        .rsplit(':')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("位置要带行号：{loc}"));
    assert!(line_no > 0, "行号必须是真的：{loc}");
    assert!(text.contains("run_id: "), "墓碑要含 run id：{text}");
    assert!(text.contains("timestamp_ms: "), "墓碑要含时间戳：{text}");
}

#[test]
fn tombstone_is_skipped_when_the_root_cannot_be_written() {
    let _guard = hook_guard();
    // 错实现形态：写不进去时在 hook 里 panic（= abort，连 stderr 链回都丢）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let unwritable = dir.path().join("not-a-dir");
    std::fs::write(&unwritable, b"x").unwrap();
    let original = std::panic::take_hook();
    let handle = install_hook(unwritable.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("boom-unwritable");
    });
    assert!(result.is_err(), "panic 仍应正常传播（不被 hook 吞掉）");
    handle.restore();
    std::panic::set_hook(original);
    // 降级：没有墓碑文件，但进程没 abort、panic 照常传播
    assert!(
        !unwritable.join("tombstones").exists(),
        "写不进去时不应留下半成品目录"
    );
}

#[test]
fn tombstone_uses_a_placeholder_run_id_when_no_run_is_active() {
    let _guard = hook_guard();
    // 错实现形态：无活动 run 时不写墓碑（证据丢失）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    // 本线程没有活动 run（线程级归属）→ 墓碑用占位 run id
    let original = std::panic::take_hook();
    let handle = install_hook(dir.path().to_path_buf());
    let result = std::panic::catch_unwind(|| {
        panic!("boom-no-run");
    });
    assert!(result.is_err());
    handle.restore();
    std::panic::set_hook(original);
    let dir_entries: Vec<PathBuf> = std::fs::read_dir(dir.path().join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(dir_entries.len(), 1, "无活动 run 也要留墓碑：{dir_entries:?}");
    let text = std::fs::read_to_string(&dir_entries[0]).unwrap();
    // 自审 minor（80）后 tombstone 会回落进程级快照表：并行测试里若有别的 run 活着，
    // 归因到它也是**正确**行为（非 run 线程崩溃不再丢归属）。这里钉的是「必有 run_id 行」：
    // 要么是本线程无 run 的占位值，要么是进程里那个活动 run（不得为空串）。
    let line = text
        .lines()
        .find(|l| l.starts_with("run_id: "))
        .expect("墓碑必须有 run_id 行");
    let value = line.trim_start_matches("run_id: ").trim();
    assert!(
        !value.is_empty(),
        "run_id 行不得为空（占位 {NO_RUN} 或活动 run）：{text}"
    );
}

#[test]
fn tombstone_path_shape_is_timestamp_and_run_id() {
    let path = tombstone_path(Path::new("/root"), 1_700_000_000_000, "run-9");
    assert_eq!(
        path,
        PathBuf::from("/root/tombstones/1700000000000-run-9.txt"),
        "墓碑文件名必须含时间戳与 run id"
    );
}

/// 自审 minor（80）回归：panic hook 在**非 run 线程**上也要能归因到活动 run。
/// 旧口径 `current()` 是线程本地 → 别的线程崩溃时墓碑退化成 no-run + 空 phase/step。
#[test]
fn tombstone_attributes_the_active_run_from_a_foreign_thread() {
    let _guard = hook_guard();
    let id = "run-foreign-thread";
    let obs = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    super::super::begin_run_in(obs.path().to_path_buf(), id, "testing");
    super::super::set_step("review:change-test-review");

    let original = std::panic::take_hook();
    let handle = install_hook(out.path().to_path_buf());
    // panic 发生在另一个线程（该线程没有 thread-local run 上下文）
    std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(|| panic!("boom-foreign"));
    })
    .join()
    .unwrap();
    handle.restore();
    std::panic::set_hook(original);

    let entries: Vec<PathBuf> = std::fs::read_dir(out.path().join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(entries.len(), 1, "非 run 线程也要留墓碑：{entries:?}");
    let text = std::fs::read_to_string(&entries[0]).unwrap();
    // 本用例钉的是**不退化**：非 run 线程崩了也得归因到某个活动 run（不得是占位 no-run）。
    // 不断言「就是本用例的 id」：全量并行时兄弟用例也会 `begin_run_in`，而
    // `current_process_wide` 的既定口径是「最近开跑者」——那条 heuristic 本身在
    // `prefers_the_latest_started_run` 里单独钉，混在这里断言等于让并行调度决定结果。
    assert!(
        !text.contains(&format!("run_id: {NO_RUN}")),
        "非 run 线程的墓碑退化成 no-run（丢失 run 归属）：{text}"
    );
    let attributed = text
        .lines()
        .find_map(|l| l.strip_prefix("run_id: "))
        .map(str::trim)
        .unwrap_or_default();
    assert!(!attributed.is_empty(), "run_id 行不得为空：{text}");
    super::super::end_run(id);
}

/// `current_process_wide` 的决胜规则：最近开跑者胜，**毫秒同值时按 run id 定序**——
/// 旧口径只比 `started_at_ms`，同值时就靠 HashMap 迭代顺序决定，同一场景两次跑可能不同。
/// 直接测纯函数，不碰进程级快照表（那边并行用例会同时改）。
#[test]
fn later_than_breaks_ties_by_run_id() {
    let snap = |id: &str, ms: u64| super::super::RunSnapshot {
        run_id: id.into(),
        started_at_ms: ms,
        ..Default::default()
    };
    assert!(super::super::later_than(&snap("b", 10), &snap("a", 5)), "后开跑的胜");
    assert!(!super::super::later_than(&snap("a", 5), &snap("b", 10)), "先开跑的不胜");
    assert!(
        super::super::later_than(&snap("run-b", 7), &snap("run-a", 7)),
        "毫秒同值必须按 run id 定序（否则归属看 HashMap 迭代顺序）"
    );
    assert!(
        super::super::later_than(&snap("run-a", 7), &snap("run-a", 7)),
        "同一条比较为真（fold 用 >=，不丢当前项）"
    );
}
