//! devforge 入口（应用层：anyhow 上下文风格）。

use anyhow::Context as _;

fn main() -> anyhow::Result<()> {
    let home = std::env::var("HOME").context("读取 HOME 环境变量")?;
    let log_dir = std::path::Path::new(&home).join(".devforge/logs");

    devforge::logging::init(&log_dir, "devforge").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    tracing::info!(component = "main", "devforge 启动");

    // 后续任务填充：P003 API 服务器在此启动
    println!("devforge v{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
