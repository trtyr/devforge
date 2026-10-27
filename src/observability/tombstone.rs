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
    // 自审 minor（80）：panic hook 跑在崩掉的那个线程上，`current()` 是线程本地口径 →
    // 非 run 线程（worker/审查子会话）崩溃时归因退化成 no-run + 空 phase/step。改用进程级
    // 快照表（唯一活动 run / 最近开跑者），线程本地有值时才优先它。
    let snap = super::current().or_else(super::current_process_wide);
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
#[path = "tombstone_tests.rs"]
mod tests;
