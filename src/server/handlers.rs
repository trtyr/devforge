//! API 端点处理器（P003 五能力域）。

use axum::extract::{Path, Query, State};
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
    /// 可选：忽略断点、清空该 SOP 的检查点全量重跑（plan 010 D002）。
    #[serde(default)]
    pub fresh: Option<bool>,
}

pub async fn start_task(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
    body: Option<Json<StartReq>>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // 可选 sop 覆盖（无 body / 无字段 = 沿用任务注册的 sop）
    let mut fresh = false;
    if let Some(Json(req)) = body {
        fresh = req.fresh.unwrap_or(false);
        if let Some(sop) = req.sop {
            let mut store = st.store.lock().expect("store 锁");
            store
                .set_task_sop(&st.registry, &project, &task, sop)
                .map_err(ApiError::from)?;
        }
    }
    // 前置快速校验（真正的守卫在 begin_run，双保险）
    let sop_name = {
        let store = st.store.lock().expect("store 锁");
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        if store.has_running_run() {
            return Err(ApiError::conflict(
                "已有运行中的任务（MVP 串行执行，一次一个）",
            ));
        }
        t.sop.clone()
    };
    // 续跑门（plan 010 T003）：fresh 清空该 SOP 的断点；否则按断点存在与否回显 resumed；
    // 只动该 SOP 自己那份状态，同 task 其他 SOP 不受影响。
    let decision =
        crate::runstate::resume::start_gate(&st.plans_root, &project, &task, &sop_name, fresh)
            .map_err(ApiError::from)?;
    // 渲染闸门（用户裁决 A）：开跑前等「本 run 所需 profile 的 settings.json 落盘」
    // 或「渲染结束」——等的是 awaitable 原语，不占 tokio worker（其它端点照常被处理）。
    let required = crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(&sop_name);
    st.render_gate
        .wait_for_profiles(&st.profiles_root, required)
        .await;
    let engine = (st.engine_factory)();
    std::thread::spawn(move || run_in_background(st, project, task, engine));
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "sop": sop_name,
            "resumed": decision.resumed,
            "note": decision.note,
        })),
    ))
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

// ---------- DR-2/DR-7：API-first 可观测 ----------

/// 实例级观测根（fixture 隔离用 obs_root 优先，生产回退进程解析）。
fn obs_root_of(st: &SharedState) -> std::path::PathBuf {
    st.obs_root
        .clone()
        .unwrap_or_else(crate::observability::root)
}

/// 结构化日志路径（与 logging::init 同根同约定：`<root>/logs/devforge.log.jsonl`）。
fn log_file_path(st: &SharedState) -> std::path::PathBuf {
    obs_root_of(st).join("logs").join("devforge.log.jsonl")
}

/// 读 JSONL 日志并按事件名子串过滤、分页。返回（命中总数，本页事件）。
/// 文件缺失/坏行跳过——可观测端点绝不因日志问题 5xx。
pub fn read_log_events(
    st: &SharedState,
    event_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> (usize, Vec<serde_json::Value>) {
    read_log_events_at(&log_file_path(st), event_filter, offset, limit)
}

/// 同上，但读指定路径（单测直接喂临时文件用）。
pub fn read_log_events_at(
    path: &std::path::Path,
    event_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> (usize, Vec<serde_json::Value>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return (0, Vec::new());
    };
    let matched: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| match event_filter {
            Some(f) => v
                .get("fields")
                .and_then(|fs| fs.get("event"))
                .and_then(|e| e.as_str())
                .is_some_and(|e| e.contains(f)),
            None => true,
        })
        .collect();
    let total = matched.len();
    let page = matched.into_iter().skip(offset).take(limit).collect();
    (total, page)
}

/// 日志里最近一条 agent.* 事件（DR-3：agent 活性的历史侧证据；无文件/无事件 = None）。
pub fn last_agent_event_from_log(path: &std::path::Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        let ev = v.get("fields")?.get("event")?.as_str()?;
        ev.starts_with("agent.").then(|| v.clone())
    })
}

