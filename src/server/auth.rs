//! A1 认证（P019 M-1）：会话签发 / 验证 / 登出。
//!
//! 口径：
//! - **内存会话**（重启即失效 = 全员重登）：安全优先，不做跨重启会话持久化；
//! - **滑动过期**：每次验证成功续期（活跃用户不掉线）；
//! - **cookie 与 token 双通道**：浏览器走 `devforge_session` cookie（HttpOnly +
//!   SameSite=Lax，EventSource 自动携带 → SSE 握手免特判）；服务间调用继续走
//!   `x-devforge-token` 头（DR-9 口径不废）；
//! - **回环豁免**：`auth_loopback_exempt`（knobs，缺省 true = 本机免鉴权的既有
//!   体验；config 收紧 false 后本机也必须登录）。远程永不豁免。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::RwLock;

/// 会话 cookie 名（前端不需要读它——HttpOnly；登出走 POST /auth/logout）。
pub const SESSION_COOKIE: &str = "devforge_session";

/// 会话 TTL（滑动）：24h 无操作过期。
pub const SESSION_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// 一条已签发的会话（内存态）。
#[derive(Debug, Clone, PartialEq)]
pub struct AuthSession {
    pub username: String,
    pub role: crate::domain::auth::Role,
    pub expires_at_ms: u64,
}

/// 会话表（进程内；ServerState 持有）。
pub type SessionMap = RwLock<HashMap<String, AuthSession>>;

/// 登录失败原因（→ 401 文案 + 审计 detail）。
#[derive(Debug, Clone, PartialEq)]
pub enum LoginError {
    /// store 无账户（未跑 `devforge auth-init`）——文案与密码错误**刻意区分**，
    /// 否则用户分不清「该初始化」还是「密码错了」。
    NotInitialized,
    /// 用户名不匹配（单账户模型：用户名也参与校验）。
    UnknownUser,
    /// 密码错误。
    BadPassword,
    /// 登录限速中（R4-EX-3：冷却/锁定期内直接拒，不跑哈希——拒绝面与提交的
    /// 用户名/密码无关，无侧信道增益）；带建议重试等待毫秒数。
    Throttled { retry_after_ms: u64 },
}

impl LoginError {
    pub fn message(&self) -> &'static str {
        match self {
            LoginError::NotInitialized => "尚未初始化 admin 账户（devforge auth-init <口令>）",
            LoginError::UnknownUser => "用户名或密码不匹配",
            LoginError::BadPassword => "用户名或密码不匹配",
            LoginError::Throttled { .. } => "尝试过于频繁，请稍后再试",
        }
    }

    /// 审计 kind（login_failure 统一；detail 里带具体原因）。
    pub fn detail(&self) -> &'static str {
        match self {
            LoginError::NotInitialized => "账户未初始化",
            LoginError::UnknownUser => "用户名不匹配",
            LoginError::BadPassword => "密码错误",
            LoginError::Throttled { .. } => "登录限速中（冷却/锁定期拒绝）",
        }
    }
}

/// 登录限速常数（R4-EX-3/R2-04/R3-06：失败指数退避 + 连败锁定）。
pub const LOGIN_BACKOFF_BASE_MS: u64 = 500;
pub const LOGIN_BACKOFF_CAP_MS: u64 = 30 * 1000;
pub const LOGIN_LOCK_THRESHOLD: u32 = 5;
pub const LOGIN_LOCK_MS: u64 = 15 * 60 * 1000;

/// 失败后的冷却窗：500ms × 2^(n-1)，封顶 30s（n = 当前连败数）。
fn login_backoff_ms(failures: u32) -> u64 {
    let shift = failures.saturating_sub(1).min(16);
    LOGIN_BACKOFF_BASE_MS
        .saturating_mul(1u64 << shift)
        .min(LOGIN_BACKOFF_CAP_MS)
}

/// 登录限速状态（进程内；单账户模型 → 全局一枚，按 IP 计数可被伪造头绕过）。
#[derive(Debug, Default, Clone)]
pub struct LoginThrottle {
    /// 连续失败次数（成功登录清零）。
    pub consecutive_failures: u32,
    /// 冷却截止：此前的尝试直接拒（指数退避窗）。
    pub cooldown_until_ms: u64,
    /// 锁定截止：连败达阈值后的长窗口（重置 = 锁定期满后成功登录）。
    pub locked_until_ms: u64,
}

/// 限速检查：锁定/冷却中返回 `Some(剩余毫秒)`（直接拒，不跑哈希）。
fn throttle_gate(state: &crate::server::ServerState, now_ms: u64) -> Option<u64> {
    let mut th = state
        .login_throttle
        .write()
        .unwrap_or_else(|e| e.into_inner());
    if now_ms < th.locked_until_ms {
        return Some(th.locked_until_ms - now_ms);
    }
    if now_ms < th.cooldown_until_ms {
        return Some(th.cooldown_until_ms - now_ms);
    }
    th.cooldown_until_ms = 0; // 冷却已过，出冷却
    None
}

/// 失败记账：连败 +1 → 达阈值进锁定窗，否则进指数退避冷却窗。
fn record_login_failure(state: &crate::server::ServerState, now_ms: u64) {
    let mut th = state
        .login_throttle
        .write()
        .unwrap_or_else(|e| e.into_inner());
    th.consecutive_failures = th.consecutive_failures.saturating_add(1);
    if th.consecutive_failures >= LOGIN_LOCK_THRESHOLD {
        th.locked_until_ms = now_ms.saturating_add(LOGIN_LOCK_MS);
        th.cooldown_until_ms = 0;
        tracing::warn!(
            component = "auth",
            event = "login.locked",
            failures = th.consecutive_failures,
            lock_ms = LOGIN_LOCK_MS,
            "连续登录失败达阈值，账户锁定 15 分钟（进程内状态，重启即重置）"
        );
    } else {
        th.cooldown_until_ms = now_ms.saturating_add(login_backoff_ms(th.consecutive_failures));
    }
}

