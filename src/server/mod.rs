//! API 服务器（P003 · 服务器形态，非 CLI）。
//!
//! 协议定案（P003 decisions/001）：REST（编排控制/产物/配置/中继输入）+ SSE（事件推送），
//! 路径前缀 `/api/v1` 从第一天；默认绑 127.0.0.1，token 预留位。
//!
//! 五能力域：编排控制 · 产物管理 · 交互中继 · 可观测 · 配置管理。

pub mod handlers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use tokio::sync::broadcast;

use crate::config::ConfigManifest;
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::task::Task;
use crate::domain::run::Run;
use crate::domain::runner::{RunEngine, RunOutcome};
use crate::engine::client::RpcClient;
use crate::engine::process::SpawnSpec;
use crate::error::Result;
use crate::sop::planning::{PlanningLoop, UserLink};
use crate::sop::rpc_bridge::{RpcPlanSession, RpcReviewFactory};

// ---------- UI 中继（交互中继域） ----------

/// ask-user-question 的 API 中继：请求推 SSE，应答经 POST 端点回填。
pub struct UiRelay {
    next_id: AtomicU64,
    requests: broadcast::Sender<String>,
    pending: Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>,
}

impl UiRelay {
    /// 事件通道：ui.request 与 run 事件同流（单一 SSE 通道，Q002 信封设计）。
    pub fn new(events: broadcast::Sender<String>) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            requests: events,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn subscribe_requests(&self) -> broadcast::Receiver<String> {
        self.requests.subscribe()
    }

    /// 阻塞等待用户应答（在引擎的同步线程里调用）。
    pub fn ask_blocking(&self, question: &str, options: &[&str]) -> Result<String> {
        let id = format!("ui-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending
            .lock()
            .expect("pending 锁")
            .insert(id.clone(), tx);
        // 先登记后广播：应答端点在事件可见后仍能命中 pending
        let _ = self.requests.send(
            json!({"type": "ui.request", "id": id, "question": question, "options": options})
                .to_string(),
        );
        rx.recv().map_err(|e| {
            crate::error::DevforgeError::System(format!("等待 UI 应答: {e}"))
        })
    }

    /// 回填应答（POST /ui/{id}/answer 调用）。
    pub fn answer(&self, id: &str, value: impl Into<String>) -> Result<()> {
        let tx = self
            .pending
            .lock()
            .expect("pending 锁")
            .remove(id)
            .ok_or_else(|| {
                crate::error::DevforgeError::Store(format!("无此待答 UI 请求 `{id}`"))
            })?;
        tx.send(value.into()).map_err(|e| {
            crate::error::DevforgeError::System(format!("回传 UI 应答: {e}"))
        })
    }

    /// 待答请求数（测试用）。
    pub fn pending_count(&self) -> usize {
        self.pending.lock().expect("pending 锁").len()
    }
}

/// sop::UserLink 的中继实现（引擎线程内使用）。
pub struct RelayUserLink {
    pub relay: Arc<UiRelay>,
}

impl UserLink for RelayUserLink {
    fn ask(&mut self, question: &str, options: &[&str]) -> Result<String> {
        self.relay.ask_blocking(question, options)
    }
}

// ---------- 生产引擎：规划 SOP 的 RPC 接线 ----------

/// 规划 SOP 的生产引擎：spawn 真 pi 子进程跑 PlanningLoop。
pub struct PlanningRpcEngine {
    pub profile_root: PathBuf,
    pub session_root: PathBuf,
    pub ui: Arc<UiRelay>,
}

impl RunEngine for PlanningRpcEngine {
    fn execute(&mut self, _task: &Task, run: &Run) -> Result<RunOutcome> {
        let plan_client = RpcClient::spawn(SpawnSpec::new(
            self.profile_root.join("plan-agent"),
            "plan-agent",
            run.id.0.clone(),
            self.session_root.join("plan"),
        ))?;
        let plan = RpcPlanSession::new(plan_client);
        let factory = RpcReviewFactory::per_round(
            self.profile_root.join("plan-review"),
            self.session_root.join("reviews"),
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };
        let mut sop_loop = PlanningLoop::new(plan, factory);
        let report = sop_loop.run(&mut user)?;
        tracing::info!(
            component = "planning-engine",
            rounds = report.rounds,
            issues = report.issues_total,
            "规划 SOP 通过"
        );
        Ok(RunOutcome::Completed)
    }
}

// ---------- 服务器状态与路由 ----------

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub store: Mutex<Store>,
    pub registry: SopRegistry,
    pub manifest: Mutex<ConfigManifest>,
    /// 清单真源落盘路径（PUT 时同步写盘；None = 仅内存）。
    pub manifest_path: Option<PathBuf>,
    /// 每 run 引擎工厂（decisions/003：可多实例化的单 run 引擎）。
    pub engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
    pub events: broadcast::Sender<String>,
    pub ui: Arc<UiRelay>,
    /// 鉴权预留位：Some(token) 时要求 `x-devforge-token` 头匹配（Q003）。
    pub token: Option<String>,
    pub profiles_root: PathBuf,
    pub plans_root: PathBuf,
    pub sessions_root: PathBuf,
}

