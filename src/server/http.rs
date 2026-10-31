//! HTTP 面（ARC-R2 自 server/mod.rs 迁入）：token 解析（P3：env + CLI 双入口）+ 路由表 + 生产引导。

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::server::handlers;
use crate::server::SharedState;

/// 从启动参数解析 `--<name> <v>` / `--<name>=<v>`（首个命中生效；无则 None）。
/// F14/T205 启动三件套（--home/--addr/--token）共用的旗标语法契约。
/// 自审 major：空格形的下一个参数若以 `--` 开头，视为缺值——否则 `--token --port`
/// 会把 `--port` 当值吞掉（公开旗标成了实际生效的配置载体）。
pub fn parse_cli_flag(args: &[String], name: &str) -> Option<String> {
    let with_eq = format!("{name}=");
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix(&with_eq) {
            return Some(v.to_string());
        }
        if a == name {
            match iter.next() {
                Some(v) if !v.starts_with("--") => return Some(v.clone()),
                _ => return None, // 缺值或紧跟另一个 flag：不猜，交给下一层承载
            }
        }
    }
    None
}

/// 启动 token 解析：CLI > config.json > 无；空串视为未设置（防空 token 锁死）。
/// 自审 major：返回值归一化 trim——「 abc 」当密钥收下但客户端只发 abc，
/// 服务永久 401 且启动期无任何报错。
pub fn resolve_startup_token(cli: Option<String>, config: Option<String>) -> Option<String> {
    // 自审 minor（215）：CLI 来的密钥会进进程 argv（/proc/<pid>/cmdline、ps、shell history
    // 全可见），暴露面比 config.json 大。不阻止（运维便利），但必须大声提示。
    if cli.as_deref().is_some_and(|v| !v.trim().is_empty()) && config.is_some() {
        tracing::warn!(
            component = "server",
            event = "auth.token_source",
            source = "argv",
            "token 取自命令行参数（会出现在 ps / /proc/<pid>/cmdline 里）——生产环境建议改用 config.json 的 token 键"
        );
    }
    let normalized = |s: String| {
        let t = s.trim().to_string();
        (!t.is_empty()).then_some(t)
    };
    cli.and_then(normalized).or_else(|| config.and_then(normalized))
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
        .route(
            "/projects/{name}",
            get(handlers::get_project).delete(handlers::delete_project),
        )
        .route(
            "/projects/{name}/archive",
            post(handlers::archive_project),
        )
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
        // ── 通知（A2）──
        .route(
            "/notifications",
            get(handlers::list_notifications),
        )
        .route(
            "/notifications/read",
            post(handlers::read_notifications),
        )
        // ── 交互中继 ──
        .route("/ui/pending", get(handlers::list_pending_ui))
        .route("/ui/{id}/answer", post(handlers::answer_ui))
        .route("/events", get(handlers::events))
        // ── 配置管理 ──
        .route(
            "/config/manifest",
            get(handlers::get_manifest).put(handlers::put_manifest),
        )
        .route("/config/knobs", get(handlers::knob_effective))
        .route("/server/ops", get(handlers::server_ops))
        .route("/config/agents", get(handlers::agents_config))
        .route("/stats", get(handlers::stats))
        .route("/logs", get(handlers::logs_query))
        .route("/sops", get(handlers::sops_list))
        .route("/dependencies", get(handlers::dependencies_list))
        .route(
            "/integrations/settings",
            get(handlers::get_integration_settings).put(handlers::put_integration_settings),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::auth_middleware,
        ));
    Router::new()
        .nest("/api/v1", api)
        // A1：登录/登出在守门之外（/api/v1 守门后鸡生蛋）；鉴权由 login 自身负责。
        .merge(
            Router::new()
                .route("/auth/login", post(handlers::auth_login))
                .route("/auth/logout", post(handlers::auth_logout))
                .with_state(state.clone()),
        )
        .with_state(state)
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
    // A1：ConnectInfo 注入——中间件用真实对端地址判回环豁免（oneshot 测试无此层，
    // Option<ConnectInfo> = None 视作本机）。
    axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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
