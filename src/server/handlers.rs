//! API 端点处理器（P003 五能力域）。

use axum::extract::{Extension, Path, Query, State};
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
use crate::server::{run_in_background, PendingAskView, SharedState};

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
            // 2026-11-01 审计补丁（R3-15）：结构化 HTTP 语义变体——不再靠中文关键词猜
            crate::error::DevforgeError::NotFound(_) => return Self::not_found(msg),
            crate::error::DevforgeError::Conflict(_) => return Self::conflict(msg),
            crate::error::DevforgeError::InvalidInput(_) => return Self::bad_request(msg),
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
        // 自审 minor（45）：客户端输入类校验错误带统一前缀「非法请求：」（域层约定），
        // 不靠个别中文关键词碰运气——否则新加的校验（如 kebab-case）会静默落进 500。
        // （Config 变体已在上面按变体优先拦掉，这里不重复。）
        if msg.contains("非法请求") {
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

// ---------- 鉴权（A1 · M-1：session cookie / token 头 / 回环豁免三通道） ----------

/// 已认证身份（中间件验证成功后注入 request extensions，写操作审计取 actor 用）。
#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub username: String,
    pub role: crate::domain::auth::Role,
    /// 认证通道：session / token / loopback（审计里可答「谁从哪条路进来的」）。
    pub via: &'static str,
}

impl AuthedUser {
    fn anonymous(via: &'static str) -> Self {
        Self {
            username: format!("@{via}"),
            role: crate::domain::auth::Role::Admin,
            via,
        }
    }

    /// 写操作审计（四类写 + manifest/model swap）：store.auth_events 落 write_op。
    /// 放在这里而非各 handler 自拼——actor 口径（via 后缀）单点。
    pub fn audit_write(&self, st: &SharedState, op: &str, detail: &str) {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.push_auth_event(
            "write_op",
            &format!("{}({})", self.username, self.via),
            &format!("{op}: {detail}"),
            crate::domain::now_ms(),
        );
    }
}

/// 守门中间件（/api/v1/*，含 SSE 握手——EventSource 自动携 cookie，免特判）：
/// ① `x-devforge-token` 头（DR-9 服务间通道，常量时间比较，不废）；
/// ② 会话 cookie（server::auth 滑动续期）；
/// ③ 回环豁免（`auth_loopback_exempt`，缺省 true = 本机免鉴权；远程永不豁免）。
/// 三通道任一通过即放行，全不命中 → 401。
pub async fn auth_middleware(
    State(st): State<SharedState>,
    mut req: Request,
    next: Next,
) -> Response {
    // 真实对端地址（into_make_service_with_connect_info 注入 extensions；
    // oneshot 测试不注入 → None，fail-closed 不豁免——模拟本机须显式插回环对端）。
    // 不从签名提取——Option<ConnectInfo> 不满足 from_fn 的 extractor bound。
    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0);
    let mut authed: Option<AuthedUser> = None;
    // ① 服务间 token 通道
    if authed.is_none()
        && let Some(expected) = &st.token
    {
        let got = req.headers().get("x-devforge-token").and_then(|v| v.to_str().ok());
            // 全仓审计 #6：常量时间比较（旧写法 `got != Some(expected)` 按字节早退，
            // 泄露 token 前缀的计时信号）
            if token_matches(got, expected) {
                authed = Some(AuthedUser::anonymous("token"));
            }
    }
    // ② 会话 cookie 通道（携带真实身份——写操作审计的 actor 来源）
    if authed.is_none()
        && let Some(cookie) = req.headers().get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok())
        && let Some(token) = crate::server::auth::token_from_cookie(cookie)
        && let Some((username, role)) =
            crate::server::auth::validate_session(&st, &token, crate::domain::now_ms())
    {
        authed = Some(AuthedUser {
            username,
            role,
            via: "session",
        });
    }
    // ③ 回环豁免通道（fail-closed：无对端信息不豁免——R4-EX-12/R2-15）
    if authed.is_none()
        && st.auth_loopback_exempt.load(std::sync::atomic::Ordering::Relaxed)
        && crate::server::auth::is_loopback_peer(peer)
    {
        authed = Some(AuthedUser::anonymous("loopback"));
    }
    let Some(user) = authed else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "未认证（登录或携带 token）"})),
        )
            .into_response();
    };
    // Observer 写操作降权（R3-05 裁决③落地：假 RBAC 变真）——login/logout 挂在
    // 守门之外不经此处；GET/SSE 只读放行。
    if user.role == crate::domain::auth::Role::Observer && is_write_method(req.method()) {
        tracing::warn!(
            component = "auth",
            event = "observer_write_forbidden",
            user = %user.username,
            via = %user.via,
            method = %req.method(),
            path = %req.uri().path(),
            "Observer 角色发起写操作，已拒绝（403）"
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Observer 为只读角色，无写权限"})),
        )
            .into_response();
    }
    req.extensions_mut().insert(user);
    next.run(req).await
}

/// 写操作判定（HTTP 动词面——POST/PATCH/PUT/DELETE）。
fn is_write_method(method: &axum::http::Method) -> bool {
    matches!(
        *method,
        axum::http::Method::POST
            | axum::http::Method::PATCH
            | axum::http::Method::PUT
            | axum::http::Method::DELETE
    )
}

/// `POST /auth/login`：校验口令签发会话 cookie。审计：成功/失败都落
/// `store.auth_events`（login_success / login_failure + 具体原因，不含口令本体）。
#[derive(Debug, Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

pub async fn auth_login(
    State(st): State<SharedState>,
    Json(req): Json<LoginReq>,
) -> Response {
    let now = crate::domain::now_ms();
    let audit = |kind: &str, detail: &str| {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.push_auth_event(kind, &req.username, detail, now);
    };
    match crate::server::auth::login(&st, &req.username, &req.password, now) {
        Ok((token, role)) => {
            audit("login_success", "登录成功");
            // R4-EX-11：绑定非回环时附 Secure（判定源 = serve 注入的绑定姿态）
            let secure = st.bind_non_loopback.load(std::sync::atomic::Ordering::Relaxed);
            (
                StatusCode::OK,
                [(
                    axum::http::header::SET_COOKIE,
                    crate::server::auth::session_cookie_value(&token, secure),
                )],
                Json(json!({
                    "ok": true,
                    "username": req.username,
                    "role": role,
                })),
            )
                .into_response()
        }
        // R4-EX-3：限速中 → 429 + Retry-After（秒，向上取整）
        Err(e @ crate::server::auth::LoginError::Throttled { retry_after_ms }) => {
            audit("login_failure", e.detail());
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(
                    axum::http::header::RETRY_AFTER,
                    retry_after_ms.div_ceil(1000).to_string(),
                )],
                Json(json!({
                    "error": e.message(),
                    "retry_after_ms": retry_after_ms,
                })),
            )
                .into_response()
        }
        Err(e) => {
            audit("login_failure", e.detail());
            (StatusCode::UNAUTHORIZED, Json(json!({"error": e.message()}))).into_response()
        }
    }
}

