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
use crate::domain::task::TaskStatus;
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

// ---------- 安全小工具（全仓审计 #3/#5/#6） ----------

/// 常量时间字节比较：不按字节早退，避免计时侧信道泄露 token 前缀。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() != b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

/// token 校验：缺失即不匹配（不因“未提供”而短路比较路径）。
fn token_matches(got: Option<&str>, expected: &str) -> bool {
    match got {
        Some(g) => ct_eq(g.as_bytes(), expected.as_bytes()),
        None => false,
    }
}

/// 路径段安全检查：空、`.`/`..`、含分隔符 → 拒（防参数拼路径越界）。
fn safe_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg.contains('/')
        && !seg.contains('\\')
        && !seg.contains('\0')
}

/// local_path 注册校验（全仓审计 #5）：门 0 会在无 `.git` 的目录上 git init +
/// `add -A` 建基线提交——错形态下路径误填为已有目录（如 home）会把其全部内容
/// 纳入一次提交。合法形态：不存在（从 0 创建）/ 空目录 / 已是 git 仓。
fn validate_local_path(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!("local_path 存在但不是目录：{}", path.display()));
    }
    if path.join(".git").exists() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(path)
        .map_err(|e| format!("local_path 不可读 {}：{e}", path.display()))?;
    if entries.next().is_some() {
        return Err(format!(
            "local_path 非空且不是 git 仓：{}——门 0 会在此 git init + add -A 建立基线，\
             误填已有目录会把其全部内容纳入一次提交。请改用空目录或已初始化的 git 仓",
            path.display()
        ));
    }
    Ok(())
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
        // 2026-10-27 自审 minor（105/202）：变体优先——旧口径全靠中文关键词子串，
        // 客户端输入类（JSON/配置）落进 500 内部错，而 System 里带「不存在」字样的
        // 基础设施故障反被判成 404。变体是确定性事实，关键词只在 Store 内细化状态码。
        match e.root_cause() {
            crate::error::DevforgeError::Config(_) => return Self::bad_request(msg),
            // 自审 ref 144：错误码进响应体（机器可读），消费方不必嗅探中文措辞
            // 请求体/数据 JSON 错 = 客户端输入问题
            crate::error::DevforgeError::Json(_) => return Self::bad_request(msg),
            // 服务端侧问题：任何时候都不得被关键词误判成 404/409
            crate::error::DevforgeError::System(_)
            | crate::error::DevforgeError::Quality(_)
            | crate::error::DevforgeError::Agent { .. }
            | crate::error::DevforgeError::RpcProtocol(_)
            | crate::error::DevforgeError::Io(_) => return Self::internal(msg),
            // Store：不变量/校验类，继续走关键词细化（已存在/串行/不存在/其余）
            _ => {}
        }
        // P3-ISSUE-01 修复：变体优先（配置类错误 = 客户端输入问题 → 400），
        // 语义关键词兜底；后续可给错误加机器可读 code（原则 §3）
        if matches!(e, crate::error::DevforgeError::Config(_)) {
            Self::bad_request(msg)
        } else if msg.contains("已存在") {
            Self::conflict(msg)
        } else if msg.contains("不存在") {
            Self::not_found(msg)
        } else if msg.contains("串行") || msg.contains("不可发起") || msg.contains("并发上限") {
            Self::conflict(msg)
        } else if msg.contains("未知 SOP") {
            Self::bad_request(msg)
        } else if msg.contains("依赖") || msg.contains("非法字符") || msg.contains("含非法") {
            // 2026-10-27 自审 major：确定性校验失败（依赖环/自依赖/依赖缺失/名字非法）
            // 是客户端输入问题 → 400，不是 500 内部错误（旧口径落进兑底 internal）。
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
        // 全仓审计 #6：常量时间比较（旧写法 `got != Some(expected)` 按字节早退，
        // 泄露 token 前缀的计时信号）
        if !token_matches(got, expected) {
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
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // 全仓审计 #5：local_path 注册前置校验——门 0 会在无 .git 的目录上 git init +
    // add -A 建基线提交，路径误填为已有目录（如 home）即把其全部内容纳入一次提交。
    if let Err(msg) = validate_local_path(std::path::Path::new(&req.local_path)) {
        return Err(ApiError::bad_request(msg));
    }
    // 自审 S-01 入口收口：name 会被 plans_root.join() 进台账路径与多处文件系统操作，
    // 出口端点已拦但创建入口本尊不放行 = 半截防线；与 safe_segment 同一 allowlist。
    if !safe_segment(&req.name) {
        return Err(ApiError::bad_request(format!(
            "项目名 `{}` 含非法路径字符（不可为空/./..，不可含分隔符）",
            req.name
        )));
    }
    let project = Project::new(req.name, req.local_path.into(), crate::domain::now_ms());
    let created = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.create_project(project).cloned()?
    };
    // 本地关系落地：从 0 = 建目录；已有 = 幂等确认（decisions/001）
    // P1-ISSUE-01 修复：目录落地失败时回滚 store 插入，不留半注册项目
    if let Err(e) = created.ensure_local_dir() {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.remove_project(&created.name);
        return Err(ApiError::from(e));
    }
    // P012 M1 · T1.3：接管清理——注册项目时扫描该仓孤儿 worktree（重启后首个 POST 即钩子；
    // 注册时不可能有活 run：run 需要 task，task 需要项目）
    match crate::workspace::scan_orphans(&created.local_path) {
        Ok(orphans) if !orphans.is_empty() => {
            for o in &orphans {
                tracing::warn!(
                    component = "workspace",
                    task = %o.task_id,
                    path = %o.path.display(),
                    "清理孤儿 worktree"
                );
                // 2026-10-27 自审 minor：清理失败必须留痕——旧口径 `let _ =` 吞掉，
                // 泄漏的 worktree 无任何痕迹（而扫描失败却有日志）。
                if let Err(e) = crate::workspace::remove_orphan(&created.local_path, o) {
                    tracing::error!(
                        component = "workspace",
                        task = %o.task_id,
                        path = %o.path.display(),
                        error = %e,
                        "孤儿 worktree 清理失败（残骸仍在盘上，下次注册会重试）"
                    );
                }
            }
            st.emit(json!({
                "type": "workspace.orphans_cleaned",
                "project": created.name,
                "count": orphans.len(),
            }));
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(component = "workspace", error = %e, "孤儿扫描失败（不阻塞注册）");
        }
    }
    st.emit(json!({"type": "project.created", "project": created.name}));
    // 自审 minor（14）：不回显主机绝对路径（created 仍按全量落库，只对外收口）
    Ok((StatusCode::CREATED, Json(created.public_view())))
}

