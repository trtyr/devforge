//! HTTP 面（ARC-R2 自 server/mod.rs 迁入）：token 解析（P3：env + CLI 双入口）+ 路由表 + 生产引导。

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::server::handlers;
use crate::server::SharedState;

/// 从启动参数解析 `--token <v>` / `--token=<v>`（首个命中生效；无则 None）。
pub fn parse_token_flag(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--token=") {
            return Some(v.to_string());
        }
        if a == "--token" {
            return iter.next().cloned();
        }
    }
    None
}

/// 启动 token 解析：CLI > env > 无；空串视为未设置（防空 token 锁死）。
pub fn resolve_startup_token(cli: Option<String>, env: Option<String>) -> Option<String> {
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    cli.and_then(non_empty).or_else(|| env.and_then(non_empty))
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
    axum::serve(listener, build_router(state)).await
}

#[cfg(test)]
mod startup_token_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn token_flag_space_form() {
        assert_eq!(parse_token_flag(&args(&["--token", "abc"])), Some("abc".into()));
    }

    #[test]
    fn token_flag_eq_form() {
        assert_eq!(parse_token_flag(&args(&["--token=xyz"])), Some("xyz".into()));
    }

    #[test]
    fn token_flag_absent() {
        assert_eq!(parse_token_flag(&args(&["--port", "1"])), None);
        assert_eq!(parse_token_flag(&[]), None);
    }

    #[test]
    fn token_flag_dangling_value_is_none() {
        // `--token` 无后继值 → None（不 panic）
        assert_eq!(parse_token_flag(&args(&["--token"])), None);
    }

    #[test]
    fn startup_token_cli_wins_over_env() {
        assert_eq!(
            resolve_startup_token(Some("cli".into()), Some("env".into())),
            Some("cli".into())
        );
    }

    #[test]
    fn startup_token_env_used_when_no_cli() {
        assert_eq!(
            resolve_startup_token(None, Some("env".into())),
            Some("env".into())
        );
    }

    #[test]
    fn startup_token_empty_is_unset() {
        // 空串/纯空白视为未设置——防空 token 把服务锁死
        assert_eq!(resolve_startup_token(Some("  ".into()), None), None);
        assert_eq!(resolve_startup_token(None, Some("".into())), None);
        assert_eq!(resolve_startup_token(None, None), None);
    }
}