/// `POST /auth/logout`：删会话 + 清 cookie（幂等；未认证也 200——登出不值得守门）。
pub async fn auth_logout(
    State(st): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Some(cookie) = headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok())
        && let Some(token) = crate::server::auth::token_from_cookie(cookie)
    {
        crate::server::auth::logout(&st, &token);
    }
    let secure = st.bind_non_loopback.load(std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            crate::server::auth::cleared_cookie_value(secure),
        )],
        Json(json!({"ok": true})),
    )
        .into_response()
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
    Extension(user): Extension<AuthedUser>,
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
    // R3-12（审计批次二）：目录先行——store 注册前本地目录必须已就绪。
    // 旧序（注册→建目录→失败回滚）在 persist 后崩溃时会留半注册项目
    // （store.json 有、目录无，重启后误导性残留）；目录先行后 store 注册不再
    // 有失败路径，注册失败时留下的只是空目录（幂等重入确认即过，无害）。
    project.ensure_local_dir().map_err(ApiError::from)?;
    let created = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.create_project(project).cloned()?
    };
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
    // R3-16/EX-13 审计系统化：创建项目是高价值写操作，必须有 actor 可追
    user.audit_write(&st, "create_project", &created.name);
    // 自审 minor（14）：不回显主机绝对路径（created 仍按全量落库，只对外收口）
    Ok((StatusCode::CREATED, Json(created.public_view())))
}

#[derive(Debug, Deserialize)]
pub struct ArchiveReq {
    /// 缺省 true（归档）；false = 取消归档。
    #[serde(default = "crate::server::handlers::default_true")]
    pub archived: bool,
}

pub(crate) fn default_true() -> bool {
    true
}

/// `POST /projects/{name}/archive`：归档/取消归档（A6）。归档项目的 start 被拒。
pub async fn archive_project(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Path(name): Path<String>,
    body: Option<Json<ArchiveReq>>,
) -> ApiResult<Json<serde_json::Value>> {
    let archived = body.map(|Json(b)| b.archived).unwrap_or(true);
    let changed = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.set_project_archived(&name, archived)?
    };
    if changed > 0 {
        user.audit_write(&st, "archive_project", &format!("{name} → archived={archived}"));
    }
    Ok(Json(json!({ "ok": true, "name": name, "archived": archived, "changed": changed })))
}

/// `DELETE /projects/{name}`：删除项目（A6）——零任务才可删（有任务 409 防误删）。
pub async fn delete_project(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let deleted = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        if store.get_project(&name).is_none() {
            return Err(ApiError::not_found(format!("项目 `{name}` 不存在")));
        }
        store.delete_project(&name).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("仍有任务") {
                ApiError::conflict(msg)
            } else {
                ApiError::from(e)
            }
        })?
    };
    user.audit_write(&st, "delete_project", &deleted);
    Ok(Json(json!({ "ok": true, "deleted": deleted })))
}

/// 项目完整视图（A6）：public_view + plantree_path（`<plans_root>/plantree/<name>`，
/// 与 change SOP 的台账根同源推导）。P019 项目卡片定稿要显式展示两条路径。
fn project_full_view(p: &Project, plans_root: &std::path::Path) -> serde_json::Value {
    let mut v = p.public_view();
    v["plantree_path"] = json!(
        crate::change::prepare::plantree_root(plans_root, &p.name).display().to_string()
    );
    v
}

pub async fn list_projects(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    let views: Vec<serde_json::Value> = store
        .projects
        .iter()
        .map(|p| project_full_view(p, &st.plans_root))
        .collect();
    Json(json!({"projects": views}))
}

pub async fn get_project(
    State(st): State<SharedState>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    store
        .get_project(&name)
        .map(|p| Json(project_full_view(p, &st.plans_root)))
        .ok_or_else(|| ApiError::not_found(format!("项目 `{name}` 不存在")))
}

// ---------- 编排控制：任务 ----------

/// M5 · T5.1：调度态只读快照（不登机器纯 API 回答在飞/卡在哪/队列/冲突）。
pub async fn scheduler_status(State(st): State<SharedState>) -> Json<serde_json::Value> {
    Json(crate::server::scheduler::status(&st))
}

// ---------- 通知（A2 · 通知系统） ----------

/// 通知列表 + 未读数（前端铃铛角标的数据源）。/// 查询参数 `unread=true` 时只返回未读条目。
pub async fn list_notifications(
    State(st): State<SharedState>,
    Query(params): Query<NotificationQuery>,
) -> Json<serde_json::Value> {
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    let unread_only = params.unread.unwrap_or(false);
    let items: Vec<&crate::domain::Notification> = store
        .notifications
        .iter()
        .filter(|n| !unread_only || !n.read)
        .collect();
    Json(serde_json::json!({
        "unread": store.unread_notification_count(),
        "items": items,
    }))
}

/// 已读标记：body `{"ids": [..]}` 逐条标记；或 `{"all": true}` 全部已读。
/// 返回 `{"marked": n, "unread": 余量}`。
pub async fn read_notifications(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    // guard 块化：audit_write 内部会重锁 store——Mutex 不可重入（k4 死锁教训），
    // 计算与审计必须分域
    let (marked, unread) = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let marked = if body.get("all").and_then(|v| v.as_bool()).unwrap_or(false) {
            store.mark_all_notifications_read()
        } else {
            body.get("ids")
                .and_then(|v| v.as_array())
                .map(|ids| {
                    ids.iter()
                        .filter_map(|v| v.as_u64())
                        .filter(|&id| store.mark_notification_read(id))
                        .count()
                })
                .unwrap_or(0)
        };
        let unread = store.unread_notification_count();
        (marked, unread)
    };
    // R3-16/EX-13 审计系统化：通知已读是写操作（状态变更）——guard 已释放
    user.audit_write(&st, "read_notifications", &format!("marked={marked}"));
    Json(serde_json::json!({
        "marked": marked,
        "unread": unread,
    }))
}

