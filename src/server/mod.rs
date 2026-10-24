//! API 服务器（P003 · 服务器形态，非 CLI）。
//!
//! 协议定案（P003 decisions/001）：REST（编排控制/产物/配置/中继输入）+ SSE（事件推送），
//! 路径前缀 `/api/v1` 从第一天；默认绑 127.0.0.1，token 预留位。
//!
//! 五能力域：编排控制 · 产物管理 · 交互中继 · 可观测 · 配置管理。

pub mod change_engine;
pub mod handlers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

use crate::config::installer::PackageInstaller;
use crate::config::ConfigManifest;
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
#[cfg(test)]
use crate::domain::run::Run;
#[cfg(test)]
use crate::domain::task::Task;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::error::Result;
use crate::observability::events::EventBus;
use crate::observability::watchdog::run_with_watchdog;
use crate::sop::planning::UserLink;


// ---------- UI 中继（交互中继域） ----------

/// ask-user-question 的 API 中继：请求推 SSE，应答经 POST 端点回填。
pub struct UiRelay {
    next_id: AtomicU64,
    bus: std::sync::Arc<EventBus>,
    pending: Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>,
}

impl UiRelay {
    /// 事件通道：ui.request 与 run 事件同流（单一 SSE 通道，Q002 信封设计）。
    pub fn new(bus: std::sync::Arc<EventBus>) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            bus,
            pending: Mutex::new(HashMap::new()),
        }
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
        self.bus.emit(json!({
            "type": "ui.request",
            "id": id,
            "question": question,
            "options": options
        }));
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

    /// 中止全部待答（cancel 端点解阻用）：丢弃 sender 使所有阻塞中的
    /// `ask_blocking` 立刻失败解阻，引擎得以按 cancel 语义收场——
    /// 修复 dialog 挂起态 cancel 半效（杀 pi 不解阻 mpsc recv）。
    pub fn abort_all(&self) -> usize {
        let mut map = self.pending.lock().expect("pending 锁");
        let n = map.len();
        map.clear();
        n
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

// ---------- 服务器状态与路由 ----------

/// 渲染闸门（用户裁决 A）：清单重渲染在途时，`start` 端点等「本 run 所需 profile 的
/// `settings.json` 落盘」或「渲染结束」再开跑。
///
/// - 事实源：闸门与门卫读**同一张** per-SOP profile 表（[`BuiltinRpcEngine::profiles_for_sop`]），
///   清单只有一份，不会漂移；
/// - 等待是 **awaitable** 的（watch 通知 + 短间隔复查），**不占** tokio worker
///   （不得用 `std::thread::sleep` 轮询）；
/// - 闸门只等「所需 profile 的就绪」，**不等整轮渲染**（等整轮会把 `start` 变成分钟级阻塞点）。
pub struct RenderGate {
    /// 在途渲染数（>0 = 有渲染在途）。
    in_flight: Mutex<usize>,
    /// 渲染完成通知（值 = 已完成代数；每次完成自增 → 订阅者的 `changed()` 会醒）。
    notify: tokio::sync::watch::Sender<u64>,
}

impl RenderGate {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(0u64);
        Self {
            in_flight: Mutex::new(0),
            notify: tx,
        }
    }

    /// 登记一次在途渲染（`PUT /config/manifest` 受理后调用）。
    pub fn begin_render(&self) {
        let mut n = self.in_flight.lock().expect("render gate 锁");
        *n += 1;
    }

    /// 结束一次渲染（成功/失败都算）并广播。
    pub fn end_render(&self) {
        {
            let mut n = self.in_flight.lock().expect("render gate 锁");
            *n = n.saturating_sub(1);
        }
        self.notify.send_modify(|v| *v += 1);
    }

    /// 是否有渲染在途。
    pub fn is_in_flight(&self) -> bool {
        *self.in_flight.lock().expect("render gate 锁") > 0
    }

    /// 等「所需 profile 的 `settings.json` 落盘」或「渲染结束」。
    ///
    /// 放行条件 = 「**无渲染在途**」**或**「**所需 profile 全部就绪**」二者之一成立
    /// （010 F6 靶 · R-2：并发两轮渲染时，先结束的那一轮**不**放行）。
    ///
    /// 返回即代表「可以开跑」：所需 profile 已就绪（后续门卫复查会过），
    /// 或渲染已结束（缺 profile 则由门卫明确失败，不再盲重启）。
    pub async fn wait_for_profiles(&self, profiles_root: &Path, required: &[&str]) {
        if required.is_empty() {
            return;
        }
        let mut rx = self.notify.subscribe();
        loop {
            if !self.is_in_flight() {
                return; // 渲染不在途 → 直接开跑
            }
            if required
                .iter()
                .all(|p| profiles_root.join(p).join("settings.json").is_file())
            {
                return; // 所需 profile 已就绪 → 不等整轮渲染
            }
            // 等「渲染结束」信号，或短间隔后重查就绪状态
            // （awaitable：不占 tokio worker；不用 std::thread::sleep 轮询）
            //
            // 收到通知**不直接放行**（R-2）：并发两轮渲染时，任意一轮结束就放行 = 闸门形同虚设
            // （所需 profile 还没落盘就把 run 放出去）→ 回环重判上面那两个放行条件。
            tokio::select! {
                _ = rx.changed() => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }
}

impl Default for RenderGate {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub store: Mutex<Store>,
    pub registry: SopRegistry,
    pub manifest: Mutex<ConfigManifest>,
    /// 清单真源落盘路径（PUT 时同步写盘；None = 仅内存）。Mutex 以便测试注入。
    pub manifest_path: Mutex<Option<PathBuf>>,
    /// 每 run 引擎工厂（decisions/003：可多实例化的单 run 引擎）。
    pub engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
    /// profile 安装器（生产 = PiInstallBridge；测试注入 Fake——单测不走真实网络）。
    pub installer: Arc<dyn PackageInstaller + Send + Sync>,
    /// 运行中 run 的干预面（P8-ISSUE-01：cancel 端点置位，watchdog 杀进程）。
    pub runs_control: Mutex<HashMap<String, Arc<RunControl>>>,
    /// 默认 run 超时 ms（None = 不限时，零硬限制哲学；可由 env DEVFORGE_RUN_TIMEOUT_MS 开启）。
    pub run_timeout_ms: Option<u64>,
    /// 事件总线：序号 + 环形缓冲 + 广播（SSE 源与 Last-Event-ID 回放）。
    pub bus: std::sync::Arc<EventBus>,
    pub ui: Arc<UiRelay>,
    /// 鉴权预留位：Some(token) 时要求 `x-devforge-token` 头匹配（Q003）。
    pub token: Option<String>,
    pub profiles_root: PathBuf,
    pub plans_root: PathBuf,
    pub sessions_root: PathBuf,
    /// 运行级重试策略（plan 010 T005）：生产默认 10/30/90s；测试注入 fast。
    pub retry_policy: crate::engine::retry::RetryPolicy,
    /// 渲染闸门（用户裁决 A）：`PUT /config/manifest` 在途时，`start` 端点等所需 profile 就绪。
    pub render_gate: RenderGate,
    /// 观测根覆盖（Some = 本实例的 run 把心跳写在这里；None = 用进程级解析结果）。
    /// 单测用它把落点钉在自己的临时目录 → 不落用户 HOME（R13），且**不与其它单测抢全局 root**。
    pub obs_root: Option<PathBuf>,
}

impl ServerState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        registry: SopRegistry,
        manifest: ConfigManifest,
        engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
        bus: std::sync::Arc<EventBus>,
        ui: Arc<UiRelay>,
        installer: Arc<dyn PackageInstaller + Send + Sync>,
        profiles_root: PathBuf,
        plans_root: PathBuf,
        sessions_root: PathBuf,
    ) -> SharedState {
        Arc::new(ServerState {
            store: Mutex::new(store),
            registry,
            manifest: Mutex::new(manifest),
            manifest_path: Mutex::new(None),
            engine_factory,
            bus,
            ui,
            installer,
            runs_control: Mutex::new(HashMap::new()),
            run_timeout_ms: None,
            token: None,
            profiles_root,
            plans_root,
            sessions_root,
            retry_policy: crate::engine::retry::RetryPolicy::default(),
            render_gate: RenderGate::new(),
            obs_root: None,
        })
    }

    pub fn emit(&self, value: serde_json::Value) {
        self.bus.emit(value);
    }
}