pub async fn list_projects(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    let views: Vec<serde_json::Value> = store.projects.iter().map(Project::public_view).collect();
    Json(json!({"projects": views}))
}

pub async fn get_project(
    State(st): State<SharedState>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    store
        .get_project(&name)
        .map(Project::public_view)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("项目 `{name}` 不存在")))
}

// ---------- 编排控制：任务 ----------

/// M5 · T5.1：调度态只读快照（不登机器纯 API 回答在飞/卡在哪/队列/冲突）。
pub async fn scheduler_status(State(st): State<SharedState>) -> Json<serde_json::Value> {
    Json(crate::server::scheduler::status(&st))
}

/// P013 M2：审计报告读取（黑板上的 audit-report.json；未生成 = 404）。
pub async fn audit_report(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    // 全仓审计 #3：路径参数不直接拼文件路径——先按 store 确认项目/任务存在，
    // 并拒绕路径分隔与 `..` 段；404 文案不回显绝对路径（不泄露主机目录布局）。
    if !safe_segment(&project) || !safe_segment(&task) {
        return Err(ApiError::bad_request("项目/任务名含非法路径字符"));
    }
    let exists = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.get_project(&project).is_some() && store.get_task(&project, &task).is_some()
    };
    if !exists {
        return Err(ApiError::not_found(format!("项目或任务不存在：{project}/{task}")));
    }
    let path = st
        .plans_root
        .join(&project)
        .join(&task)
        .join("audit-report.json");
    let bytes = std::fs::read(&path)
        .map_err(|_| ApiError::not_found(format!("审计报告尚未生成：{project}/{task}/audit-report.json")))?;
    Ok(Json(serde_json::from_slice(&bytes).map_err(|e| {
        ApiError::internal(format!("审计报告损坏：{e}"))
    })?))
}

#[derive(Deserialize)]
pub struct CreateTaskReq {
    pub id: String,
    pub sop: String,
    /// 工作项契约（P012 M0）：可选——无契约 = 不做边界终校验（向后兼容）。
    #[serde(default)]
    pub contract: Option<crate::domain::contract::WorkItemContract>,
    /// 依赖图（P012 T4.1）：依赖任务全部 Completed 后才可被调度。
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 独占窗口（P012 M4）：运行期间冻结其他派工。
    #[serde(default)]
    pub exclusive: bool,
}