/// `GET /notifications` 的查询参数。
#[derive(Debug, Deserialize)]
pub struct NotificationQuery {
    #[serde(default)]
    pub unread: Option<bool>,
}

// ---------- 配置：旋钮生效值（F14/T205） ----------

/// 旋钮生效值端点：声明表全量覆盖 + 来源标注（file 覆盖可见、未覆盖=default）——
/// 「现在生效了什么」不再靠读源码。
pub async fn knob_effective() -> Json<serde_json::Value> {
    let items = crate::config::knobs::effective_values();
    Json(serde_json::json!({ "knobs": items }))
}

// ---------- 可观测：server 运维信息（A7 · P019） ----------

/// 进程内存占用（Linux：/proc/self/status 的 VmRSS；读不到 = None——非 Linux 或
/// 容器裁剪不假装有数）。Linux-only 是本仓既定口径（P015·A1）。
fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// 文件系统用量（libc::statvfs；目标 = plans_root 所在文件系统）。
/// plans 目录可能尚未创建（首次启动）——向上找最近存在的祖先做 statvfs，
/// 返回实际检查的路径（响应里如实报，不假装检查了 plans_root 本身）。
fn disk_usage(path: &std::path::Path) -> Option<(String, u64, u64)> {
    let mut probe = path.to_path_buf();
    loop {
        if probe.exists() {
            break;
        }
        probe = probe.parent()?.to_path_buf();
    }
    let c = std::ffi::CString::new(probe.to_str()?).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    let block = st.f_frsize as u64;
    Some((
        probe.display().to_string(),
        st.f_bavail as u64 * block,
        st.f_blocks as u64 * block,
    ))
}

/// `GET /server/ops`：运维信息带（Dashboard 顶部）——版本 / 启动时刻 / uptime /
/// 内存 RSS / plans_root 所在文件系统用量。
/// 时区纪律（P019 定稿）：时间字段一律 unix ms（UTC 口径），永不输出裸时间字符串。
pub async fn server_ops(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let now = crate::domain::now_ms();
    let uptime_ms = now.saturating_sub(st.started_at_ms);
    let memory = rss_bytes().map(|rss_bytes| {
        json!({ "rss_bytes": rss_bytes, "source": "/proc/self/status VmRSS" })
    });
    let disk = disk_usage(&st.plans_root).map(|(checked, available_bytes, total_bytes)| {
        json!({
            "path": checked,
            "available_bytes": available_bytes,
            "total_bytes": total_bytes,
        })
    });
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "started_at_ms": st.started_at_ms,
        "uptime_ms": uptime_ms,
        "memory": memory,
        "disk": disk,
    }))
}

// ---------- 配置：Agent 只读视图（A3 · P019） ----------

/// `GET /config/agents`：Agent 管理页属性面板数据源——manifest 只读序列化。
///
/// 每个 agent：描述（role_prompt 全文，前端自折叠）/ 工具面（extra_packages /
/// skills / mcp / path_permissions）/ 思考档位 / 模型绑定（id + 池条目解引用出
/// provider/model_id/base_url）。附带 sops 清单（剧本规划页复用）与模型池全量
/// （换模型的候选列表）。
///
/// 口径：只读，无 key 类字段（嵌套结构本无 key；顶层 api_key 不在本视图）。
/// 「流程出场」无结构化数据源（SOP 的 agent 绑定在实现层硬编码）——不在本端点
/// 编造，归 A9 SOP 序列化时定数据源。
pub async fn agents_config(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let m = st.manifest.lock().unwrap_or_else(|e| e.into_inner());
    let agents: Vec<serde_json::Value> = m
        .agents
        .iter()
        .map(|a| {
            let pool = m.model_pool.iter().find(|e| e.id == a.model);
            json!({
                "name": a.name,
                "role_prompt": a.role_prompt,
                "extra_packages": a.extra_packages,
                "skills": a.skills,
                "mcp": a.mcp,
                "thinking_level": a.thinking_level,
                "path_permissions": a.path_permissions,
                "model": a.model,
                "model_detail": pool,
            })
        })
        .collect();
    let sops: Vec<serde_json::Value> = st
        .registry
        .all()
        .iter()
        .map(|s| json!({ "id": s.id, "name": s.name, "description": s.description }))
        .collect();
    Json(json!({
        "agents": agents,
        "sops": sops,
        "model_pool": m.model_pool,
    }))
}

// ---------- 可观测：统计聚合（A5 · P019） ----------

