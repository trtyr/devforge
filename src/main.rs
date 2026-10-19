//! devforge 入口（应用层）：起 API 服务器（P003 服务器形态）。

use anyhow::Context as _;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // doc-lint：引擎侧确定性门禁（D008/D012）。退出码可判：0 = 通过。
    // 形态：`devforge doc-lint [docs_dir]`，默认检查 `docs/contexts`。
    if args.first().map(String::as_str) == Some("doc-lint") {
        let dir = args
            .get(1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("docs/contexts"));
        let report = devforge::docgen::lint::lint_docs(&dir)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        for v in report.violations() {
            println!("{}", v.render());
        }
        println!("{}", report.summary());
        std::process::exit(if report.passed() { 0 } else { 1 });
    }

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
        let m = devforge::config::ConfigManifest::builtin();
        m.save(&manifest_path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        m
    };

    // 目录约定（decisions/002/006 + 原则 §2/§4）
    let profiles_root = root.join("profiles");
    let plans_root = root.join("plans");
    let sessions_root = root.join("sessions");

    // 生产引擎：规划 SOP 的 RPC 接线
    // 失活收割阈值（006 decisions/002）：DEVFORGE_RUN_IDLE_MS 显式开启（ms，>0），
    // 默认 None = 永不因无进展自动杀 run（零硬限制哲学；慢而活着的 run 不误伤）
    let idle_ms = std::env::var("DEVFORGE_RUN_IDLE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0);
    let bus = std::sync::Arc::new(devforge::server::EventBus::new());
    let ui = Arc::new(devforge::server::UiRelay::new(bus.clone()));
    let engine_profiles = profiles_root.clone();
    let engine_sessions = sessions_root.clone();
    let engine_plans = plans_root.clone();
    let engine_ui = ui.clone();
    let engine_factory = Box::new(move || -> Box<dyn devforge::domain::runner::RunEngine + Send> {
        Box::new(devforge::server::BuiltinRpcEngine {
            profile_root: engine_profiles.clone(),
            session_root: engine_sessions.clone(),
            plans_root: engine_plans.clone(),
            ui: engine_ui.clone(),
            codegraph_bin: std::path::PathBuf::from("codegraph"),
            idle_ms,
        })
    });

    let mut state = devforge::server::ServerState::new(
        devforge::domain::Store::new(),
        devforge::domain::sop::SopRegistry::builtin(),
        manifest,
        engine_factory,
        bus,
        ui,
        std::sync::Arc::new(devforge::config::installer::PiInstallBridge),
        profiles_root,
        plans_root,
        sessions_root,
    );
    // P3-ISSUE-02 修复：真源路径交给状态，PUT 替换时同步落盘
    *state
        .manifest_path
        .lock()
        .expect("manifest_path 锁") = Some(manifest_path);
    // P3-ISSUE-03 修复：token 配置入口（CLI --token > DEVFORGE_TOKEN 环境变量；
    // 未设即放行，Q003）+ run 超时（P8-ISSUE-01）：默认不限时（零硬限制），
    // DEVFORGE_RUN_TIMEOUT_MS 显式开启（两者在 Arc 分享前经 get_mut 注入）
    if let Some(s) = std::sync::Arc::get_mut(&mut state) {
        if let Some(t) = devforge::server::resolve_startup_token(
            devforge::server::parse_token_flag(&args),
            std::env::var("DEVFORGE_TOKEN").ok(),
        ) {
            s.token = Some(t);
        }
        if let Ok(ms) = std::env::var("DEVFORGE_RUN_TIMEOUT_MS")
            && let Ok(n) = ms.parse::<u64>()
            && n > 0
        {
            s.run_timeout_ms = Some(n);
        }
    }

    let addr = std::env::var("DEVFORGE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7888".into())
        .parse()
        .context("解析 DEVFORGE_ADDR")?;
    devforge::server::serve(state, addr)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