// ---------- 启动入口：token 配置解析（P3：env + CLI 双入口） ----------

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

    // 观测上下文置位：run 归属本线程（引擎与重试层同线程 → attempt 字段同源），
    // 心跳随之落盘（阶段 = 该 SOP 的阶段词汇，R24）。
    let run_phase = run_phase_for_sop(task_snapshot.sop.as_str());
    // 观测根：实例覆盖（单测）优先，否则用进程级解析结果；**每个 run 捕获一次** → 落点固定
    let obs_root = state.obs_root.clone().unwrap_or_else(crate::observability::root);
    // F18：心跳首写不落关键路径（deferred 变体，盘写后台线程）
    crate::observability::begin_run_in_deferred(obs_root, &run.id.0, run_phase);
    tracing::info!(
        component = "run",
        event = "phase.change",
        from = "start",
        to = run_phase,
        "run 开始"
    );

    // 干预面登记（P8-ISSUE-01）：cancel 端点置位 / 可选超时，watchdog 杀自家 pi
    let mut control = RunControl::new();
    if let Some(timeout) = state.run_timeout_ms {
        control = control.with_deadline(crate::domain::now_ms() + timeout);
    }
    let control = Arc::new(control);
    state
        .runs_control
        .lock()
        .expect("runs_control 锁")
        .insert(format!("{project}/{task_id}"), control.clone());

    // 基础设施故障自动重启（plan 010 T005/D004）：只重 System 类，质量失败不重试
    let policy = state.retry_policy.clone();
    let sop_for_state = task_snapshot.sop.clone();
    let state_for_retry = state.clone();
    let (project_for_retry, task_for_retry, run_id_for_retry) =
        (project.clone(), task_id.clone(), run.id.0.clone());
    let (outcome, attempts) = crate::engine::retry::run_with_retry(&policy, &control, |attempt| {
        if attempt > 1 {
            // attempt 递增落盘（审计用；断点本体由各 SOP 自己维护）
            let store = crate::runstate::StateStore::new(
                state_for_retry.plans_root.clone(),
                crate::runstate::StateKey::new(&project_for_retry, &task_for_retry, &sop_for_state),
            );
            let mut rs = match store.load() {
                crate::runstate::Loaded::Found(s) => s,
                _ => store.fresh_state(),
            };
            rs.attempt = attempt;
            if let Err(e) = store.save(&mut rs) {
                tracing::warn!(component = "run", error = %e, "attempt 落盘失败");
            }
            tracing::warn!(
                component = "run",
                attempt,
                project = %project_for_retry,
                task = %task_for_retry,
                "基础设施故障，自动重启 run（从断点继续）"
            );
            state_for_retry.emit(json!({
                "type": "run.retry",
                "project": project_for_retry,
                "task": task_for_retry,
                "run_id": run_id_for_retry,
                "attempt": attempt,
            }));
        }
        engine.execute(&task_snapshot, &run, &control)
    });

    state
        .runs_control
        .lock()
        .expect("runs_control 锁")
        .remove(&format!("{project}/{task_id}"));

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
        "attempts": attempts,
        "ok": ok && matches!(outcome, Ok(RunOutcome::Completed)),
    }));
    // 观测上下文清位：心跳文件**保留**最后一次状态（死亡现场的证据，不删）
    crate::observability::end_run(&run.id.0);
}

