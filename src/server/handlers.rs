//! API 端点处理器（P003 五能力域）。

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response, Sse};
use axum::extract::Request;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;

use crate::config::ConfigManifest;
use crate::domain::project::Project;
use crate::domain::task::Task;
use crate::server::{run_in_background, SharedState};

// ---------- 错误映射（原则 §1：对外错误面向人和 AI 双读者） ----------

/// API 错误：状态码 + 消息。
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::CONFLICT, message: msg.into() }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: msg.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({"error": self.message});
        (self.status, Json(body)).into_response()
    }
}

impl From<crate::error::DevforgeError> for ApiError {
    fn from(e: crate::error::DevforgeError) -> Self {
        let msg = e.to_string();
        // P3-ISSUE-01 修复：变体优先（配置类错误 = 客户端输入问题 → 400），
        // 语义关键词兜底；后续可给错误加机器可读 code（原则 §3）
        if matches!(e, crate::error::DevforgeError::Config(_)) {
            Self::bad_request(msg)
        } else if msg.contains("已存在") {
            Self::conflict(msg)
        } else if msg.contains("不存在") {
            Self::not_found(msg)
        } else if msg.contains("串行") || msg.contains("不可发起") {
            Self::conflict(msg)
        } else if msg.contains("未知 SOP") {
            Self::bad_request(msg)
        } else {
            Self::internal(msg)
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ---------- 鉴权（Q003：预留位——未配置 token 即放行） ----------

pub async fn auth_middleware(
    State(st): State<SharedState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(expected) = &st.token {
        let got = req.headers().get("x-devforge-token").and_then(|v| v.to_str().ok());
        if got != Some(expected.as_str()) {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "token 不匹配"})))
                .into_response();
        }
    }
    next.run(req).await
}

// ---------- 编排控制：项目 ----------

#[derive(Deserialize)]
pub struct CreateProjectReq {
    pub name: String,
    /// 本地路径——全手动指定（P004 decisions/001）
    pub local_path: String,
}

pub async fn create_project(
    State(st): State<SharedState>,
    Json(req): Json<CreateProjectReq>,
) -> ApiResult<(StatusCode, Json<Project>)> {
    let project = Project::new(req.name, req.local_path.into(), crate::domain::now_ms());
    let created = {
        let mut store = st.store.lock().expect("store 锁");
        store.create_project(project).cloned()?
    };
    // 本地关系落地：从 0 = 建目录；已有 = 幂等确认（decisions/001）
    // P1-ISSUE-01 修复：目录落地失败时回滚 store 插入，不留半注册项目
    if let Err(e) = created.ensure_local_dir() {
        let mut store = st.store.lock().expect("store 锁");
        store.remove_project(&created.name);
        return Err(ApiError::from(e));
    }
    st.emit(json!({"type": "project.created", "project": created.name}));
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_projects(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let store = st.store.lock().expect("store 锁");
    Json(json!({"projects": store.list_projects()}))
}

pub async fn get_project(
    State(st): State<SharedState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Project>> {
    let store = st.store.lock().expect("store 锁");
    store
        .get_project(&name)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("项目 `{name}` 不存在")))
}

// ---------- 编排控制：任务 ----------

#[derive(Deserialize)]
pub struct CreateTaskReq {
    pub id: String,
    pub sop: String,
}

