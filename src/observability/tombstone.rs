//! 死亡取证：panic hook + 墓碑文件（`root/tombstones/<ts>-<runid>.txt`）。
//!
//! 纪律（Q9/R20）：
//! - 写入 **best-effort**：写失败只记一行 stderr，**绝不**在 hook 里 panic
//!   （panic 中 panic = abort，连 stderr 链回都丢 → 死亡取证反变成死亡无证）；
//! - 无论成败都**链回**上一个 hook（默认 stderr 输出不丢）；
//! - HOME 未设 / root 不可写 / 磁盘满 → 降级（不写墓碑、不 panic、照常链回）。
//!
//! 落点：`root/tombstones/<时间戳毫秒>-<runid>.txt`（无活动 run 时 run id 用占位值 `no-run`）。

use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 上一个 panic hook（进程级全局，只有一个）。
pub type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send>;

/// 无活动 run 时的 run id 占位值（证据不丢，只是不带 run 归属）。
pub const NO_RUN: &str = "no-run";

/// 已安装的 hook 的句柄：持有上一个 hook（单测装完要还原——hook 是进程级全局）。
pub struct HookHandle {
    previous: Arc<Mutex<Option<PanicHook>>>,
}

impl HookHandle {
    /// 还原上一个 hook（把当前 hook 换回上一个）。
    pub fn restore(self) {
        if let Some(p) = self
            .previous
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
        {
            std::panic::set_hook(p);
        }
    }
}

/// 安装 panic hook：panic 时写墓碑到 `root`，然后**链回**上一个 hook。
///
/// `root` = 观测根（由调用方解析好；写不进去 → 降级，不 panic）。
/// 返回句柄（装完要还原时用 [`HookHandle::restore`]）。
pub fn install_hook(root: std::path::PathBuf) -> HookHandle {
    let previous = std::panic::take_hook();
    let slot = Arc::new(Mutex::new(Some(previous)));
    let chained = slot.clone();
    std::panic::set_hook(Box::new(move |info| {
        // 墓碑写入永不 panic：失败只记一行 stderr（catch_unwind 兜底二次 panic）
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Err(e) = write_tombstone(&root, info) {
                eprintln!("[observability] 墓碑写入失败（已降级）：{e}");
            }
        }));
        // 链回上一个 hook（默认 stderr 输出不丢）；已被还原时没有可链的 → 跳过
        if let Ok(guard) = chained.lock()
            && let Some(p) = guard.as_ref()
        {
            p(info);
        }
    }));
    HookHandle { previous: slot }
}

/// 墓碑文件路径：`root/tombstones/<ts>-<runid>.txt`。
pub fn tombstone_path(root: &Path, timestamp_ms: u64, run_id: &str) -> PathBuf {
    root.join("tombstones")
        .join(format!("{timestamp_ms}-{run_id}.txt"))
}

/// 写墓碑：panic 原文 + 位置（file:line）+ 当时的阶段/步骤 + 时间戳 + run id。
pub fn write_tombstone(root: &Path, info: &PanicHookInfo<'_>) -> std::io::Result<PathBuf> {
    let now = crate::domain::now_ms();
    let snap = super::current();
    let run_id = snap
        .as_ref()
        .map(|s| s.run_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| NO_RUN.to_string());
    let phase = snap.as_ref().map(|s| s.phase.clone()).unwrap_or_default();
    let step = snap.as_ref().map(|s| s.step.clone()).unwrap_or_default();
    let dir = root.join("tombstones");
    std::fs::create_dir_all(&dir)?;
    let path = tombstone_path(root, now, &run_id);
    let body = format!(
        "panic: {}\nlocation: {}\nrun_id: {run_id}\nphase: {phase}\nstep: {step}\ntimestamp_ms: {now}\n",
        payload_text(info),
        location_text(info),
    );
    std::fs::write(&path, body)?;
    Ok(path)
}

/// panic 载荷的文本（`&str` / `String` 两种常见形态；其它形态退化为类型名）。
fn payload_text(info: &PanicHookInfo<'_>) -> String {
    let p = info.payload();
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "（非字符串 panic 载荷）".to_string()
    }
}

/// panic 位置（`file:line`；无位置信息时写「未知」）。
fn location_text(info: &PanicHookInfo<'_>) -> String {
    match info.location() {
        Some(loc) => format!("{}:{}", loc.file(), loc.line()),
        None => "未知".to_string(),
    }
}

#[cfg(test)]
mod tests {
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
            loc.contains("tombstone.rs:"),
            "位置要指向 panic 现场的文件（file:line）：{loc}"
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
        assert!(
            text.contains(&format!("run_id: {NO_RUN}")),
            "无活动 run 时用占位 run id：{text}"
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
}