/// unix ms → UTC 日期串（`YYYY-MM-DD`；复用 change_id 的 civil_from_days——
/// 同一算法单一实现，不引第三方时间库）。时区纪律：分桶按 UTC，前端本地渲染。
fn utc_date_string(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let (y, m, d) = crate::change::change_id::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `GET /stats?days=N`（缺省 14，clamp 1..=90）：Dashboard 图表数据——
/// 按天 run 成功率 / 时长趋势 / 任务态分布。数据源 = store 现有 runs/tasks
/// （不新建数据源）；分桶按 `ended_at_ms` 的 UTC 日历日，无终态的 run 不进
/// 日桶（计入 totals.running）。
pub async fn stats(
    State(st): State<SharedState>,
    axum::extract::Query(q): axum::extract::Query<StatsQuery>,
) -> Json<serde_json::Value> {
    let days = q.days.unwrap_or(14).clamp(1, 90);
    let now = crate::domain::now_ms();
    let window_start = now.saturating_sub(days * 86_400_000);

    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());

    // 按天分桶（BTreeMap 保证日期有序——前端折线不用再排）
    #[derive(Default)]
    struct DayBucket {
        settled: u64,
        failed: u64,
        durations_ms: Vec<u64>,
    }
    let mut daily: std::collections::BTreeMap<String, DayBucket> = std::collections::BTreeMap::new();
    let mut totals = serde_json::Map::new();
    let (mut t_settled, mut t_failed, mut t_running) = (0u64, 0u64, 0u64);

    for run in &store.runs {
        match run.status {
            crate::domain::run::RunStatus::Running => t_running += 1,
            crate::domain::run::RunStatus::Settled => t_settled += 1,
            crate::domain::run::RunStatus::Failed => t_failed += 1,
        }
        // 日桶：只收窗口内有终态的 run（成功率/时长趋势都是终态语义）
        let Some(ended) = run.ended_at_ms else { continue };
        if ended < window_start {
            continue;
        }
        let bucket = daily.entry(utc_date_string(ended)).or_default();
        match run.status {
            crate::domain::run::RunStatus::Settled => {
                bucket.settled += 1;
                bucket
                    .durations_ms
                    .push(ended.saturating_sub(run.started_at_ms));
            }
            crate::domain::run::RunStatus::Failed => bucket.failed += 1,
            crate::domain::run::RunStatus::Running => {}
        }
    }

    let daily_json: Vec<serde_json::Value> = daily
        .into_iter()
        .map(|(date, b)| {
            let terminal = b.settled + b.failed;
            let success_rate = if terminal > 0 {
                serde_json::json!(b.settled as f64 / terminal as f64)
            } else {
                serde_json::Value::Null
            };
            let avg = if b.durations_ms.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!(
                    b.durations_ms.iter().sum::<u64>() / b.durations_ms.len() as u64
                )
            };
            json!({
                "date": date,
                "runs_total": terminal,
                "settled": b.settled,
                "failed": b.failed,
                "success_rate": success_rate,
                "avg_duration_ms": avg,
            })
        })
        .collect();

    // 任务态分布（六态全列出——零也列出，前端图表键稳定）
    let mut dist = serde_json::Map::new();
    for s in [
        "pending",
        "running",
        "completed",
        "failed",
        "archived",
        "blocked",
    ] {
        dist.insert(s.to_string(), json!(0));
    }
    for t in &store.tasks {
        let key = match t.status {
            crate::domain::task::TaskStatus::Pending => "pending",
            crate::domain::task::TaskStatus::Running => "running",
            crate::domain::task::TaskStatus::Completed => "completed",
            crate::domain::task::TaskStatus::Failed => "failed",
            crate::domain::task::TaskStatus::Archived => "archived",
            crate::domain::task::TaskStatus::Blocked => "blocked",
        };
        let cur = dist.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        dist.insert(key.to_string(), json!(cur + 1));
    }
    totals.insert(
        "runs".into(),
        json!(t_settled + t_failed + t_running),
    );
    totals.insert("settled".into(), json!(t_settled));
    totals.insert("failed".into(), json!(t_failed));
    totals.insert("running".into(), json!(t_running));

    Json(json!({
        "range_days": days,
        "generated_at_ms": now,
        "daily": daily_json,
        "task_status_distribution": dist,
        "totals": totals,
    }))
}

/// `GET /stats` 查询参数。
#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    pub days: Option<u64>,
}

// ---------- 可观测：日志查询（A8 · P019） ----------

/// Howard Hinnant days_from_civil（civil_from_days 的逆）：UTC 日历日 → 自
/// 1970-01-01 的天数。与 change_id::civil_from_days 配对使用。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// tracing JSON 行的 `timestamp`（RFC3339 Zulu，如 `2026-10-31T15:10:07.099597Z`）
/// → unix ms。手写解析（不引第三方时间库）；畸形输入 = None。
pub(crate) fn rfc3339_zulu_to_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':'
    {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: u64 = s.get(14..16)?.parse().ok()?;
    let sec: u64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&mo) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    // 月内天数（含闰年规则）——日历合法才进天序换算
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim: [u32; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if d < 1 || d > dim[(mo - 1) as usize] {
        return None;
    }
    // 小数秒（可选）：取前 6 位补零到微秒精度再换算毫秒
    let mut frac_ms: u64 = 0;
    if let Some(rest) = s.get(19..)
        && let Some(frac) = rest.strip_prefix('.')
    {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let padded = format!("{:0<6}", &digits[..digits.len().min(6)]);
            frac_ms = padded.parse::<u64>().unwrap_or(0) / 1_000;
        }
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3_600 + (mi * 60 + sec) as i64;
    Some((secs as u64) * 1_000 + frac_ms)
}

/// 日志级别下限（ERROR > WARN > INFO > DEBUG）；未知级别视作最低（不被 level 过滤滤掉）。
fn level_rank(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 3,
        "WARN" => 2,
        "INFO" => 1,
        _ => 0,
    }
}

/// 日志查询参数（A8）。
#[derive(Debug, Clone, Deserialize)]
pub struct LogsQuery {
    /// 级别下限（error/warn/info/debug；匹配该级及以上）。
    pub level: Option<String>,
    /// `fields.component` 精确匹配。
    pub component: Option<String>,
    /// 关键词（整行小写包含）。
    pub q: Option<String>,
    /// run_id（整行包含——引擎行在任意字段携带 run_id 时可命中）。
    pub run_id: Option<String>,
    /// 时间窗下限（unix ms；基于行 timestamp 解析，无法解析时间戳的行**保留**——
    /// 日志查询丢行是事故，宁多勿丢）。
    pub since_ms: Option<u64>,
    /// 时间窗上限（unix ms，含端点）。
    pub until_ms: Option<u64>,
    /// 分页偏移（匹配集内，文件序旧→新）。
    pub offset: Option<usize>,
    /// 页大小（缺省 200，clamp 1..=1000）。
    pub limit: Option<usize>,
}

/// 单行是否命中过滤（纯函数，可测）。`raw` = 原始行文本（q/run_id 全行匹配用）。
pub(crate) fn log_line_matches(
    line: &serde_json::Value,
    raw: &str,
    q: &LogsQuery,
    since_ms: Option<u64>,
    until_ms: Option<u64>,
) -> bool {
    if let Some(want) = &q.level
        && level_rank(line["level"].as_str().unwrap_or("")) < level_rank(want)
    {
        return false;
    }
    if let Some(want) = &q.component
        && line["fields"]["component"].as_str() != Some(want.as_str())
    {
        return false;
    }
    let lower = raw.to_ascii_lowercase();
    if let Some(want) = &q.q
        && !lower.contains(&want.to_ascii_lowercase())
    {
        return false;
    }
    if let Some(want) = &q.run_id
        && !raw.contains(want.as_str())
    {
        return false;
    }
    let ts = line["timestamp"].as_str().and_then(rfc3339_zulu_to_ms);
    if let Some(since) = since_ms
        && let Some(ts) = ts
        && ts < since
    {
        return false;
    }
    if let Some(until) = until_ms
        && let Some(ts) = ts
        && ts > until
    {
        return false;
    }
    true
}

