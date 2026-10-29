//! HTTP 面（ARC-R2 自 server/mod.rs 迁入）：token 解析（P3：env + CLI 双入口）+ 路由表 + 生产引导。

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::server::handlers;
use crate::server::SharedState;

/// token 的环境变量名（自审 minor 46：契约不再被劈成两半——旧口径本文件管优先级与解析、
/// `DEVFORGE_TOKEN` 却由 main.rs 独家持有，读本文件看不到配置入口全貌）。
pub const TOKEN_ENV: &str = "DEVFORGE_TOKEN";

/// 从进程环境读 token（`TOKEN_ENV`）。
pub fn token_from_env() -> Option<String> {
    std::env::var(TOKEN_ENV).ok()
}

/// 从启动参数解析 `--token <v>` / `--token=<v>`（首个命中生效；无则 None）。
/// 自审 major：空格形的下一个参数若以 `--` 开头，视为缺值——否则 `--token --port`
/// 会把 `--port` 当密钥吞掉（公开常量成了实际生效的共享密钥，还顶掉 env 里的真 token）。
pub fn parse_token_flag(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--token=") {
            return Some(v.to_string());
        }
        if a == "--token" {
            match iter.next() {
                Some(v) if !v.starts_with("--") => return Some(v.clone()),
                _ => return None, // 缺值或紧跟另一个 flag：不猜，交给 env
            }
        }
    }
    None
}

/// 启动 token 解析：CLI > env > 无；空串视为未设置（防空 token 锁死）。
/// 自审 major：返回值归一化 trim——「 abc 」当密钥收下但客户端只发 abc，
/// 服务永久 401 且启动期无任何报错。
pub fn resolve_startup_token(cli: Option<String>, env: Option<String>) -> Option<String> {
    // 自审 minor（215）：CLI 来的密钥会进进程 argv（/proc/<pid>/cmdline、ps、shell history
    // 全可见），暴露面比 env 大。不阻止（运维便利），但必须大声提示。
    if cli.as_deref().is_some_and(|v| !v.trim().is_empty()) && env.is_some() {
        tracing::warn!(
            component = "server",
            event = "auth.token_source",
            source = "argv",
            "token 取自命令行参数（会出现在 ps / /proc/<pid>/cmdline 里）——生产环境建议改用 {TOKEN_ENV}"
        );
    }
    let normalized = |s: String| {
        let t = s.trim().to_string();
        (!t.is_empty()).then_some(t)
    };
    cli.and_then(normalized).or_else(|| env.and_then(normalized))
}

pub fn build_router(state: SharedState) -> Router {
    // 自审 minor（47）：路由表按四个域分段（编排控制 / 产物管理+可观测 / 交互中继 / 配置管理），
    // 缩进统一——旧口径被上一次提交插坏了缩进，分段结构读不出来。
    let api = Router::new()
        // ── 编排控制 ──
        .route(
            "/projects",
            post(handlers::create_project).get(handlers::list_projects),
        )
        .route("/projects/{name}", get(handlers::get_project))
        .route(
            "/projects/{name}/tasks",
            post(handlers::create_task).get(handlers::list_tasks),
        )
        .route(
            "/projects/{name}/tasks/{task}/start",
            post(handlers::start_task),
        )
        .route(
            "/projects/{name}/tasks/{task}/cancel",
            post(handlers::cancel_task),
        )
        // ── 产物管理 + 可观测 ──
        .route("/projects/{name}/tasks/{task}/runs", get(handlers::list_runs))
        .route(
            "/projects/{name}/integration/queue",
            post(handlers::enqueue_candidate).get(handlers::get_integration_queue),
        )
        .route(
            "/projects/{name}/tasks/{task}/runs/{run}",
            get(handlers::get_run_detail),
        )
        .route(
            "/projects/{name}/tasks/{task}/runs/{run}/events",
            get(handlers::run_events),
        )
        .route(
            "/projects/{name}/tasks/{task}/runs/{run}/transcript",
            get(handlers::run_transcript),
        )
        .route(
            "/projects/{name}/tasks/{task}/graph",
            get(handlers::task_graph),
        )
        .route(
            "/projects/{name}/tasks/{task}/audit-report",
            get(handlers::audit_report),
        )
        .route("/scheduler/status", get(handlers::scheduler_status))
        // ── 交互中继 ──
        .route("/ui/pending", get(handlers::list_pending_ui))
        .route("/ui/{id}/answer", post(handlers::answer_ui))
        .route("/events", get(handlers::events))
        // ── 配置管理 ──
        .route(
            "/config/manifest",
            get(handlers::get_manifest).put(handlers::put_manifest),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::auth_middleware,
        ));
    Router::new().nest("/api/v1", api).with_state(state)
}

/// 生产引导：绑定地址并常驻服务（main 调用）。
pub async fn serve(state: SharedState, addr: std::net::SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // 自审 minor（217）：启动期关键状态迁移落事件——鉴权姿态（是否启用、来源）与调度器启动。
    // 「就绪」在 bind 成功、即将 accept 时打点（不再是 bind 之前）。
    tracing::info!(
        component = "server",
        event = "server.listening",
        %addr,
        "devforge API 已绑定端口，开始 accept（启动阶段完成）"
    );
    // P012 M4：调度循环常驻（30s 周期 + run 收尾 kick 即时）。
    // 自审 major：JoinHandle 不再裸丢——调度循环只会在 panic/异常时结束，
    // 静默丢弃 = 队列永不派工而 HTTP 面看起来健康。监督任务负责留痕。
    let scheduler = tokio::spawn(crate::server::scheduler::scheduler_loop(
        state.clone(),
        crate::server::scheduler::SchedulerConfig::default(),
    ));
    tracing::info!(
        component = "server",
        event = "scheduler.started",
        "调度循环已启动（1s 心跳看 kick / 30s 周期全量 tick）"
    );
    tokio::spawn(async move {
        if let Err(err) = scheduler.await {
            tracing::error!(
                component = "server",
                error = %err,
                "调度循环已退出（panic 或异常）——队列不再派工，需重启进程"
            );
        }
    });
    // 自审 minor（218）：常驻服务要有优雅停机——旧口径 SIGINT 直接终结进程，SSE 长连接被
    // 硬断且不留停机日志，运维无法区分「主动停」与「崩了」。
    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!(
                component = "server",
                event = "server.shutdown",
                reason = "SIGINT",
                "收到 SIGINT，开始优雅停机（停止接受新连接，等在途请求收尾）"
            );
        })
        .await?;
    tracing::info!(
        component = "server",
        event = "server.stopped",
        "devforge API 已停机"
    );
    Ok(())
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod startup_token_tests;