impl ServerState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        registry: SopRegistry,
        manifest: ConfigManifest,
        engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
        events: broadcast::Sender<String>,
        ui: Arc<UiRelay>,
        profiles_root: PathBuf,
        plans_root: PathBuf,
        sessions_root: PathBuf,
    ) -> SharedState {
        Arc::new(ServerState {
            store: Mutex::new(store),
            registry,
            manifest: Mutex::new(manifest),
            manifest_path: None,
            engine_factory,
            events,
            ui,
            token: None,
            profiles_root,
            plans_root,
            sessions_root,
        })
    }

    pub fn emit(&self, value: serde_json::Value) {
        let _ = self.events.send(value.to_string());
    }
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
        // 产物管理 + 可观测
        .route("/projects/{name}/tasks/{task}/runs", get(handlers::list_runs))
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

// ---------- 后台执行（编排控制的 start 端点用） ----------

/// 台账条目直通（server 侧记录用）。
pub type EngineBox = Box<dyn RunEngine + Send>;

/// 在独立线程跑一个 run（不持锁执行引擎——Store 锁只在 begin/finish 短暂持有）。
pub fn run_in_background(
    state: SharedState,
    project: String,
    task_id: String,
    mut engine: EngineBox,
) {
    let (run, task_snapshot) = {
        let mut store = state.store.lock().expect("store 锁");
        match store.begin_run(&project, &task_id) {
            Ok(run) => {
                let task = store
                    .get_task(&project, &task_id)
                    .cloned()
                    .expect("begin_run 已校验存在");
                (run, task)
            }
            Err(e) => {
                state.emit(json!({"type": "run.error", "project": project, "task": task_id, "error": e.to_string()}));
                return;
            }
        }
    };
    state.emit(json!({"type": "run.started", "project": project, "task": task_id, "run_id": run.id.0}));

    let outcome = engine.execute(&task_snapshot, &run);

    let finish = match &outcome {
        Ok(RunOutcome::Completed) => crate::domain::store::FinishOutcome::Completed,
        Ok(RunOutcome::Failed { reason }) => crate::domain::store::FinishOutcome::Failed {
            reason: reason.clone(),
        },
        Err(e) => crate::domain::store::FinishOutcome::Failed {
            reason: e.to_string(),
        },
    };
    let ok = {
        let mut store = state.store.lock().expect("store 锁");
        store.finish_run(&run.id, finish).is_ok()
    };
    state.emit(json!({
        "type": "run.finished",
        "project": project,
        "task": task_id,
        "run_id": run.id.0,
        "ok": ok && matches!(outcome, Ok(RunOutcome::Completed)),
    }));
}

// 台账条目类型在本模块的再导出（spawn 侧调用方记录用）
pub use crate::engine::ledger::LedgerEntry as SpawnLedgerEntry;