/// 每个 SOP 的阶段词汇（心跳的阶段字段 = 阶段名；R24：与里程碑同一套值）。
///
/// change SOP 的首个阶段是 `ChangePhase::Plan`（编排层随后推进到 coding/testing/done）；
/// 其余 SOP 用自己的阶段标识。
fn run_phase_for_sop(sop: &str) -> &str {
    match sop {
        "change" => crate::change::state::ChangePhase::Plan.as_str(),
        "planning" => "plan",
        "coding" => "coding",
        "testing" => "test",
        "doc-from-zero" => "doc",
        "doc-fix" => "doc-fix",
        other => other,
    }
}

// 台账条目类型在本模块的再导出（spawn 侧调用方记录用）
pub use crate::engine::ledger::LedgerEntry as SpawnLedgerEntry;








#[cfg(test)]
mod coding_engine_tests {
    use super::*;
    use crate::engine::builtin::{preflight_codegraph, BuiltinRpcEngine};

    #[test]
    fn preflight_inits_when_no_index() {
        // decisions/013：工作区无索引 → 引擎调用 init（假脚本被调时建 .codegraph 标记）
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let bin = dir.path().join("fake-codegraph.sh");
        std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        preflight_codegraph(&ws, &bin);
        assert!(ws.join(".codegraph").exists(), "预检应调用 init 建索引");
    }

