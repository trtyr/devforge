---
type: module
module_path: src/domain/auth.rs
unit: pending-inventory
description: A1 认证域——admin 账户（argon2id）、角色、审计事件与会话令牌生成
---

# src/domain/auth.rs

A1 认证（P019 M-1）的领域层：单 admin 账户模型 + 审计事件结构 + 口令哈希工具。
存储落在 `Store` 的 `auth` 段与 `auth_events` 段（见 src/domain/store.rs）；HTTP 面在 src/server/auth.rs
（会话签发/验证）与 handlers.rs（登录端点、守门中间件）。

## 职责

**负责**：
- `Role`（admin 全权 / observer 预留——守门暂不区分，降权后续立项）、`AuthAccount`
  （单账户，PHC 串落盘，永不回显明文）、`AuthEvent`（login_success/login_failure/write_op）。
- `hash_password` / `verify_password`：argon2id 默认参数；盐取 uuid v4×2 的 16 字节
  （不为此拉 rand 依赖链）；PHC 串自带盐与参数。损坏 PHC 串校验 = false 不 panic。
- `new_session_token`：uuid v4×2 = 256bit，cookie 与 x-devforge-token 共用形态外各走各通道。

**不负责**：会话存储与过期（server/auth.rs）；登录端点与审计写入调用（handlers.rs）；
账户初始化 CLI（main.rs auth-init）。

## 公开成员

| 成员 | 类型 |
|---|---|
| `Role` / `AuthAccount` / `AuthEvent` / `MAX_AUTH_EVENTS` | 类型与常量 |
| `hash_password` / `verify_password` / `new_session_token` | 函数 |

## 依赖

- 上游：`Store`（auth 段持久化，`#[serde(default)]` 旧 store.json 兼容）；
  `src/server/auth.rs` 与 `src/server/handlers.rs` 消费。
- 外部：`argon2` 0.5（纯 Rust）、`uuid`。

## 测试

`src/domain/auth_tests.rs` 5 例：哈希往返（随机盐）、垃圾 PHC 串、令牌形态/唯一性、store 段
serde 兼容、审计事件 cap 裁剪。
