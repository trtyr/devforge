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
}

impl LoginError {
    pub fn message(&self) -> &'static str {
        match self {
            LoginError::NotInitialized => "尚未初始化 admin 账户（devforge auth-init <口令>）",
            LoginError::UnknownUser => "用户名或密码不匹配",
            LoginError::BadPassword => "用户名或密码不匹配",
        }
    }

    /// 审计 kind（login_failure 统一；detail 里带具体原因）。
    pub fn detail(&self) -> &'static str {
        match self {
            LoginError::NotInitialized => "账户未初始化",
            LoginError::UnknownUser => "用户名不匹配",
            LoginError::BadPassword => "密码错误",
        }
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
    let account = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.auth_account() {
            None => return Err(LoginError::NotInitialized),
            Some(acc) => acc.clone(),
        }
    };
    if account.username != username {
        return Err(LoginError::UnknownUser);
    }
    if !crate::domain::auth::verify_password(password, &account.password_hash) {
        return Err(LoginError::BadPassword);
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

/// 验证会话（滑动续期）：token 有效且未过期 → Some((username, role))；
/// 过期条目顺手清掉（不为此单独起清扫线程）。
pub fn validate_session(
    state: &crate::server::ServerState,
    token: &str,
    now_ms: u64,
) -> Option<(String, crate::domain::auth::Role)> {
    let mut sessions = state
        .auth_sessions
        .write()
        .unwrap_or_else(|e| e.into_inner());
    // 顺手清扫：过期的全清（会话量级 = 人数量级，O(n) 无压力）
    sessions.retain(|_, s| s.expires_at_ms > now_ms);
    let session = sessions.get_mut(token)?;
    // 滑动续期
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
pub fn session_cookie_value(token: &str) -> String {
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        SESSION_TTL_MS / 1000
    )
}

/// 登出的 Set-Cookie（同形清值 + 立即过期）。
pub fn cleared_cookie_value() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

/// 回环判定：ConnectInfo 缺席（进程内 oneshot 测试）视作回环——测试即本机；
/// 生产链路 axum 恒带 ConnectInfo，以真实对端为准。
pub fn is_loopback_peer(addr: Option<SocketAddr>) -> bool {
    match addr {
        Some(a) => a.ip().is_loopback(),
        None => true,
    }
}

#[cfg(test)]
#[path = "auth_server_tests.rs"]
mod tests;
