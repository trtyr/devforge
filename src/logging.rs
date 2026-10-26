//! 结构化日志（architecture-principles §2：完整日志系统）。
//!
//! 设计要点：
//! - JSON 结构化层写文件（`~/.devforge/logs/`），人类可读层写 stderr——现场给人，落盘给机器
//! - 级别可配：`RUST_LOG` 环境变量优先，默认 `info`
//! - [`init`] 幂等：全局 subscriber 已被占用时静默跳过（重复初始化不是错误）

use std::path::{Path, PathBuf};

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::error::{Context as _, Result};

/// 日志文件名约定：`<component>.log.jsonl`。
pub fn log_file_path(log_dir: &Path, component: &str) -> PathBuf {
    log_dir.join(format!("{component}.log.jsonl"))
}

/// 打开（或创建）追加模式的 JSONL 日志文件。
pub fn open_log_file(log_dir: &Path, component: &str) -> Result<std::fs::File> {
    std::fs::create_dir_all(log_dir).ctx("创建日志目录")?;
    let path = log_file_path(log_dir, component);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ctx(format!("打开日志文件 {}", path.display()))
}

/// 初始化全局日志：JSON 文件层 + stderr 人类层。
///
/// 幂等：全局 subscriber 已被设置时跳过（比如测试环境重复调用）。
pub fn init(log_dir: &Path, component: &str) -> Result<()> {
    let file = open_log_file(log_dir, component)?;

    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .with_writer(std::sync::Mutex::new(file));

    let human_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_writer(std::io::stderr);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 幂等：已被占用则跳过（不影响本次进程内已有的 subscriber）
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(json_layer)
        .with(human_layer)
        .try_init();
    Ok(())
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;