pub async fn create_task(
    State(st): State<SharedState>,
    Path(project): Path<String>,
    Json(req): Json<CreateTaskReq>,
) -> ApiResult<(StatusCode, Json<Task>)> {
    // 自审 S-01 入口收口：task.id 同样进 plans_root/<project>/<task> 路径与 workspace
    // worktree 目标路径（WA-01 同源），入口即拦，不依赖下游各出口各自记得。
    if !safe_segment(&req.id) {
        return Err(ApiError::bad_request(format!(
            "任务 id `{}` 含非法路径字符（不可为空/./..，不可含分隔符）",
            req.id
        )));
    }
    // 自审 S-02 修法②：base_rev 在入口验真实性——写错 rev 会让契约终校验的 git diff
    // 永远非零退出（旧实现那里是 fail-open 放行，现为 System 拒放行），run 白跑一整轮
    // 才发现契约不可用。这里提前拒收：`git cat-file -e <rev>^{commit}` 零成本。
    if let Some(contract) = &req.contract {
        if let Some(base_rev) = &contract.base_rev {
            let local_path = {
                let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                store.get_project(&project).map(|p| p.local_path.clone())
            };
            let Some(local_path) = local_path else {
                return Err(ApiError::bad_request(format!("项目 `{project}` 不存在")));
            };
            let probe = std::process::Command::new("git")
                .args([
                    "-C",
                    &local_path.to_string_lossy(),
                    "cat-file",
                    "-e",
                    &format!("{base_rev}^{{commit}}"),
                ])
                .output();
            let ok = matches!(probe, Ok(o) if o.status.success());
            if !ok {
                return Err(ApiError::bad_request(format!(
                    "contract.base_rev `{base_rev}` 不是项目仓库 `{project}` 里的合法 commit"
                )));
            }
        }
    }
    let created = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let registry = st.registry.clone();
        store.create_task(
            &registry,
            &project,
            req.id,
            req.sop,
            req.contract,
            crate::domain::task::TaskDeps {
                depends_on: req.depends_on,
                exclusive: req.exclusive,
            },
        )
        .cloned()?
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
        .unwrap_or_else(|e| e.into_inner())
        .get(&format!("{project}/{task}"))
        .cloned();
    match control {
        Some(c) => {
            c.cancel();
            // 解阻 dialog 挂起：丢掟全部待答 sender，阻塞中的 ask 立刻失败，
            // SOP 以错误解阻 → 下方 execute 的 is_cancelled 映射为 Failed
            let run_id = {
                let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                store
                    .get_task(&project, &task)
                    .and_then(|t| t.current_run.clone())
            };
            match run_id {
                Some(rid) => {
                    st.ui.abort_run(rid.0.as_str());
                }
                // 无归属信息（理论不可达：control 只在 run 期间登记）——退回全局解阻
                None => {
                    st.ui.abort_all();
                }
            }
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
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
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
    // 2026-10-27 自审 minor：用 Bytes 显式解析——`Option<Json<StartReq>>` 在缺
    // Content-Type 时把请求体静默丢弃（sop 覆盖与 fresh 无声失效，调用方以为生效）。
    body: axum::body::Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // 全仓审计 server #：路径段先过安全校验——下面 fresh 分支会用 project/task 拼 change_root
    // 再 `remove_dir_all`，而 `..` 是合法路径段（同文件的 audit_report 早就校验了，这里漏了）。
    if !safe_segment(&project) || !safe_segment(&task) {
        return Err(ApiError::bad_request("项目/任务名含非法路径字符"));
    }
    // 可选 sop 覆盖：**先记下、不落盘**——server #2：旧实现先 set_task_sop 再 begin_run，
    // 准入失败时任务的 SOP 已经被换掉（一次失败的 start 改写了任务身份）。
    let mut fresh = false;
    let mut requested_sop: Option<String> = None;
    if !body.is_empty() {
        match serde_json::from_slice::<StartReq>(&body) {
            Ok(req) => {
                fresh = req.fresh.unwrap_or(false);
                requested_sop = req.sop;
            }
            Err(e) => {
                // 体非空但解不开 = 客户端错误（不再静默忽略）
                return Err(ApiError::bad_request(format!("请求体解析失败：{e}")));
            }
        }
    }
    let sop_override = requested_sop.is_some();
    // 前置校验（只读）：任务存在性 + 已完成 change 的隐式空转
    let registered_sop = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        // 复验 N1（中高）：change 已完成后再 start 是静默空转（编排器 phase=Done 全跳过、
        // spawn 六个会话后用旧产物出报告）——显式拒绝，重跑带 fresh。
        // 只挡 change（通用 SOP 重跑 = 冷启动重跑，合法）；sop 覆盖 = 跨 SOP 换道（P005），放行。
        if t.status == TaskStatus::Completed && !fresh && !sop_override && t.sop == "change" {
            tracing::warn!(
                component = "server",
                event = "start.rejected",
                project = %project,
                task = %task,
                reason = "completed-change-without-fresh",
                "start 被拒：change 已完成，需 fresh:true"
            );
            return Err(ApiError::conflict(format!(
                "任务 `{project}/{task}` 已完成；重跑请带 fresh:true（完整重置）"
            )));
        }
        t.sop.clone()
    };
    let sop_name = requested_sop
        .clone()
        .unwrap_or_else(|| registered_sop.clone());
    // 渲染闸门（用户裁决 A）：开跑前等「本 run 所需 profile 的 settings.json 落盘」
    // 或「渲染结束」——等的是 awaitable 原语，不占 tokio worker（其它端点照常被处理）。
    // **必须在准入与 SOP 覆盖写盘之前**：客户端在闸门等待期间断开（超时/取消）时，
    // 不得留下占着串行槽的幽灵 run，也不得让一次「从未开跑」的请求改写任务身份。
    let required = crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(&sop_name);
    st.render_gate
        .wait_for_profiles(&st.profiles_root, required)
        .await;
    // sop 覆盖必须在准入之前写盘（`set_task_sop` 拒绝 Running 态，而 begin_run 会把任务置
    // Running）——所以准入失败时**回滚**旧 sop，不让一次失败的 start 改写任务身份（server #2）。
    // （2026-10-27 自审 major：写盘时机从闸门之前移到闸门之后——闸门里断开不再永久改写身份。）
    let mut previous_sop: Option<String> = None;
    if let Some(sop) = requested_sop.clone() {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .set_task_sop(&st.registry, &project, &task, sop)
            .map_err(ApiError::from)?;
        previous_sop = Some(registered_sop);
    }
    // *** 准入在响应之前 ***（server #1）：旧实现直接回 202，真正的 can_begin_run 在后台线程，
    // 失败只发一条 run.error 事件——调用方永远不知道 run 没起来，两个并发请求都能拿到 202。
    let run = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.begin_run(&project, &task) {
            Ok(r) => r,
            Err(e) => {
                // 回滚 sop 覆盖：准入没通过，任务身份不得被改写
                if let Some(old) = previous_sop.clone() {
                    if let Err(re) = store.set_task_sop(&st.registry, &project, &task, old) {
                        // 2026-10-27 自审 minor：回滚失败必须响（不变量靠它维持）
                        tracing::error!(
                            component = "server",
                            event = "start.sop_rollback_failed",
                            project = %project,
                            task = %task,
                            error = %re,
                            "准入失败后回滚 SOP 覆盖失败——任务身份可能已被改写，需人工核对"
                        );
                    }
                }
                tracing::warn!(
                    component = "server",
                    event = "start.rejected",
                    project = %project,
                    task = %task,
                    reason = "admission",
                    error = %e,
                    "start 被拒：准入未通过"
                );
                return Err(ApiError::from(e));
            }
        }
    };
    // 续跑门（plan 010 T003）：**放在准入之后**——fresh 会删该 SOP 的断点，被拒的 start
    // 不许先把断点删掉（与下面 change 黑板清理同一条原则）。失败要回滚槽位与 sop 覆盖。
    let decision = match crate::runstate::resume::start_gate(
        &st.plans_root,
        &project,
        &task,
        &sop_name,
        fresh,
    ) {
        Ok(d) => d,
        Err(e) => {
            let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(fe) = store.finish_run(
                &run.id,
                crate::domain::store::FinishOutcome::Failed {
                    reason: e.to_string(),
                },
            ) {
                // 2026-10-27 自审 major：落账失败必须留痕——吞掉 = 任务永久停在
                // Running 并占死全局串行槽，且无任何日志可查。
                tracing::error!(
                    component = "server",
                    event = "run.finish_failed",
                    run_id = %run.id.0,
                    error = %fe,
                    "准入失败回滚时 finish_run 落账失败——run 可能停在 Running 占住串行槽，需人工检查 store.json"
                );
            }
            if let Some(old) = previous_sop.clone() {
                if let Err(re) = store.set_task_sop(&st.registry, &project, &task, old) {
                    tracing::error!(
                        component = "server",
                        event = "start.sop_rollback_failed",
                        project = %project,
                        task = %task,
                        error = %re,
                        "续跑门失败后回滚 SOP 覆盖失败——任务身份可能已被改写"
                    );
                }
            }
            tracing::warn!(
                component = "server",
                event = "start.rejected",
                project = %project,
                task = %task,
                reason = "resume-gate",
                error = %e,
                "start 被拒：续跑门未通过"
            );
            return Err(ApiError::from(e));
        }
    };
    // 复验 N1：fresh 完整重置——change 的 current 标记/change-id 黑板一并轮换，
    // 重新 assign_change_id 会分配新号（旧号目录不残留）。
    // 放在准入之后：被拒的 start 不许先把现场删掉。
    if fresh && sop_name == "change" {
        let change_root =
            crate::change::change_id::change_root(&st.plans_root, &project, &task);
        if change_root.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&change_root) {
                // 已准入：失败要回滚槽位再抛，否则任务卡 Running
                {
                    let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(fe) = store.finish_run(
                        &run.id,
                        crate::domain::store::FinishOutcome::Failed {
                            reason: format!("清空 change 黑板失败：{e}"),
                        },
                    ) {
                        tracing::error!(
                            component = "server",
                            event = "run.finish_failed",
                            run_id = %run.id.0,
                            error = %fe,
                            "清黑板失败回滚时 finish_run 落账失败——run 可能停在 Running 占住串行槽"
                        );
                    }
                }
                tracing::warn!(
                    component = "server",
                    event = "start.rejected",
                    project = %project,
                    task = %task,
                    reason = "change-blackboard-clear",
                    error = %e,
                    "start 已准入但清黑板失败，已回滚槽位"
                );
                return Err(ApiError::internal(format!(
                    "清空 change 黑板 {} 失败：{e}",
                    change_root.display()
                )));
            }
        }
    }
    let engine = (st.engine_factory)();
    let run_id = run.id.0.clone();
    // 自审 minor（223）：干预面在**返回 202 之前**同步登记（与准入同口径）——
    // 旧口径由后台线程登记，紧跟 start 的 cancel 会打在一个尚未登记的控制对象上（404），
    // 而 run 已准入在跑。
    let control = st.register_control(&project, &task);
    tracing::info!(
        component = "server",
        event = "start.accepted",
        project = %project,
        task = %task,
        sop = %sop_name,
        run_id = %run_id,
        resumed = decision.resumed,
        fresh,
        sop_override,
        "start 已受理（202 已返回后后台执行；干预面已同步登记）"
    );
    std::thread::spawn(move || run_in_background(st, project, task, engine, run, control));
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "sop": sop_name,
            "run_id": run_id,
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
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
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
        // 自审 minor（208）：不回显主机绝对路径（与 project 的 public_view 同口径）——
        // 只给相对台账根的尾巴，客户端拿它拼自己的展示，不拿它拼真实磁盘路径。
        "plan_tree_rel": plan_tree_rel(&st.plans_root, &project, &task),
        "runs": runs,
    })))
}