/// `GET /logs`：引擎事件流查询（`<logs_root>/devforge.log.jsonl*`）——
/// 级别 / 组件 / 关键词 / run_id / 时间窗过滤 + 分页（时间序旧→新）。
///
/// 口径：全文件扫描后过滤；时间窗过滤对无法解析时间戳的行放行（宁多勿丢）；
/// 弹窗看完整命令与输出 = 本端点（引擎面）+ 既有
/// `/projects/.../runs/{run}/transcript`（对话面）双源拼装。
/// 收尾批（2026-11-01）：轮转感知——运维轮转（logrotate 形态）留下的
/// `devforge.log.jsonl.*` 历史文件一并归并扫描（gz 读不了跳过），
/// 分页语义跨文件连续；响应新增 `files` 列表（实际扫描的文件名）。
pub async fn logs_query(
    State(st): State<SharedState>,
    Query(q): Query<LogsQuery>,
) -> Json<serde_json::Value> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let all_files = rotated_log_files(&st.logs_root);
    // R5-EX-7/8：文件个数上限——轮转副本无限累积时不再全量扫（保留最新 N 个，
    // 响应如实标记截断）。旧→新排序下截最旧。
    const LOG_SCAN_MAX_FILES: usize = 32;
    let files_truncated = all_files.len() > LOG_SCAN_MAX_FILES;
    let files: Vec<_> = if files_truncated {
        all_files[all_files.len() - LOG_SCAN_MAX_FILES..].to_vec()
    } else {
        all_files
    };
    let mut matched: Vec<serde_json::Value> = Vec::new();
    let mut total_matched: usize = 0;
    for path in &files {
        // R5-EX-7/8：惰性行迭代（BufReader）替代整文件 read_to_string——
        // 大文件不再整体入内存，内存从 O(文件) 降到 O(行)
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        let reader = std::io::BufReader::new(file);
        for line in std::io::BufRead::lines(reader) {
            let Ok(line) = line else {
                continue; // 读取错误行跳过（同损坏行口径）
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue; // 非法行（半写/损坏）跳过——查询面不做修复
            };
            if log_line_matches(&v, &line, &q, q.since_ms, q.until_ms) {
                total_matched += 1;
                if total_matched > offset && matched.len() < limit {
                    let mut row = v;
                    if let Some(ts) = row["timestamp"].as_str().and_then(rfc3339_zulu_to_ms) {
                        row["timestamp_ms"] = json!(ts);
                    }
                    matched.push(row);
                }
            }
        }
    }
    let scanned: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .collect();
    Json(json!({
        "total_matched": total_matched,
        "offset": offset,
        "limit": limit,
        "lines": matched,
        "files": scanned,
        "files_truncated": files_truncated,
    }))
}

/// 轮转感知（收尾批 2026-11-01）：logs_root 下的引擎日志文件清单，按
/// (mtime, 文件名) 旧→新排序——分页「文件序旧→新」的语义在多文件下保持。
/// 只收未压缩文本（`devforge.log.jsonl` 及其 `.*` 后缀轮转副本；`.gz` 读不了
/// 跳过，响应 files 列表如实反映实际扫描范围）。
fn rotated_log_files(logs_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(logs_root) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            (name == "devforge.log.jsonl" || name.starts_with("devforge.log.jsonl."))
                && p.is_file()
                && !name.ends_with(".gz")
        })
        .filter_map(|p| {
            let mtime = p.metadata().and_then(|m| m.modified()).ok()?;
            Some((mtime, p))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    files.into_iter().map(|(_, p)| p).collect()
}

// ---------- 配置：SOP 只读序列化（A9 · P019） ----------

/// `GET /sops`：剧本规划页数据源——SOP 定义 + agent 出场表（A9）。
///
/// 口径（侦察结论，2026-10-31）：SOP 的结构化面 = id/name/description + agent
/// 出场表（[`SopRegistry::agent_appearances`]，资产层事实的人工编码，守卫测试
/// 钉 agent 名存在于 manifest）；**节点图/门序列无结构化数据源**（编排逻辑在
/// 编排器代码里显式接线）——不编造节点图，前端剧本页的「只读编排图」以出场
/// 表为节点集，图结构待编排器数据化后另立任务。
pub async fn sops_list(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let sops: Vec<serde_json::Value> = st
        .registry
        .all()
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
                "agents": st.registry.agent_appearances(s.id),
            })
        })
        .collect();
    Json(json!({ "sops": sops }))
}

// ---------- 配置：依赖管理读面（A4 · P019） ----------

/// `GET /dependencies`：外部依赖清单 + 存活/版本探测 + 日志桥接（A4 读面）。
///
/// 口径（P019 定稿）：只读监控——无卸载、无 AI 自动修复；下载/更新是写操作
/// +长任务，留待后续任务化。探测带 3s 超时（不卡 HTTP 请求）；清单静态写死
/// （pi/codegraph/git），日志候选如实报存在性（「没有日志的就算了」）。
pub async fn dependencies_list(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let deps = crate::server::dependencies::list_dependencies(st.dependency_probe.as_ref());
    Json(json!({ "dependencies": deps }))
}

/// `POST /dependencies/{agent}/re-render`（A4 写面 · 2026-11-01 拍板）：
/// profile 依赖修复——**人显式点的按钮**（A1 审计留痕），引擎只执行确定性的
/// 重渲染（与 P019「无 AI 自动修复」不冲突）。
///
/// 复用 put_manifest 的后台渲染管线：render_gate 单飞 + 合并 + panic 守卫 +
/// 幂等跳过（rendered_fingerprint）+ 双记录对账 + pi install 补装；完成/失败
/// 走 SSE `config.rendered` / `config.render_failed`。`{agent}` 用于存在性
/// 校验与审计语义；渲染走全量幂等管线——完好的 profile 自动跳过，只修红的。
/// 外部二进制（pi/codegraph/git）的升级不做：安装方式耦合，归运维/部署期。
pub async fn dependency_rerender(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    axum::extract::Path(agent): axum::extract::Path<String>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // 打错名的修复按钮 = 404（不是静默全量渲染）
    let known = st
        .manifest
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .agents
        .iter()
        .any(|a| a.name == agent);
    if !known {
        return Err(ApiError::not_found(format!(
            "agent `{agent}` 不在清单里，无法触发 profile 重渲染"
        )));
    }
    // 渲染全局单飞：在途时合并（与 put_manifest 同款语义——在跑轮末自动重渲最新清单）
    if !st.render_gate.try_begin_or_coalesce() {
        tracing::info!(
            component = "server",
            event = "dependency.rerender.coalesced",
            agent = %agent,
            "渲染已在途——修复请求合并进在跑那一轮"
        );
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({"ok": true, "async": true, "coalesced": true})),
        ));
    }
    let st2 = st.clone();
    std::thread::spawn(move || {
        let mut guard = RenderInFlightGuard {
            st: st2.clone(),
            finished: false,
        };
        loop {
            let st3 = st2.clone();
            render_body_without_guard(st2.clone(), move |v| st3.emit(v));
            if !st2.render_gate.finish_round_or_continue() {
                guard.finished = true;
                break;
            }
        }
    });
    user.audit_write(
        &st,
        "dependency_rerender",
        &format!("profile 依赖修复触发（agent={agent}）"),
    );
    tracing::info!(
        component = "server",
        event = "dependency.rerender",
        agent = %agent,
        "profile 依赖修复：后台重渲染开始"
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"ok": true, "async": true, "agent": agent})),
    ))
}