/// DR-7：run 详情聚合——不登机器、纯 API 回答「在干嘛 / 干到哪 / 卡没卡」。
pub async fn get_run_detail(
    State(st): State<SharedState>,
    Path((project, task, run_id)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let now = crate::domain::now_ms();
    let (run, sop) = {
        let store = st.store.lock().expect("store 锁");
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        let run = t
            .runs
            .iter()
            .find(|id| id.0 == run_id)
            .and_then(|id| store.get_run(id))
            .ok_or_else(|| ApiError::not_found(format!("run `{run_id}` 不存在")))?;
        (
            json!({
                "id": run.id.0,
                "status": run.status,
                "started_at_ms": run.started_at_ms,
                "ended_at_ms": run.ended_at_ms,
            }),
            t.sop.clone(),
        )
    };
    // doc 进度（x/y）：断点里的 done + units_total（DR-5）
    let progress = if sop == "doc-from-zero" {
        let s = crate::runstate::StateStore::new(
            st.plans_root.clone(),
            crate::runstate::StateKey::new(
                &project,
                &task,
                crate::sop::doc::resume::DOC_SOP,
            ),
        );
        match s.load().state() {
            Some(state) => {
                let done = crate::sop::doc::state::done_units(state);
                json!({
                    "done": done.len(),
                    "total": crate::sop::doc::state::units_total(state),
                    "units_done": done,
                })
            }
            None => json!(null),
        }
    } else {
        serde_json::Value::Null
    };
    // 心跳（run 活性）
    let hb = crate::observability::heartbeat::read(&obs_root_of(&st), &run_id).ok();
    let heartbeat = match &hb {
        Some(h) => json!({
            "phase": h.phase,
            "step": h.step,
            "attempt": h.attempt,
            "age_ms": now.saturating_sub(h.timestamp_ms),
        }),
        None => serde_json::Value::Null,
    };
    // agent 活性：历史侧（日志尾扫）+ 进程侧（本进程内最近 agent 事件）
    let log_path = log_file_path(&st);
    let agent = json!({
        "last_event": last_agent_event_from_log(&log_path),
        "last_event_age_ms": crate::observability::last_agent_event_age_ms(),
        "stall_warn_ms": crate::observability::stall_warn_ms(),
    });
    Ok(Json(json!({
        "project": project,
        "task": task,
        "sop": sop,
        "run": run,
        "heartbeat": heartbeat,
        "agent": agent,
        "progress": progress,
        "log_file": log_path,
    })))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// 事件名子串过滤（如 `agent.`、`doc.unit`）。
    pub event: Option<String>,
}

/// DR-7：run 的历史事件流水（数据源 = 结构化 JSONL 日志；文件只是实现细节，API 是主入口）。
pub async fn run_events(
    State(st): State<SharedState>,
    Path((project, task, run_id)): Path<(String, String, String)>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let store = st.store.lock().expect("store 锁");
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        if !t.runs.iter().any(|id| id.0 == run_id) {
            return Err(ApiError::not_found(format!("run `{run_id}` 不存在")));
        }
    }
    let limit = q.limit.unwrap_or(200).min(2000);
    let offset = q.offset.unwrap_or(0);
    let (total, events) = read_log_events(&st, q.event.as_deref(), offset, limit);
    Ok(Json(json!({
        "run": run_id,
        "total": total,
        "offset": offset,
        "limit": limit,
        "events": events,
    })))
}

#[derive(Deserialize)]
pub struct TranscriptQuery {
    pub from: Option<usize>,
    pub limit: Option<usize>,
    /// 只看某个角色的会话目录（如 doc / doc-reviews / coding）。
    pub role: Option<String>,
}

/// DR-9 L4：run 的完整会话转录（pi 侧对话+工具往返）。文件是存储，API 是唯一入口——
/// 不登机器即可回答「它刚才读了什么写了什么说了什么」。
pub async fn run_transcript(
    State(st): State<SharedState>,
    Path((project, task, run_id)): Path<(String, String, String)>,
    Query(q): Query<TranscriptQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let store = st.store.lock().expect("store 锁");
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        if !t.runs.iter().any(|id| id.0 == run_id) {
            return Err(ApiError::not_found(format!("run `{run_id}` 不存在")));
        }
    }
    // 收集本 run 的会话文件：<sessions_root>/<role>/<ts>_<run_id>.jsonl
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&st.sessions_root) {
        for role_dir in rd.filter_map(|e| e.ok()) {
            let role_name = role_dir.file_name().to_string_lossy().to_string();
            if let Some(want) = &q.role
                && role_name != *want
            {
                continue;
            }
            if let Ok(files_rd) = std::fs::read_dir(role_dir.path()) {
                for f in files_rd.filter_map(|e| e.ok()) {
                    let name = f.file_name().to_string_lossy().to_string();
                    if name.ends_with(".jsonl") && name.contains(&run_id) {
                        files.push(f.path());
                    }
                }
            }
        }
    }
    files.sort();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for f in &files {
        if let Ok(content) = std::fs::read_to_string(f) {
            for line in content.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    entries.push(v);
                }
            }
        }
    }
    let total = entries.len();
    let from = q.from.unwrap_or(0);
    let limit = q.limit.unwrap_or(200).min(2000);
    let page: Vec<serde_json::Value> = entries.into_iter().skip(from).take(limit).collect();
    Ok(Json(json!({
        "run": run_id,
        "files": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "total": total,
        "from": from,
        "limit": limit,
        "entries": page,
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
    // 渲染闸门：登记「渲染在途」——`start` 端点据此等本 run 所需 profile 就绪
    // （用户裁决 A：开跑前等 profile 就绪，而不是开跑后被门卫判成「未渲染」而盲重启）
    st.render_gate.begin_render();
    let st2 = st.clone();
    std::thread::spawn(move || {
        let st3 = st2.clone();
        render_body(st2, move |v| st3.emit(v));
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"ok": true, "async": true, "agents": agents})),
    ))
}