pub async fn task_graph(
    State(st): State<SharedState>,
    Path((project, task)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
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

/// 结构化日志在观测根下的相对位置（自审 minor 208：对外只回相对路径）。
const LOG_FILE_REL: &str = "logs/devforge.log.jsonl";

/// 台账目录相对形式（`<台账根名>/<project>/<task>`）：对外不回绝对路径。
fn plan_tree_rel(plans_root: &std::path::Path, project: &str, task: &str) -> String {
    let root_name = plans_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "plans".to_string());
    format!("{root_name}/{project}/{task}")
}

/// 结构化日志路径（与 logging::init 同根同约定：`<root>/logs/devforge.log.jsonl`）。
fn log_file_path(st: &SharedState) -> std::path::PathBuf {
    obs_root_of(st).join(LOG_FILE_REL)
}

/// 读 JSONL 日志并按事件名子串过滤、分页。返回（命中总数，本页事件）。
/// 文件缺失/坏行跳过——可观测端点绝不因日志问题 5xx。
/// `run_id` 给定则只返回**属于该 run** 的事件（带 run_id 字段的行）。
pub fn read_log_events(
    st: &SharedState,
    run_id: Option<&str>,
    event_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> (usize, Vec<serde_json::Value>) {
    read_log_events_at(&log_file_path(st), run_id, event_filter, offset, limit)
}

/// 同上，但读指定路径（单测直接喂临时文件用）。
pub fn read_log_events_at(
    path: &std::path::Path,
    run_id: Option<&str>,
    event_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> (usize, Vec<serde_json::Value>) {
    // 2026-10-27 自审 minor（209/210 + 110）：① 按 run_id 过滤（旧口径返回全局日志，
    // 响应里的 "run" 字段是误导标签）；② 流式扫描——旧口径先把全量行读进内存再切片，
    // limit 完全不约束内存（日志动辄 MB 级）。
    let Ok(file) = std::fs::File::open(path) else {
        return (0, Vec::new());
    };
    use std::io::BufRead as _;
    let mut total = 0usize;
    let mut page = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let fields = v.get("fields");
        // run 归属：只有带 run_id 的行才能归到某个 run（引擎/服务端关键事件均带）
        if let Some(want) = run_id {
            let got = fields
                .and_then(|f| f.get("run_id"))
                .and_then(|r| r.as_str());
            if got != Some(want) {
                continue;
            }
        }
        if let Some(f) = event_filter {
            let ok = fields
                .and_then(|fs| fs.get("event"))
                .and_then(|e| e.as_str())
                .is_some_and(|e| e.contains(f));
            if !ok {
                continue;
            }
        }
        if total >= offset && page.len() < limit {
            page.push(v);
        }
        total += 1;
    }
    (total, page)
}

/// 日志里最近一条**属于该 run** 的 agent.* 事件（无 run_id / 无文件 = None）。
/// 2026-10-27 自审 minor（109）：旧口径跳 run 尾扫全量日志——拿到的常是别的 run 的事件。
pub fn last_agent_event_from_log(
    path: &std::path::Path,
    run_id: &str,
) -> Option<serde_json::Value> {
    let file = std::fs::File::open(path).ok()?;
    use std::io::BufRead as _;
    let mut hit = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(fields) = v.get("fields") else { continue };
        if fields.get("run_id").and_then(|r| r.as_str()) != Some(run_id) {
            continue;
        }
        let Some(ev) = fields.get("event").and_then(|e| e.as_str()) else {
            continue;
        };
        if ev.starts_with("agent.") {
            hit = Some(v.clone());
        }
    }
    hit
}

/// DR-7：run 详情聚合——不登机器、纯 API 回答「在干嘛 / 干到哪 / 卡没卡」。
pub async fn get_run_detail(
    State(st): State<SharedState>,
    Path((project, task, run_id)): Path<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let now = crate::domain::now_ms();
    let (run, sop) = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
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
    // agent 活性：历史侧（日志尾扫，全局流）+ 进程侧（按 run-id 键直查——HTTP 线程
    // 无 thread-local run 上下文，旧口径线程局部查询在此恒为 null）
    let log_path = log_file_path(&st);
    let agent = json!({
        "last_event": last_agent_event_from_log(&log_path, &run_id),
        "last_event_age_ms": crate::observability::last_agent_event_age_for(&run_id),
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
        // 自审 minor（208）：日志文件只回相对路径（观测根下的约定位置），不回主机绝对路径。
        "log_file_rel": LOG_FILE_REL,
        // P012 M0 · T0.3：阶段转换历史（串行分数四段的边界戳，相邻 at_ms 差值即段耗时）
        "phases": crate::observability::phase_log(&run_id),
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
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let t = store
            .get_task(&project, &task)
            .ok_or_else(|| ApiError::not_found(format!("任务 `{project}/{task}` 不存在")))?;
        if !t.runs.iter().any(|id| id.0 == run_id) {
            return Err(ApiError::not_found(format!("run `{run_id}` 不存在")));
        }
    }
    let limit = q.limit.unwrap_or(200).min(2000);
    let offset = q.offset.unwrap_or(0);
    let (total, events) = read_log_events(&st, Some(&run_id), q.event.as_deref(), offset, limit);
    Ok(Json(json!({
        "run": run_id,
        // 2026-10-27 自审 minor：只返回可归属到本 run 的日志行（带 run_id 字段），
        // 旧口径回全局事件流，"run" 字段是误导标签。
        "scope": "run",
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
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
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
        // 自审 minor（208）：会话文件名（`<ts>_<run_id>.jsonl`）即可唯一定位，
        // 回主机绝对路径等于把目录布局白送出去。
        "files": files
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect::<Vec<_>>(),
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
    st.ui.answer(&id, req.value).map_err(|e| {
        // 2026-10-27 自审 minor：无此待答 = 404（客户端重复/过期应答），不是 500 服务端故障。
        if e.to_string().contains("无此待答") {
            ApiError::not_found(e.to_string())
        } else {
            ApiError::from(e)
        }
    })?;
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
    // 2026-10-27 自审 major：去重阈值 = **实际回放过的最大序号**；回放为空时必须归零。
    // 旧口径回退成客户端的 Last-Event-ID——服务重启后序号重置，客户端带着旧大序号重连，
    // 全部新序号实时事件 ≤ 旧大序号 → 被永久丢弃。空回放 = 无重叠可能 → 不该过滤任何实时事件。
    let replay_max = replay.last().map(|(id, _)| *id).unwrap_or(0);
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
    // 自审 critical：api_key 明文不回传（脱敏占位；PUT 见占位保持原值）
    Json(st.manifest.lock().unwrap_or_else(|e| e.into_inner()).redacted())
}

/// 整份替换清单（P002；P7-ISSUE-03 修复：渲染异步化——同步只做真源落盘与内存生效，
/// profile 重渲染（含 pi install 分钟级耗时）在后台线程执行，
/// 完成/失败经 SSE `config.rendered` / `config.render_failed` 通知）。
pub async fn put_manifest(
    State(st): State<SharedState>,
    Json(manifest): Json<ConfigManifest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let mut manifest = manifest;
    // 自审 critical：GET 回传的是脱敏占位——PUT 见占位 = 保持现有 Key（往返不丢凭据）。
    // 服务端没有可保留的 Key 却收到占位 = 客户端凭空构造，拒收防止占位字面量落盘。
    if manifest.api_key.as_deref() == Some(crate::config::manifest::REDACTED_API_KEY) {
        let existing = st.manifest.lock().unwrap_or_else(|e| e.into_inner()).api_key.clone();
        if existing.is_none() {
            return Err(ApiError::bad_request(
                "api_key 为脱敏占位但服务端没有可保留的原值——请显式提供真实 Key 或置 null",
            ));
        }
        manifest.api_key = existing;
    }
    // P3-ISSUE-01：同步验证客户端输入（模型/provider 引用可解析），配置错误 fail fast 400
    for agent in &manifest.agents {
        // 自审 critical：name/skills 会被渲染器 join 进 profiles_root 并在目录内落
        // 明文 Key 的 auth.json——路径段不安全 = 任意路径写 + 凭据外泄，必须在此拒收。
        if !safe_segment(&agent.name) {
            return Err(ApiError::bad_request(format!(
                "agent 名 `{}` 含非法路径字符（不可为空/./..，不可含分隔符）",
                agent.name
            )));
        }
        for skill in &agent.skills {
            if !safe_segment(skill) {
                return Err(ApiError::bad_request(format!(
                    "skill 名 `{skill}` 含非法路径字符（不可为空/./..，不可含分隔符）"
                )));
            }
        }
        manifest.resolve_model(agent).map_err(ApiError::from)?;
        manifest.resolve_provider(agent).map_err(ApiError::from)?;
    }
    // 2026-10-27 自审 minor：计数取**本次受理的清单**（旧口径再读一次 st.manifest = 当前值，
    // 并发 PUT 下会回错数字）。
    let agents = manifest.agents.len();
    {
        let mut m = st.manifest.lock().unwrap_or_else(|e| e.into_inner());
        // 2026-10-27 自审 major：先落盘后换内存——旧口径先替换再 save，落盘失败只回 500
        // 不回滚 → 内存与真源文件分叉，重启后配置静默回退。落盘失败时内存未动，两端一致。
        if let Some(path) = st.manifest_path.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            manifest.save(path).map_err(ApiError::from)?;
        }
        *m = manifest;
    }
    tracing::info!(
        component = "server",
        event = "config.updated",
        agents,
        "清单已更新（真源落盘 + 内存生效），后台重渲染开始"
    );
    // 2026-10-27 自审 major：并发 PUT 各自 spawn 渲染线程 = 多线程非原子写同一
    // profiles_root（配置文件撕裂 + pi install 争抢 + 线程无界增长）。渲染全局单飞：
    // 在途时只置 dirty 标记，由在跑的渲染轮末再渲一次（内存清单已更新 → 渲的就是最新版）。
    if !st.render_gate.try_begin_or_coalesce() {
        tracing::info!(
            component = "server",
            event = "config.render.coalesced",
            "渲染已在途——本次提交合并进在跑那一轮（结束后自动重渲最新清单）"
        );
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({"ok": true, "async": true, "agents": agents, "coalesced": true})),
        ));
    }
    // 渲染闸门：登记「渲染在途」——`start` 端点据此等本 run 所需 profile 就绪
    // （用户裁决 A：开跑前等 profile 就绪，而不是开跑后被门卫判成「未渲染」而盲重启）
    let st2 = st.clone();
    std::thread::spawn(move || {
        // 2026-10-27：合并渲染循环的 panic 兜底——若 emit 抛异常，循环体后面的
        // finish_round_or_continue 就不会执行，在途计数永久 >0（后续 start 只能在
        // 闸门上等到超时）。守卫在「正常收工」时置 finished，避免双重递减。
        let mut guard = RenderInFlightGuard {
            st: st2.clone(),
            finished: false,
        };
        loop {
            let st3 = st2.clone();
            // 不带守卫：在途计数归零的决策交给 finish_round_or_continue（与提交同锁）
            render_body_without_guard(st2.clone(), move |v| st3.emit(v));
            if !st2.render_gate.finish_round_or_continue() {
                guard.finished = true;
                break;
            }
            tracing::info!(
                component = "server",
                event = "config.render.round2",
                "合并的配置变更触发重渲（第二轮）"
            );
        }
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
    /// 正常收工时置 true（已由 finish_round_or_continue 归零，Drop 不得重复递减）。
    finished: bool,
}

impl Drop for RenderInFlightGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // 异常展开路径：兜底归零 + 广播，等着的 start 不再等
        self.st.render_gate.end_render();
    }
}

/// 渲染线程体（带在途守卫；供 R-3 用例验证「通知抛异常时计数也归零」）。
///
/// `emit` 是参数而不是直接调 `st.emit`：为了让「发完成通知时抛异常」这条路径**可测**
/// （R-3 的验收判据）——生产调用点走 render_body_without_guard + 外层守卫。
#[cfg(test)]
fn render_body(st: SharedState, emit: impl FnOnce(serde_json::Value)) {
    let _guard = RenderInFlightGuard {
        st: st.clone(),
        finished: false,
    };
    render_body_without_guard(st, emit)
}

/// 渲染 + 发通知（**不管在途计数**，由调用方持有守卫）——合并重渲的循环轮用这个，
/// 否则每轮都归零一次，两轮之间出现空窗让第二个渲染线程插进来。
fn render_body_without_guard(st: SharedState, emit: impl FnOnce(serde_json::Value)) {
    // 渲染本身异常也要发通知（故把渲染包在 catch_unwind 里）
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let m = st.manifest.lock().unwrap_or_else(|e| e.into_inner()).clone();
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
        Err(payload) => {
            // 2026-10-27 自审 minor：捞回 panic 载荷（旧口径整只丢掉，SSE 只拿到
            // 「异常终止」，错误上下文链在此断掉）。
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "（非字符串 panic 载荷）".to_string());
            tracing::error!(
                component = "server",
                event = "config.render.panic",
                panic = %msg,
                "渲染线程 panic（载荷已捞回）"
            );
            emit(json!({"type": "config.render_failed", "error": format!("渲染线程异常终止：{msg}")}));
        }
    }
}