    #[test]
    fn preflight_skips_when_index_exists() {
        // 已有索引 → 不调 init（指向不存在的可执行，若被调即报错暴露）
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(ws.join(".codegraph")).unwrap();
        preflight_codegraph(&ws, Path::new("/nonexistent/codegraph"));
        assert!(ws.join(".codegraph").exists());
    }

    #[test]
    fn dispatch_routes_change_to_its_engine() {
        // 注册表与引擎同步：change 已接线，不应再落到「未接线」分支
        let mut engine = BuiltinRpcEngine {
            profile_root: PathBuf::from("/tmp/devforge-nonexistent-profiles"),
            session_root: PathBuf::from("/tmp"),
            plans_root: PathBuf::from("/tmp"),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = PathBuf::from("/tmp");
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("未接线"), "change 必须已接线：{msg}");
        assert!(
            msg.contains("change-plan-agent"),
            "应卡在 profile 门卫（说明真的进了 run_change）：{msg}"
        );
    }

    #[test]
    fn change_without_requirement_file_is_quality_failure() {
        // 门 0 前先要需求：黑板里没 requirement.md 不得开跑
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for p in crate::server::change_engine::CHANGE_PROFILES {
            let d = profiles.join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = BuiltinRpcEngine {
            profile_root: profiles,
            session_root: dir.path().join("sessions"),
            plans_root: plans,
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let ws = dir.path().join("repo");
        std::fs::create_dir_all(&ws).unwrap();
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = ws;
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("requirement.md"), "{err}");
        assert!(err.is_quality_failure());
    }

    #[test]
    fn change_run_preflights_codegraph_before_plan() {
        // change-plan-agent 被要求 codegraph-first：run_change 必须先建索引（与其他代码类 SOP 同口径）
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for p in crate::server::change_engine::CHANGE_PROFILES {
            let d = profiles.join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        let ws = dir.path().join("repo");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/a.rs"), "// a\n").unwrap();
        // 假 codegraph：init 时在当前工作区建 .codegraph 标记
        let bin = dir.path().join("fake-codegraph");
        std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = BuiltinRpcEngine {
            profile_root: profiles,
            session_root: dir.path().join("sessions"),
            plans_root: plans.clone(),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: bin,
            idle_ms: None,
        };
        // 需求故意缺失：preflight 先于需求门，能验到预检且不会 spawn 真 pi
        let task_root = plans.join("p").join("t");
        std::fs::create_dir_all(&task_root).unwrap();
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = ws.clone();
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("requirement.md"), "{err}");
        assert!(
            ws.join(".codegraph").exists(),
            "run_change 必须先做 codegraph 预检（plan agent 依赖图）"
        );
    }

    #[test]
    fn dispatch_rejects_unwired_sop() {
        // 注册表与引擎同步性：未知/未接线 SOP 在 execute 入口即拒，不 spawn 任何进程
        let mut engine = BuiltinRpcEngine {
            profile_root: PathBuf::from("/tmp"),
            session_root: PathBuf::from("/tmp"),
            plans_root: PathBuf::from("/tmp"),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let task = Task::new("t", "p", "ghost", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未接线"));
    }

    // ---------- F1：spawn 前 profile 门卫（e2e F1-MISSING-PROFILE-HANG） ----------

    fn f1_engine(profile_root: std::path::PathBuf, plans_root: std::path::PathBuf) -> BuiltinRpcEngine {
        BuiltinRpcEngine {
            profile_root,
            session_root: PathBuf::from("/tmp"),
            plans_root,
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        }
    }

    #[test]
    fn profile_gate_passes_when_rendered() {
        // 已渲染（目录 + settings.json 标记）→ 放行
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("coding-agent");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("settings.json"), "{}").unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        engine.require_profile("coding-agent").unwrap();
    }

    #[test]
    fn profile_gate_fails_naming_missing_path() {
        // 缺失 → 报错含角色目录与标记文件路径，指明先渲染
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
        assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
        assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
    }

    #[test]
    fn run_coding_fails_before_side_effects_when_profiles_missing() {
        // 编码 run 在 profile 缺失时立即失败（不再真起 pi 挂 Running），
        // 且校验先于一切副作用：黑板骨架不应被创建
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "coding", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：黑板不应被创建"
        );
    }

    #[test]
    fn run_planning_fails_before_side_effects_when_profiles_missing() {
        // 规划路径同样受门卫保护：缺失即失败，plan root 不落盘
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "planning", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：plan root 不应被创建"
        );
    }