// ---------- 配置：集成开关读写（A10 · P019） ----------

/// `GET /integrations/settings`：集成开关读面——文件缺失/损坏回落全 off
/// （ui-9 口径：「文件不存在 = 功能不存在」）。消费方（调度器 tick 热读）
/// 归 gap-E auto-integrate 落地时接线。
pub async fn get_integration_settings(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let settings = crate::integration::settings::load(&crate::integration::settings::settings_path(
        &st.plans_root,
    ));
    Json(serde_json::to_value(settings).unwrap_or(serde_json::Value::Null))
}

/// `PUT /integrations/settings`：集成开关写面——原子写（tmp + rename），
/// 半写文件永不被 tick 读走。写操作审计（A1）。
#[derive(Debug, Deserialize)]
pub struct PutIntegrationSettingsReq {
    pub auto_integrate_enabled: bool,
    pub gates: crate::integration::settings::IntegrationGates,
}

pub async fn put_integration_settings(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    axum::Json(req): axum::Json<PutIntegrationSettingsReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let settings = crate::integration::settings::IntegrationSettings {
        auto_integrate_enabled: req.auto_integrate_enabled,
        gates: req.gates,
    };
    crate::integration::settings::save(
        &crate::integration::settings::settings_path(&st.plans_root),
        &settings,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    user.audit_write(
        &st,
        "put_integration_settings",
        &format!(
            "auto_integrate={}, gates(clippy={},doc_lint={})",
            settings.auto_integrate_enabled, settings.gates.clippy, settings.gates.doc_lint
        ),
    );
    Ok(Json(serde_json::to_value(settings).unwrap_or(serde_json::Value::Null)))
}

// ---------- P020 反馈系统：issues ----------
// 2026-11-01 审计补丁（R3-15）：issue_err（中文关键词猜状态码）已退役——
// store 的 issue 域改投 NotFound/Conflict/InvalidInput 变体，ApiError::from 变体优先直映射。

#[derive(Deserialize)]
pub struct CreateIssueReq {
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub severity: crate::domain::IssueSeverity,
    pub category: crate::domain::IssueCategory,
    #[serde(default)]
    pub run_refs: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// `POST /projects/{project}/issues`：操作员报 issue（source=Operator，key 项目前缀序分配）。
pub async fn create_issue(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Path(project): Path<String>,
    Json(req): Json<CreateIssueReq>,
) -> ApiResult<(StatusCode, Json<crate::domain::Issue>)> {
    let issue = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .create_issue(
                &project,
                req.title,
                req.body,
                crate::domain::IssueSource::Operator,
                req.severity,
                req.category,
                req.run_refs,
                req.evidence,
            )
            ?
    };
    user.audit_write(&st, "create_issue", &format!("{project}/{}", issue.key));
    // SSE：issue.created（前端按 issue. 前缀订阅）
    st.emit(json!({
        "type": "issue.created",
        "key": issue.key,
        "project": project,
        "severity": issue.severity,
        "category": issue.category,
        "source": issue.source,
        "title": issue.title,
    }));
    // A2：critical 级 = 运营者必须马上知道（channel_breaker 同款内联通知）
    if issue.severity == crate::domain::IssueSeverity::Critical {
        crate::server::notify(
            &st,
            "issue",
            "error",
            &format!("Critical issue {}: {}", issue.key, issue.title),
            Some(&format!("{project}/{}", issue.key)),
        );
    }
    Ok((StatusCode::CREATED, Json(issue)))
}

/// `GET /projects/{project}/issues`：列表（status/severity/category 过滤 + 分页）。
#[derive(Deserialize)]
pub struct ListIssuesQuery {
    pub status: Option<crate::domain::IssueStatus>,
    pub severity: Option<crate::domain::IssueSeverity>,
    pub category: Option<crate::domain::IssueCategory>,
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

pub async fn list_issues(
    State(st): State<SharedState>,
    Path(project): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ListIssuesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
    let all: Vec<&crate::domain::Issue> = store
        .issues
        .iter()
        .filter(|i| i.project == project)
        .filter(|i| q.status.is_none_or(|s| i.status == s))
        .filter(|i| q.severity.is_none_or(|s| i.severity == s))
        .filter(|i| q.category.is_none_or(|c| i.category == c))
        .collect();
    let total = all.len();
    let items: Vec<crate::domain::Issue> = all
        .into_iter()
        .skip(q.offset)
        .take(limit)
        .cloned()
        .collect();
    Ok(Json(json!({ "total": total, "offset": q.offset, "limit": limit, "issues": items })))
}

/// `GET /issues/{key}`：详情（含 timeline）。
pub async fn get_issue(
    State(st): State<SharedState>,
    Path(key): Path<String>,
) -> ApiResult<Json<crate::domain::Issue>> {
    let issue = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.get_issue(&key).cloned()
    };
    issue
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("issue `{key}` 不存在")))
}

/// `PATCH /issues/{key}`：状态流转 + 字段编辑（双半边，乐观锁）。
/// status 与 title/body/severity/category 可任选（同传 = 先流转后编辑，version 链式推进）；
/// Queued→Resolved 人工不可手标——由 run 联动（k4）。
#[derive(Deserialize)]
pub struct PatchIssueReq {
    #[serde(default)]
    pub status: Option<crate::domain::IssueStatus>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub severity: Option<crate::domain::IssueSeverity>,
    #[serde(default)]
    pub category: Option<crate::domain::IssueCategory>,
    pub version: u64,
}