#[cfg(test)]
#[path = "handlers_tests.rs"]
mod tests;

// ---------- P012 M3 · 集成队列（merge queue） ----------

#[derive(Deserialize)]
pub struct EnqueueCandidateReq {
    /// 来源任务 id（分支 task/{id} 的所有者）。
    pub task_id: String,
    /// 冻结 commit（worktree freeze 产物）。
    pub frozen_sha: String,
    /// 候选基点。
    pub base_rev: String,
    /// 契约边界（组合验证后复核）。
    #[serde(default)]
    pub owned_paths: Vec<String>,
}

/// 入队：候选冻结件登记（集成由后台消费者串行执行——M4 调度器接入前为手动触发的
/// `integrate_next`；本端点只负责登记与可见性）。
pub async fn enqueue_candidate(
    State(st): State<SharedState>,
    Path(project): Path<String>,
    Json(req): Json<EnqueueCandidateReq>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // 自审 critical：project 直接 join 进 plans_root/integrations/<project>/queue.json
    //（本文件既有防护模式），不过 safe_segment = 路径穿越 + 任意目录创建/覆写。
    if !safe_segment(&project) {
        return Err(ApiError::bad_request("项目名含非法路径字符"));
    }
    // 2026-10-27 自审 minor：与同文件其它 project 作用域端点一致——项目必须存在
    //（旧口径可对不存在的项目建出 integrations/<name>/ 目录）。
    {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        if store.get_project(&project).is_none() {
            return Err(ApiError::not_found(format!("项目 `{project}` 不存在")));
        }
    }
    // task_id 会被拼进产物文件名（write_artifact）与分支名——入口先过 allowlist
    if !safe_segment(&req.task_id) {
        return Err(ApiError::bad_request(format!(
            "task_id `{}` 含非法路径字符",
            req.task_id
        )));
    }
    // 自审 major：入队走锁定读-改-写——与 scheduler tick 的自动入队并发时，
    // 无锁 load→save 会互相覆写（丢候选）。
    // 自审 major：frozen_sha 会被当作 git 参数（git merge <sha>）——非十六进制
    // 形状 = 参数注入面；也是 worktree freeze 契约的形状门。
    if req.frozen_sha.len() != 40 || !req.frozen_sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request(
            "frozen_sha 必须是 40 位十六进制 commit sha",
        ));
    }
    let depth = crate::integration::MergeQueue::mutate(&st.plans_root, &project, |q| {
        // 2026-10-27 自审 minor：去重——同一 (task_id, frozen_sha) 仍在队且 Pending
        // 时不得重复入队（否则同一候选被集成两次）。
        if q.candidates.iter().any(|c| {
            c.task_id == req.task_id
                && c.frozen_sha == req.frozen_sha
                && matches!(c.status, crate::integration::CandidateStatus::Pending)
        }) {
            return Ok(q.candidates.len());
        }
        q.enqueue(crate::integration::Candidate {
            task_id: req.task_id.clone(),
            frozen_sha: req.frozen_sha.clone(),
            base_rev: req.base_rev.clone(),
            owned_paths: req.owned_paths.clone(),
            status: crate::integration::CandidateStatus::Pending,
            evidence_paths: vec![],
        });
        Ok(q.candidates.len())
    })?;
    st.emit(json!({
        "type": "integration.enqueued",
        "project": project,
        "task": req.task_id,
    }));
    Ok((
        StatusCode::CREATED,
        Json(json!({"queued": true, "depth": depth})),
    ))
}

/// 队列状态：全部候选与各自状态（Pending/Integrated/NeedsRepair/NeedsRebase）。
pub async fn get_integration_queue(
    State(st): State<SharedState>,
    Path(project): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // 自审 critical：同 enqueue_candidate——project 不过 safe_segment 不落地。
    if !safe_segment(&project) {
        return Err(ApiError::bad_request("项目名含非法路径字符"));
    }
    {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        if store.get_project(&project).is_none() {
            return Err(ApiError::not_found(format!("项目 `{project}` 不存在")));
        }
    }
    let q = crate::integration::MergeQueue::load(&st.plans_root, &project)?;
    Ok(Json(json!({
        "project": project,
        "depth": q.candidates.len(),
        "candidates": q.candidates,
    })))
}
