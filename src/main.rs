//! devforge 入口（应用层）：起 API 服务器（P003 服务器形态）。

use anyhow::Context as _;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let home = std::env::var("HOME").context("读取 HOME 环境变量")?;
    let root = std::path::Path::new(&home).join(".devforge");

    devforge::logging::init(&root.join("logs"), "devforge")
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    tracing::info!(component = "main", "devforge 启动");

    // 配置清单：真源落盘 ~/.devforge/manifest.json（无则用内置默认生成）
    let manifest_path = root.join("manifest.json");
    let manifest = if manifest_path.exists() {
        devforge::config::ConfigManifest::load(&manifest_path).map_err(|e| anyhow::anyhow!(e.to_string()))?
    } else {
        let m = devforge::config::ConfigManifest::planning_builtin();
        m.save(&manifest_path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        m
    };

    // 目录约定（decisions/002/006 + 原则 §2/§4）
    let profiles_root = root.join("profiles");
    let plans_root = root.join("plans");
    let sessions_root = root.join("sessions");

    // 生产引擎：规划 SOP 的 RPC 接线
    let (events, _) = tokio::sync::broadcast::channel(256);
    let ui = Arc::new(devforge::server::UiRelay::new(events.clone()));
    let engine_profiles = profiles_root.clone();
    let engine_sessions = sessions_root.clone();
    let engine_ui = ui.clone();
    let engine_factory = Box::new(move || -> Box<dyn devforge::domain::runner::RunEngine + Send> {
        Box::new(devforge::server::PlanningRpcEngine {
            profile_root: engine_profiles.clone(),
            session_root: engine_sessions.clone(),
            ui: engine_ui.clone(),
        })
    });

    let state = devforge::server::ServerState::new(
        devforge::domain::Store::new(),
        devforge::domain::sop::SopRegistry::builtin(),
        manifest,
        engine_factory,
        events,
        ui,
        profiles_root,
        plans_root,
        sessions_root,
    );

    let addr = std::env::var("DEVFORGE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7888".into())
        .parse()
        .context("解析 DEVFORGE_ADDR")?;
    devforge::server::serve(state, addr)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