    // ---------- P007 T006：testing / doc-fix 引擎接线 ----------

    #[test]
    fn run_testing_fails_before_side_effects_when_profiles_missing() {
        // 测试 run 在 profile 缺失时立即失败，且 testing/ 骨架不应被创建
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "testing", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：黑板不应被创建"
        );
    }

    #[test]
    fn run_doc_fix_fails_before_side_effects_when_profiles_missing() {
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "doc-fix", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(!plans.join("p").join("t").exists(), "校验应先于副作用");
    }

    #[test]
    fn testing_start_requires_workspace_after_profile_gate() {
        // profile 已渲染但无工作区 → workspace 门（测试需要项目地址写测/起被测系统）
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for role in ["test-agent", "test-review"] {
            let p = profiles.join(role);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("settings.json"), "{}").unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(profiles, plans.clone());
        let task = Task::new("t", "p", "testing", 1); // workspace 默认空
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("无代码工作区"), "应报 workspace 门: {err}");
        assert!(
            !plans.join("p").join("t").join("testing").exists(),
            "workspace 门也应先于骨架创建"
        );
    }

    // ---------- 010 F6 靶：per-SOP profile 表（门卫与闸门的唯一事实源） ----------

    #[test]
    fn profiles_for_sop_is_the_single_source_for_the_gate_and_the_door() {
        // 错实现形态：闸门另抄一份 profile 清单（改了 SOP 的需求，闸门还在等旧清单）
        // → 本用例必须变红
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("planning"),
            ["plan-agent", "plan-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("coding"),
            ["coding-agent", "coding-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("testing"),
            ["test-agent", "test-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("doc-from-zero"),
            ["doc-agent", "doc-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("doc-fix"),
            ["doc-fix", "doc-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("change"),
            crate::server::change_engine::CHANGE_PROFILES.as_slice()
        );
        assert!(
            BuiltinRpcEngine::profiles_for_sop("unknown-sop").is_empty(),
            "未知 SOP 没有 profile 需求"
        );
    }

    #[test]
    fn the_door_checks_every_profile_of_the_sop() {
        // 错实现形态：门卫只查干活 agent（漏掉审查角色）→ 缺审查 profile 也能开跑
        // → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        let agent = profiles.join("coding-agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(agent.join("settings.json"), "{}").unwrap();
        let mut engine = f1_engine(profiles, dir.path().join("plans"));
        let task = Task::new("t", "p", "coding", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(
            err.to_string().contains("coding-review"),
            "审查 profile 缺失也要被门卫拦住：{err}"
        );
    }

    // ---------- 010 F6 靶：渲染闸门 ----------

    #[tokio::test]
    async fn the_gate_waits_for_the_required_profiles_and_ignores_the_rest() {
        // 错实现形态：闸门不等所需 profile（start 立刻开跑 → 门卫判未渲染 → 盲重启）
        // 或等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent", "plan-review"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiter.is_finished(), "所需 profile 未就绪时闸门必须等");
        // 所需 profile 就绪（其它 profile 仍缺、渲染仍在途）→ 放行
        for p in ["plan-agent", "plan-review"] {
            let d = dir.path().join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("所需 profile 就绪后闸门应放行")
            .unwrap();
        assert!(
            gate.is_in_flight(),
            "放行不改变渲染在途状态（闸门不等整轮渲染）"
        );
    }

    #[tokio::test]
    async fn the_gate_releases_when_the_render_finishes_even_if_the_profile_is_missing() {
        // 错实现形态：渲染结束也不放行（start 永远挂着）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!waiter.is_finished(), "渲染仍在途且 profile 未就绪 → 继续等");
        gate.end_render();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("渲染结束后闸门应放行（成败由门卫复查决定）")
            .unwrap();
    }

    #[tokio::test]
    async fn the_gate_keeps_waiting_while_another_render_is_still_in_flight() {
        // 错实现形态：任意一轮渲染结束就放行（`rx.changed()` 分支直接 return）→ 并发两轮渲染时
        //   第一轮结束就把 run 放出去（所需 profile 还没落盘）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render(); // 第 1 轮渲染在途
        gate.begin_render(); // 第 2 轮渲染在途
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiter.is_finished(), "两轮渲染都在途时闸门必须等");
        // 第 1 轮结束：仍有渲染在途、所需 profile 未就绪 → 不得放行
        gate.end_render();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !waiter.is_finished(),
            "仍有渲染在途时不得放行（所需 profile 还没落盘）"
        );
        // 所需 profile 就绪 → 放行（放行条件的另一半）
        let d = dir.path().join("plan-agent");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("所需 profile 就绪后闸门应放行")
            .unwrap();
    }

    #[tokio::test]
    async fn the_gate_does_not_wait_when_no_render_is_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let gate = RenderGate::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            gate.wait_for_profiles(dir.path(), &["plan-agent"]),
        )
        .await
        .expect("无渲染在途时不应等待");
    }

    // ---------- 010 F6 靶：原始错误可见 + profile 门卫的错误种类 ----------

    #[test]
    fn profile_gate_failure_is_a_config_error_not_an_infrastructure_failure() {
        // 错实现形态：仍用 `System`（会被 `is_infra_failure` 判成基础设施故障 → 盲重启 3 次）
        // → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        assert!(
            matches!(err, crate::error::DevforgeError::Config(_)),
            "profile 未就绪应是配置错误（不是基础设施故障）：{err:?}"
        );
        assert!(
            !crate::engine::retry::is_infra_failure(&Err(err)),
            "配置错误不得被判为基础设施故障（否则会盲重启）"
        );
    }

    #[test]
    fn profile_gate_failure_keeps_its_text_unchanged() {
        // 错实现形态：改写错误文案（既有单测与 e2e 靠它指路）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
        assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
        assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
        assert!(
            msg.contains("先 PUT /api/v1/config/manifest"),
            "报错应带处置建议: {msg}"
        );
    }

    #[test]
    fn quality_failure_in_map_intervention_is_logged_as_warn_with_the_retry_layer_fields() {
        // 错实现形态：①质量失败落 ERROR（应 WARN）②缺 `error_chain` / `attempt` / `retryable`
        // ③只留既有那条无字段的 WARN → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().join("profiles"), PathBuf::from("/tmp"));
        let text = crate::observability::test_support::capture_jsonl(|| {
            let outcome = engine
                .map_intervention(
                    &RunControl::new(),
                    "change-engine",
                    crate::error::DevforgeError::Quality("越界改动未申报".into()),
                )
                .unwrap();
            assert!(
                matches!(outcome, RunOutcome::Failed { .. }),
                "质量失败仍是明确的 Failed（不重启）"
            );
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed"))
            .collect();
        assert_eq!(lines.len(), 1, "质量失败应落一条同形状条目：{text}");
        let line = lines[0];
        assert!(line.contains("\"level\":\"WARN\""), "质量失败应落 WARN：{line}");
        assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
        assert!(line.contains("\"component\":\"change-engine\""), "{line}");
        assert!(line.contains("\"attempt\":"), "缺 attempt 字段：{line}");
        assert!(line.contains("\"retryable\":false"), "质量失败不重启：{line}");
        assert!(line.contains("越界改动未申报"), "缺原始错误全文：{line}");
        assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
    }

    #[test]
    fn the_quality_warning_of_a_later_attempt_carries_that_attempts_number() {
        // 错实现形态：attempt 写在**失败分支**里 → 第 2 次尝试期间上下文还是第 1 次的值
        //   → 这条质量 WARN 带 attempt=1（陈旧）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().join("profiles"), PathBuf::from("/tmp"));
        let control = RunControl::new();
        let text = crate::observability::test_support::capture_jsonl(|| {
            // 第 1 次：基础设施故障（会被自动重启）；第 2 次：质量失败（走生产链路的落点 ②）
            let (result, attempts) = crate::engine::retry::run_with_retry(
                &crate::engine::retry::RetryPolicy::fast(),
                &control,
                |n| {
                    if n == 1 {
                        Err(crate::error::DevforgeError::System(
                            "模拟上游 500（测试注入）".into(),
                        ))
                    } else {
                        engine.map_intervention(
                            &control,
                            "change-engine",
                            crate::error::DevforgeError::Quality("终审未通过".into()),
                        )
                    }
                },
            );
            assert_eq!(attempts, 2, "第 1 次基础设施故障后应重启到第 2 次");
            assert!(
                matches!(result, Ok(RunOutcome::Failed { .. })),
                "质量失败仍是明确的 Failed（不重启）"
            );
        });
        let quality: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed") && l.contains("\"level\":\"WARN\""))
            .collect();
        assert_eq!(quality.len(), 1, "质量失败应落一条 WARN 条目：{text}");
        assert!(
            quality[0].contains("\"attempt\":2"),
            "第 2 次尝试的质量 WARN 必须带**本次**尝试号（不是上一次的 1）：{}",
            quality[0]
        );
    }

    #[test]
    fn testing_watch_dirs_cover_handoff_sessions() {
        // D011：testing run 的活性监视覆盖移交会话（doc-fix/doc-reviews 同 run 活性整体）
        let engine = f1_engine(PathBuf::from("/tmp/profiles"), PathBuf::from("/tmp"));
        let dirs = engine.testing_watch_dirs();
        let names: Vec<_> = dirs.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, vec!["testing", "testing-reviews", "doc-fix", "doc-reviews"]);
        let names = engine
            .docfix_watch_dirs()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["doc-fix", "doc-reviews"]);
    }
}