pub async fn patch_issue(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Path(key): Path<String>,
    Json(req): Json<PatchIssueReq>,
) -> ApiResult<Json<crate::domain::Issue>> {
    let patch = crate::domain::issue::IssueFieldPatch {
        title: req.title.clone(),
        body: req.body.clone(),
        severity: req.severity,
        category: req.category,
    };
    if req.status.is_none() && patch.is_empty() {
        return Err(ApiError::bad_request(
            "PATCH 补丁为空：需要 status 或 title/body/severity/category 至少一项".to_string(),
        ));
    }
    let issue = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        // ① 流转半边（可选）：校验与 version bump 都在 store
        let mut current = match req.status {
            Some(to) => store
                .transition_issue(&key, to, req.version, &user.username)
                ?,
            None => store
                .get_issue(&key)
                .cloned()
                .ok_or_else(|| ApiError::not_found(format!("issue `{key}` 不存在")))?,
        };
        // ② 字段编辑半边（可选）：无流转时用 req.version 做乐观锁校验
        if !patch.is_empty() {
            let expected = if req.status.is_some() { current.version } else { req.version };
            current = store
                .update_issue_fields(&key, &patch, expected, &user.username)
                ?;
        }
        current
    };
    if let Some(to) = req.status {
        user.audit_write(&st, "patch_issue", &format!("{key} → {to:?}"));
        st.emit(json!({
            "type": "issue.transitioned",
            "key": key,
            "to": to,
            "version": issue.version,
        }));
    }
    if !patch.is_empty() {
        user.audit_write(&st, "edit_issue", &format!("{key} v{}", issue.version));
        st.emit(json!({
            "type": "issue.edited",
            "key": key,
            "version": issue.version,
        }));
    }
    Ok(Json(issue))
}

/// `POST /issues/{key}/convert`：转 task（复用 create_task 全套校验；issue 进 Queued）。
#[derive(Deserialize)]
pub struct ConvertIssueReq {
    /// task id 缺省 = issue key 小写加 `-fix`（如 `devforge-3-fix`）。
    #[serde(default)]
    pub task_id: Option<String>,
    /// 缺省 change SOP（修复走 change 流程）。
    #[serde(default)]
    pub sop: Option<String>,
    /// 契约（可选——留空交给 plan agent 规划，F12/白名单门照拦）。
    #[serde(default)]
    pub contract: Option<crate::domain::contract::WorkItemContract>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub exclusive: bool,
    /// workspace 覆盖（缺省用项目登记路径）。
    #[serde(default)]
    pub workspace: Option<String>,
}

