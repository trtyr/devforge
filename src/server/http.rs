//! HTTP 面（ARC-R2 自 server/mod.rs 迁入）：token 解析（P3：env + CLI 双入口）+ 路由表 + 生产引导。

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::server::handlers;
use crate::server::SharedState;

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
    let normalized = |s: String| {
        let t = s.trim().to_string();
        (!t.is_empty()).then_some(t)
    };
    cli.and_then(normalized).or_else(|| env.and_then(normalized))
}

pub fn build_router(state: SharedState) -> Router {
    let api = Router::new()
        // 编排控制
        .route("/projects", post(handlers::create_project).get(handlers::list_projects))
        .route("/projects/{name}", get(handlers::get_project))
        .route(
            "/projects/{name}/tasks",
            post(handlers::create_task).get(handlers::list_tasks),
        )
        .route("/projects/{name}/tasks/{task}/start", post(handlers::start_task))
        .route(
            "/projects/{name}/tasks/{task}/cancel",
            post(handlers::cancel_task),
        )
        // 产物管理 + 可观测
        .route(
        "/projects/{name}/tasks/{task}/runs",
        get(handlers::list_runs),
    )
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
        .route("/projects/{name}/tasks/{task}/graph", get(handlers::task_graph))
.route(
    "/projects/{name}/tasks/{task}/audit-report",
    get(handlers::audit_report),
)
.route("/scheduler/status", get(handlers::scheduler_status))
        // 交互中继
        .route("/ui/{id}/answer", post(handlers::answer_ui))
        .route("/events", get(handlers::events))
        // 配置管理
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
    tracing::info!(component = "server", %addr, "devforge API 就绪");
    // P012 M4：调度循环常驻（30s 周期 + run 收尾 kick 即时）。
    // 自审 major：JoinHandle 不再裸丢——调度循环只会在 panic/异常时结束，
    // 静默丢弃 = 队列永不派工而 HTTP 面看起来健康。监督任务负责留痕。
    let scheduler = tokio::spawn(crate::server::scheduler::scheduler_loop(
        state.clone(),
        crate::server::scheduler::SchedulerConfig::default(),
    ));
    tokio::spawn(async move {
        if let Err(err) = scheduler.await {
            tracing::error!(
                component = "server",
                error = %err,
                "调度循环已退出（panic 或异常）——队列不再派工，需重启进程"
            );
        }
    });
    axum::serve(listener, build_router(state)).await
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod startup_token_tests;
