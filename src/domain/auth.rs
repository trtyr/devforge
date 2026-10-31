//! A1 认证（P019 M-1）：admin 账户、角色、审计事件与口令哈希工具。
//!
//! 口径（M-1 定稿 2026-10-31）：
//! - **单 admin 账户**：store.json `auth` 段存 PHC 串（argon2id，盐内嵌）；
//!   `None` = 未初始化（登录端点据实回复，不假装「密码错误」）；
//! - **角色**：admin 全权；`observer` 是预留字段（序列化合法、守门暂不区分——
//!   只读观察者的降权在 M-1 之后单独立项，先让数据结构不返工）；
//! - **审计事件**：登录成功/失败与写操作落 `store.auth_events`（cap 裁剪同通知口径）；
//! - **口令哈希**：argon2id 默认参数；PHC 串自带盐与参数，升级参数 = 重哈希的
//!   事（M-1 不做）；`new_session_token` 给会话签发用（256bit 随机，uuid v4 ×2）。

use serde::{Deserialize, Serialize};

/// 角色（M-1：admin 全权；observer 预留——守门暂不区分，字段先合法存在）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 全权（唯一在用角色）。
    Admin,
    /// 只读观察者（预留：登录可过，写操作后续降权——M-1 之后立项）。
    Observer,
}

/// admin 账户（单账户模型：store.json `auth` 段，None = 未初始化）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthAccount {
    /// 登录名（M-1 单账户固定 `admin`；字段保留以便未来多账户不返工）。
    pub username: String,
    /// argon2id PHC 串（`$argon2id$…`，盐与参数内嵌——永不回显明文，也无需存明文）。
    pub password_hash: String,
    /// 角色（M-1 恒 Admin；Observer 预留）。
    pub role: Role,
    /// 创建时刻（unix ms，UTC）。
    pub created_at_ms: u64,
}

/// 审计事件（登录成功/失败、写操作——M-1「登录与全部写操作可追溯」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthEvent {
    /// 单调递增 id（store 内自增）。
    pub id: u64,
    /// 事件类别：login_success / login_failure / write_op。
    pub kind: String,
    /// 操作者（M-1 恒为账户名或 `anonymous`——失败登录可能还没过身份）。
    pub actor: String,
    /// 摘要（如 `POST /api/v1/projects`、`密码错误`；不含口令本体）。
    pub detail: String,
    /// 时刻（unix ms，UTC）。
    pub at_ms: u64,
}

/// 审计事件上限（与 MAX_NOTIFICATIONS 同口径：全量重写代价 O(条数)，无界 = 写放大）。
pub const MAX_AUTH_EVENTS: usize = 200;

/// 口令哈希（argon2id，默认参数；返回 PHC 串——盐内嵌，直接落盘）。
/// 盐取 uuid v4 ×2 的 16 字节（122bit 随机/字节，盐只需不可预测与唯一，足够），
/// 不为此多拉 rand 依赖链。
pub fn hash_password(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let mut salt_bytes = [0u8; 16];
    salt_bytes[..8].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..8]);
    salt_bytes[8..].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..8]);
    let salt = SaltString::encode_b64(&salt_bytes).expect("16 字节盐 b64 编码合法");
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2id 哈希失败（参数合法时不可达）")
        .to_string()
}

/// 口令校验（PHC 串解析失败 = 不匹配——损坏的哈希不该放任何人进来）。
pub fn verify_password(password: &str, phc: &str) -> bool {
    use argon2::password_hash::PasswordVerifier;
    let Ok(parsed) = argon2::password_hash::PasswordHash::new(phc) else {
        return false;
    };
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// 会话令牌（256bit 随机：uuid v4 ×2 拼接——cookie 与 `x-devforge-token` 共用）。
pub fn new_session_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