/// 登录：校验账户口令，成功签发会话（滑动 TTL）。失败由调用方落审计（login_failure）
/// ——本函数不碰 store 的审计段，401 响应路径保持纯粹。
pub fn login(
    state: &crate::server::ServerState,
    username: &str,
    password: &str,
    now_ms: u64,
) -> Result<(String, crate::domain::auth::Role), LoginError> {
    // 限速前置（R4-EX-3）：锁定/冷却中直接拒——不跑哈希，拒绝面与提交的
    // 用户名/密码无关，爆破者从拒绝得不到任何口令侧信息。
    if let Some(retry_after_ms) = throttle_gate(state, now_ms) {
        return Err(LoginError::Throttled { retry_after_ms });
    }
    let account = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.auth_account() {
            None => return Err(LoginError::NotInitialized),
            Some(acc) => acc.clone(),
        }
    };
    // 时序侧信道死（R3-06）：无论用户名对错**先跑**口令校验（成本恒定），
    // 用户名存在性不再可从响应时间探测。
    let password_ok = crate::domain::auth::verify_password(password, &account.password_hash);
    if account.username != username {
        record_login_failure(state, now_ms);
        return Err(LoginError::UnknownUser);
    }
    if !password_ok {
        record_login_failure(state, now_ms);
        return Err(LoginError::BadPassword);
    }
    {
        // 成功登录：限速状态清零（锁定期满后的成功 = 重置）
        let mut th = state
            .login_throttle
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *th = LoginThrottle::default();
    }
    let token = crate::domain::auth::new_session_token();
    let session = AuthSession {
        username: account.username.clone(),
        role: account.role,
        expires_at_ms: now_ms + SESSION_TTL_MS,
    };
    state
        .auth_sessions
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(token.clone(), session);
    Ok((token, account.role))
}

/// 滑动续期降频阈值（R3-14）：距过期点超过 TTL/2 的验证走纯读路径——绝大多数
/// 验证不拿写锁，写锁只服务真正需要推进过期/清扫的会话。
const SESSION_RENEW_INTERVAL_MS: u64 = SESSION_TTL_MS / 2;

/// 验证会话（读写分离 + 续期降频，R3-14）：token 有效且未过期 → Some((username, role))。
/// 快路径：读锁验证 + 距过期点超过 TTL/2 → 纯读返回（并发读互不阻塞）；
/// 慢路径：接近续期点才拿写锁（续期 + 顺手清扫过期条目，不再每次验证都扫全表）。
pub fn validate_session(
    state: &crate::server::ServerState,
    token: &str,
    now_ms: u64,
) -> Option<(String, crate::domain::auth::Role)> {
    // 快路径：读锁。会话存在且远未到续期点 → 纯读返回
    {
        let sessions = state
            .auth_sessions
            .read()
            .unwrap_or_else(|e| e.into_inner());
        match sessions.get(token) {
            Some(s) if s.expires_at_ms > now_ms
                && now_ms + SESSION_RENEW_INTERVAL_MS < s.expires_at_ms =>
            {
                return Some((s.username.clone(), s.role));
            }
            None => return None,
            _ => {} // 在续期窗内（或已过期）→ 慢路径
        }
    }
    // 慢路径：写锁续期 + 顺手清扫过期（读/写间隙可能已被登出或过期——重验）
    let mut sessions = state
        .auth_sessions
        .write()
        .unwrap_or_else(|e| e.into_inner());
    sessions.retain(|_, s| s.expires_at_ms > now_ms);
    let session = sessions.get_mut(token)?;
    session.expires_at_ms = now_ms + SESSION_TTL_MS;
    Some((session.username.clone(), session.role))
}

/// 登出：删除会话（幂等——不存在的 token 静默成功）。
pub fn logout(state: &crate::server::ServerState, token: &str) {
    state
        .auth_sessions
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(token);
}

/// 从 Cookie 头提取会话令牌（只认 `devforge_session=`；多段 cookie 容错）。
pub fn token_from_cookie(cookie_header: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix(SESSION_COOKIE)
            && let Some(v) = v.strip_prefix('=')
        {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// 组 Set-Cookie 值（HttpOnly + SameSite=Lax + Path=/ + Max-Age 与 TTL 同步）。
/// R4-EX-11/R2-14/R3-13：`secure` = 绑定非回环地址时附加 `Secure`
/// （无条件附加会弄坏本机 http 登录——浏览器拒收回环 https-only cookie）。
pub fn session_cookie_value(token: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}{secure_flag}",
        SESSION_TTL_MS / 1000
    )
}

/// 登出的 Set-Cookie（同形清值 + 立即过期）。
pub fn cleared_cookie_value(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure_flag}")
}

/// 回环判定：ConnectInfo 缺席（oneshot 测试不注入）**不豁免**（R4-EX-12/R2-15
/// fail-closed：无对端信息 = 无法证明本机）；生产链路 axum 恒带 ConnectInfo，
/// 测试模拟本机请求须显式插入回环 ConnectInfo。
pub fn is_loopback_peer(addr: Option<SocketAddr>) -> bool {
    match addr {
        Some(a) => a.ip().is_loopback(),
        None => false,
    }
}

#[cfg(test)]
#[path = "auth_server_tests.rs"]
mod tests;