pub async fn create_task(
    State(st): State<SharedState>,
    Path(project): Path<String>,
    Json(req): Json<CreateTaskReq>,
) -> ApiResult<(StatusCode, Json<Task>)> {
    let created = {
        let mut store = st.store.lock().expect("store 锁");
        let registry = st.registry.clone();
        store.create_task(&registry, &project, req.id, req.sop).cloned()?
    };
    st.emit(json!({"type": "task.created", "project": project, "task": created.id}));
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn cancel_task(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // P8-ISSUE-01 修复：取消端点——置位后 watchdog 按 run-id 精确 SIGKILL 自家 pi
    let control = st
        .runs_control
        .lock()
        .expect("runs_control 锁")
        .get(&format!("{project}/{task}"))
        .cloned();
    match control {
        Some(c) => {
            c.cancel();
            // 解阻 dialog 挂起：丢掟全部待答 sender，阻塞中的 ask 立刻失败，
            // SOP 以错误解阻 → 下方 execute 的 is_cancelled 映射为 Failed
            st.ui.abort_all();
            Ok(Json(json!({"ok": true, "cancelled": true})))
        }
        None => Err(ApiError::not_found(format!(
            "任务 `{project}/{task}` 当前无运行中的 run，无可取消对象"
        ))),
    }
}

pub async fn list_tasks(
    State(st): State<SharedState>,
    Path(project): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().expect("store 锁");
    if store.get_project(&project).is_none() {
        return Err(ApiError::not_found(format!("项目 `{project}` 不存在")));
    }
    Ok(Json(json!({"tasks": store.list_tasks(&project)})))
}

#[derive(Deserialize, Default)]
pub struct StartReq {
    /// 可选 SOP 覆盖（P005 跨 SOP 触发：同任务先规划后编码）。
    pub sop: Option<String>,
}

pub async fn start_task(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
    body: Option<Json<StartReq>>,
) -> ApiResult<StatusCode> {
    // 可选 sop 覆盖（无 body / 无字段 = 沿用任务注册的 sop）
    if let Some(Json(req)) = body {
        if let Some(sop) = req.sop {
            let mut store = st.store.lock().expect("store 锁");
            store
                .set_task_sop(&st.registry, &project, &task, sop)
                .map_err(ApiError::from)?;
        }
    }
    // 前置快速校验（真正的守卫在 begin_run，双保险）
    {
        let store = st.store.lock().expect("store 锁");
        store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        if store.has_running_run() {
            return Err(ApiError::conflict(
                "已有运行中的任务（MVP 串行执行，一次一个）",
            ));
        }
    }
    let engine = (st.engine_factory)();
    std::thread::spawn(move || run_in_background(st, project, task, engine));
    Ok(StatusCode::ACCEPTED)
}

// ---------- 产物管理 + 可观测 ----------

pub async fn list_runs(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().expect("store 锁");
    let t = store
        .get_task(&project, &task)
        .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
    let runs: Vec<serde_json::Value> = t
        .runs
        .iter()
        .filter_map(|id| store.get_run(id))
        .map(|r| {
            json!({
                "id": r.id.0,
                "status": r.status,
                "started_at_ms": r.started_at_ms,
                "ended_at_ms": r.ended_at_ms,
            })
        })
        .collect();
    // 产物归集：plan tree 位置（decisions/002 两层级）
    Ok(Json(json!({
        "project": project,
        "task": task,
        "current_run": t.current_run,
        "plan_tree_dir": st.plans_root.join(&project).join(&task),
        "runs": runs,
    })))
}

pub async fn task_graph(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().expect("store 锁");
    let t = store
        .get_task(&project, &task)
        .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
    let nodes: Vec<serde_json::Value> = t
        .runs
        .iter()
        .filter_map(|id| store.get_run(id))
        .map(|r| json!({"node": r.id.0, "kind": "run", "status": r.status}))
        .collect();
    Ok(Json(json!({
        "project": project,
        "task": task,
        "status": t.status,
        "sop": t.sop,
        "nodes": nodes,
    })))
}

// ---------- 交互中继 ----------

#[derive(Deserialize)]
pub struct AnswerReq {
    pub value: String,
}

pub async fn answer_ui(
    State(st): State<SharedState>,
    Path(id): Path<String>,
    Json(req): Json<AnswerReq>,
) -> ApiResult<Json<serde_json::Value>> {
    st.ui.answer(&id, req.value).map_err(ApiError::from)?;
    Ok(Json(json!({"ok": true})))
}

/// SSE 事件流：run 生命周期 + ui.request + 领域事件（Q002 信封：JSON 一行一事件）。
/// 断线续传（A1 尾巴收口）：携带 `Last-Event-ID` 头时先回放缓冲中该序号之后的
/// 留档事件再接实时流；回放与订阅间隙的重叠按序号去重（缓冲窗口内恰一次）。
pub async fn events(
    State(st): State<SharedState>,
    headers: HeaderMap,
) -> Sse<impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    let last_id: u64 = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let rx = st.bus.subscribe();
    let replay = st.bus.replay_after(last_id);
    let replay_max = replay.last().map(|(id, _)| *id).unwrap_or(last_id);
    let replayed = tokio_stream::iter(replay.into_iter().map(|(id, data)| {
        Ok(axum::response::sse::Event::default()
            .id(id.to_string())
            .data(data))
    }));
    let live = BroadcastStream::new(rx).filter_map(move |item| match item {
        Ok((id, data)) => {
            if id <= replay_max {
                return None; // 回放已覆盖（订阅与快照间隙去重）
            }
            Some(Ok(axum::response::sse::Event::default()
                .id(id.to_string())
                .data(data)))
        }
        Err(_) => None, // Lagged：订阅者跟不上就跳过（不毒化流）
    });
    Sse::new(replayed.chain(live)).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
    )
}

// ---------- 配置管理 ----------

pub async fn get_manifest(State(st): State<SharedState>) -> Json<ConfigManifest> {
    Json(st.manifest.lock().expect("manifest 锁").clone())
}

/// 整份替换清单（P002；P7-ISSUE-03 修复：渲染异步化——同步只做真源落盘与内存生效，
/// profile 重渲染（含 pi install 分钟级耗时）在后台线程执行，
/// 完成/失败经 SSE `config.rendered` / `config.render_failed` 通知）。
pub async fn put_manifest(
    State(st): State<SharedState>,
    Json(manifest): Json<ConfigManifest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // P3-ISSUE-01：同步验证客户端输入（模型/provider 引用可解析），配置错误 fail fast 400
    for agent in &manifest.agents {
        manifest.resolve_model(agent).map_err(ApiError::from)?;
        manifest.resolve_provider(agent).map_err(ApiError::from)?;
    }
    {
        let mut m = st.manifest.lock().expect("manifest 锁");
        *m = manifest;
        if let Some(path) = st.manifest_path.lock().expect("manifest_path 锁").as_ref() {
            m.save(path).map_err(ApiError::from)?;
        }
    }
    let agents = st.manifest.lock().expect("manifest 锁").agents.len();
    let st2 = st.clone();
    std::thread::spawn(move || {
        let m = st2.manifest.lock().expect("manifest 锁").clone();
        let renderer = crate::config::Renderer {
            manifest: &m,
            installer: st2.installer.as_ref(),
            skills_source: None,
        };
        match renderer.render_all(&st2.profiles_root) {
            Ok(rendered) => {
                st2.emit(json!({"type": "config.rendered", "rendered": rendered.len()}));
            }
            Err(e) => {
                st2.emit(json!({"type": "config.render_failed", "error": e.to_string()}));
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"ok": true, "async": true, "agents": agents})),
    ))
}