/// 渲染在途的 drop 守卫：在途计数的归零由 `Drop` 保证——渲染线程体**结束**或
/// **因 panic 展开**时都必归零（010 F6 靶 · R-3）。
///
/// 改前是线程体末尾手工调 `end_render()`：而发完成通知（`emit`）在 `catch_unwind` **之外**，
/// 通知发出时抛异常 → 那一行永远不执行 → 计数**永久大于 0** → 之后的 `start` 永远在闸门上等。
struct RenderInFlightGuard {
    st: SharedState,
}

impl Drop for RenderInFlightGuard {
    fn drop(&mut self) {
        // 渲染结束（成功/失败/异常都算）→ 广播，等着的 start 请求不再等
        self.st.render_gate.end_render();
    }
}

/// 渲染线程体：渲染 → 发完成通知（成功 / 失败 / 异常三种结果都发一条）。
///
/// `emit` 是参数而不是直接调 `st.emit`：为了让「发完成通知时抛异常」这条路径**可测**
/// （R-3 的验收判据）——生产调用点传 `|v| st.emit(v)`。
fn render_body(st: SharedState, emit: impl FnOnce(serde_json::Value)) {
    // 在途守卫：线程体结束**或 panic 展开**都归零（不再依赖线程体末尾的显式调用）
    let _guard = RenderInFlightGuard { st: st.clone() };
    // 渲染本身异常也要发通知（故把渲染包在 catch_unwind 里）
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let m = st.manifest.lock().expect("manifest 锁").clone();
        let renderer = crate::config::Renderer {
            manifest: &m,
            installer: st.installer.as_ref(),
            skills_source: None,
        };
        renderer.render_all(&st.profiles_root)
    }));
    match outcome {
        Ok(Ok(rendered)) => {
            emit(json!({"type": "config.rendered", "rendered": rendered.len()}));
        }
        Ok(Err(e)) => {
            emit(json!({"type": "config.render_failed", "error": e.to_string()}));
        }
        Err(_) => {
            emit(json!({"type": "config.render_failed", "error": "渲染线程异常终止"}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::installer::FakeInstaller;
    use crate::domain::run::Run;
    use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
    use crate::domain::sop::SopRegistry;
    use crate::domain::store::Store;
    use crate::domain::task::Task;
    use crate::server::{EventBus, ServerState, UiRelay};

    /// 最小引擎：渲染闸门的用例不需要引擎行为。
    struct NoopEngine;

    impl RunEngine for NoopEngine {
        fn execute(
            &mut self,
            _task: &Task,
            _run: &Run,
            _control: &RunControl,
        ) -> crate::error::Result<RunOutcome> {
            Ok(RunOutcome::Completed)
        }
    }

    fn test_state(dir: &std::path::Path) -> SharedState {
        let bus = std::sync::Arc::new(EventBus::new());
        let ui = std::sync::Arc::new(UiRelay::new(bus.clone()));
        ServerState::new(
            Store::new(),
            SopRegistry::builtin(),
            ConfigManifest::builtin(),
            Box::new(|| Box::new(NoopEngine) as Box<dyn RunEngine + Send>),
            bus,
            ui,
            std::sync::Arc::new(FakeInstaller),
            dir.join("profiles"),
            dir.join("plans"),
            dir.join("sessions"),
        )
    }

    #[test]
    fn the_in_flight_count_goes_back_to_zero_when_the_render_body_panics() {
        // 错实现形态：守卫缺失（在途计数只在线程体末尾手工递减）→ 发完成通知时 panic 展开
        //   → 计数永久大于 0 → 之后的 start 永远在闸门上等 → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let st = test_state(dir.path());
        st.render_gate.begin_render();
        assert!(st.render_gate.is_in_flight(), "登记后应有渲染在途");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_body(st.clone(), |_| panic!("发完成通知时炸了"));
        }));
        assert!(result.is_err(), "发完成通知抛异常时线程体应展开");
        assert!(
            !st.render_gate.is_in_flight(),
            "在途计数必须归零（否则之后的 start 永远在等）"
        );
    }

    #[tokio::test]
    async fn a_finished_render_leaves_no_in_flight_count_behind() {
        // 错实现形态：渲染结束后计数不归零（守卫没接上 / 归零那一行被删）→ 之后的 start 永远在等
        //   → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let st = test_state(dir.path());
        let resp = match put_manifest(State(st.clone()), Json(ConfigManifest::builtin())).await {
            Ok(r) => r,
            Err(_) => panic!("内置清单应通过校验（不该 400）"),
        };
        assert_eq!(resp.0, StatusCode::ACCEPTED, "PUT 清单应立即 202（渲染在后台）");
        for _ in 0..200 {
            if !st.render_gate.is_in_flight() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            !st.render_gate.is_in_flight(),
            "渲染结束后在途计数必须归零（R-27）"
        );
    }
}