pub async fn convert_issue(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
    Path(key): Path<String>,
    Json(req): Json<ConvertIssueReq>,
) -> ApiResult<(StatusCode, Json<Task>)> {
    let (project, issue_key) = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let Some(issue) = store.get_issue(&key) else {
            return Err(ApiError::not_found(format!("issue `{key}` 不存在")));
        };
        (issue.project.clone(), issue.key.clone())
    };
    let task_id = req
        .task_id
        .unwrap_or_else(|| format!("{}-fix", key.to_lowercase()));
    let sop = req
        .sop
        .unwrap_or_else(|| crate::change::state::CHANGE_SOP.to_string());
    // 2026-11-01 审计批次一（EX-1）：workspace 覆盖与项目注册同防线——门 0 会在无 .git
    // 的目录 git init + add -A 建基线，任意目录不可经 convert 绕过校验。
    if let Some(ws) = &req.workspace {
        validate_local_path(std::path::Path::new(ws)).map_err(ApiError::bad_request)?;
    }
    // （R2-10）base_rev 预检对齐 create_task：探测目录 = workspace 覆盖后的最终仓
    if let Some(contract) = &req.contract
        && let Some(base_rev) = &contract.base_rev
    {
        let probe_dir = match &req.workspace {
            Some(ws) => Some(std::path::PathBuf::from(ws)),
            None => {
                let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                store.get_project(&project).map(|p| p.local_path.clone())
            }
        }
        .ok_or_else(|| ApiError::not_found(format!("项目 `{project}` 不存在")))?;
        let probe = std::process::Command::new("git")
            .args([
                "-C",
                &probe_dir.to_string_lossy(),
                "cat-file",
                "-e",
                &format!("{base_rev}^{{commit}}"),
            ])
            .output();
        let ok = matches!(probe, Ok(o) if o.status.success());
        if !ok {
            return Err(ApiError::bad_request(format!(
                "contract.base_rev `{base_rev}` 不是仓库 `{}` 里的合法 commit",
                probe_dir.display()
            )));
        }
    }
    let created = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let registry = st.registry.clone();
        let contract = req.contract.clone();
        let deps = crate::domain::task::TaskDeps {
            depends_on: req.depends_on.clone(),
            exclusive: req.exclusive,
        };
        // （EX-2 前置）非 Triaged 在 create_task 之前拒——最常见的失败面不再产生孤儿
        let issue_status = store.get_issue(&key).map(|i| i.status);
        if issue_status != Some(crate::domain::IssueStatus::Triaged) {
            return Err(ApiError::bad_request(format!(
                "issue `{key}` 非 Triaged（当前 {issue_status:?}）——先分诊再转化"
            )));
        }
        store
            .create_task(
                &registry,
                &project,
                &task_id,
                &sop,
                contract,
                deps,
            )?;
        // 可选 workspace 覆盖（create_task 快照项目路径后的补充指定）
        if let Some(ws) = &req.workspace
            && let Some(t) = store
                .tasks
                .iter_mut()
                .find(|t| t.project == project && t.id == task_id)
        {
            t.workspace = std::path::PathBuf::from(ws);
        }
        // （EX-2 兑底）link 失败回滚：删掉刚建的任务，不留孤儿
        if let Err(e) = store.link_issue_task(&key, &task_id, &user.username) {
            store.remove_task(&project, &task_id);
            return Err(e.into());
        }
        // link 之后再取：task.issue_ref 已回填（返回值带完整引用关系）
        store
            .get_task(&project, &task_id)
            .cloned()
            .ok_or_else(|| ApiError::internal("task 创建后不可见（store 不一致）"))
    };
    user.audit_write(
        &st,
        "convert_issue",
        &format!("{issue_key} → {project}/{task_id}"),
    );
    st.emit(json!({
        "type": "issue.transitioned",
        "key": issue_key,
        "to": "queued",
        "task": task_id,
    }));
    Ok((StatusCode::CREATED, Json(created?)))
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
    Extension(user): Extension<AuthedUser>,
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
    if let Some(contract) = &req.contract
        && let Some(base_rev) = &contract.base_rev
    {
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
    user.audit_write(&st, "create_task", &format!("{project}/{}", created.id));
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn cancel_task(
    State(st): State<SharedState>,
    Extension(user): Extension<AuthedUser>,
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
            user.audit_write(&st, "cancel_task", &format!("{project}/{task}"));
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
    Extension(user): Extension<AuthedUser>,
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
        // A6：归档项目的 start 拒绝（409）——归档语义的准入面。
        if store
            .get_project(&project)
            .is_some_and(|p| p.archived)
        {
            return Err(ApiError::conflict(format!(
                "项目 `{project}` 已归档——先取消归档再启动任务"
            )));
        }
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
                if let Some(old) = previous_sop.clone()
                    && let Err(re) = store.set_task_sop(&st.registry, &project, &task, old)
                {
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
            if let Some(old) = previous_sop.clone()
                && let Err(re) = store.set_task_sop(&st.registry, &project, &task, old)
            {
                tracing::error!(
                    component = "server",
                    event = "start.sop_rollback_failed",
                    project = %project,
                    task = %task,
                    error = %re,
                    "续跑门失败后回滚 SOP 覆盖失败——任务身份可能已被改写"
                );
            }
            drop(store);
            // P020：issue 联动（Failed 回炉）——放 store 作用域后，避免 Mutex 重入
            crate::server::issue_run_outcome_linkage(
                &st,
                &project,
                &task,
                &run.id.0,
                false,
                &e.to_string(),
            );
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
        if change_root.is_dir()
            && let Err(e) = std::fs::remove_dir_all(&change_root)
        {
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
            // P020：issue 联动（Failed 回炉）
            crate::server::issue_run_outcome_linkage(
                &st,
                &project,
                &task,
                &run.id.0,
                false,
                &format!("清空 change 黑板失败：{e}"),
            );
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
    let mut engine = (st.engine_factory)();
    // P020-k5：agent 上报面注入（引擎捕获 submit_issue → store，免 API 鉴权）+ issue 快照写黑板
    engine.set_issue_sink(Some(std::sync::Arc::new({
        let st_sink = st.clone();
        move |rep| crate::server::submit_agent_issue(&st_sink, rep)
    })));
    crate::server::write_issue_snapshot(&st, &project, &task);
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
    user.audit_write(
        &st,
        "start_task",
        &format!("{project}/{task} → run {run_id}（sop={sop_name}, resumed={}）", decision.resumed),
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
    // 心跳（run 活性）——R5-P54：读侧与写侧同源：优先 run 开跑时捕获的观测根
    // （多实例/fixture 错位时与心跳写侧一致），未登记（已收尾 run）回退实例根
    let hb_root = crate::observability::run_root(&run_id).unwrap_or_else(|| obs_root_of(&st));
    let hb = crate::observability::heartbeat::read(&hb_root, &run_id).ok();
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
    // 收集本 run 的会话文件（R5-P44/P53：布局单一事实源——
    // `<sessions_root>/<run_id>/<role>/<文件>.jsonl`，旧口径单层 read_dir 把
    // 第一层目录名（run_id）当角色比较 → role 过滤永不生效、files 恒空）。
    // 递归两层：第一层 = 角色目录（role 过滤在此），第二层 = 会话文件。
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let run_sessions = crate::engine::session_root_for(&st.sessions_root, &run_id);
    if let Ok(rd) = std::fs::read_dir(&run_sessions) {
        for role_dir in rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
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
    Extension(user): Extension<AuthedUser>,
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
    user.audit_write(&st, "answer_ruling", &format!("ui/{id}"));
    Ok(Json(json!({"ok": true})))
}

/// 待答清单（GET /ui/pending）：当前所有等应答的 UI 请求。
///
/// 响应是**裸数组**（`[{id, run_id, question, age_ms}]`）——按需求字面取，不包
/// `{"pending": [...]}`：这是与仓内其它列表端点（`list_projects` 裹一层）不同的
/// **明确裁决**，不要顺手改回包裹形态。空表返回 `[]`（不是 404：清单端点的语义是
/// 「当前没有待答」，不是「端点不存在」）。
///
/// id 与 SSE `ui.request` 的 id、`POST /ui/{id}/answer` 的路径参数同源——监工的
/// 「从事件看到 id → 查清单 → 应答」这条链路靠它接上。
pub async fn list_pending_ui(State(st): State<SharedState>) -> Json<Vec<PendingAskView>> {
    Json(st.ui.pending_snapshot())
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
    let (replay, replay_gap) = st.bus.replay_after_with_gap(last_id);
    // 2026-10-27 自审 major：去重阈值 = **实际回放过的最大序号**；回放为空时必须归零。
    // 旧口径回退成客户端的 Last-Event-ID——服务重启后序号重置，客户端带着旧大序号重连，
    // 全部新序号实时事件 ≤ 旧大序号 → 被永久丢弃。空回放 = 无重叠可能 → 不该过滤任何实时事件。
    let replay_max = replay.last().map(|(id, _)| *id).unwrap_or(0);
    // 外部审计 #8（2026-10-28）：缺口必须作为元事件先行，客户端才能区分「没有新事件」
    // 与「事件已被挤出留档窗口」。合成事件 id = 缺口上界：客户端再断线时以它做
    // Last-Event-ID，回放仍返回同一批幸存事件且不再报缺口（收敛，不循环）。
    let gap_head = replay_gap.map(|(from, to)| {
        let data = serde_json::json!({
            "type": "events.dropped",
            "missing_from": from,
            "missing_to": to,
            "hint": "留档窗口已滚动，这段事件无法回放",
        })
        .to_string();
        Ok(
            axum::response::sse::Event::default()
                .id(to.to_string())
                .data(data),
        )
    });
    let replayed = tokio_stream::iter(gap_head.into_iter().chain(replay.into_iter().map(|(id, data)| {
        Ok(axum::response::sse::Event::default()
            .id(id.to_string())
            .data(data))
    })));
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
    Extension(user): Extension<AuthedUser>,
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
    user.audit_write(&st, "put_manifest", &format!("模型/配置交换（agents={agents}）"));
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
    Extension(user): Extension<AuthedUser>,
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
    // R3-16/EX-13 审计系统化：手动入队直接通向自动合并链，必须有 actor 可追
    user.audit_write(
        &st,
        "enqueue_candidate",
        &format!("{project}/{} @ {}", req.task_id, &req.frozen_sha[..8.min(req.frozen_sha.len())]),
    );
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
