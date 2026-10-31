---
type: module
module_path: src/server/http.rs
unit: pending-inventory
description: 路由表全景与启动序（P003 服务器形态）——A 组批次后的 25+ 端点、A1 三通道守门、F14 启动注入
---

# src/server/http.rs

axum Router 组装与生产引导（serve）。2026-11-01 状态：A 组后端缺口 A1-A10
落地后的路由全景。

## 职责

**负责**：axum Router 组装（五能力域分段）、A1 守门中间件的挂载层、生产引导
（serve：bind → 调度循环监督 → 优雅停机）、ConnectInfo 注入。

**不负责**：各端点的 handler 逻辑（handlers.rs）；会话签发/验证（auth.rs）；
启动期的根解析/旋钮引导（main.rs，本模块只消费其产物）。

## 公开成员

| 成员 | 类型 |
|---|---|
| `build_router` | 函数（Router 组装） |
| `serve` | 异步函数（生产引导） |
| `parse_cli_flag` / `resolve_startup_token` | 函数（启动三件套契约） |
| `TOKEN_ENV`（已随 F14 删除）| —— |

## 依赖

- 上游：main.rs（serve 调用方）；handlers/scheduler/auth 各模块。
- 向下：axum 0.8、tokio、tokio-stream。
- 外部：无（端口监听除外）。

## 路由分段（五个能力域）

- **编排控制**：/projects CRUD + archive/delete（A6）；/projects/{name}/tasks
  CRUD + start/cancel；/runs/{run}/cancel。
- **产物管理+可观测**：/audit-report、/graph、runs 事件与 transcript、
  /server/ops（A7 运维带）、/stats（A5 统计聚合）、/logs（A8 引擎事件流查询）、
  /events（SSE）。
- **交互中继**：/ui/pending、/ui/{id}/answer。
- **配置管理**：/config/manifest（api_key 脱敏往返）、/config/knobs（F14 生效值）、
  /config/agents（A3 属性面板）、/sops（A9 剧本清单+出场表）、/dependencies
  （A4 读面）、/integrations/settings（A10 开关读写）。
- **守门之外**：/auth/login、/auth/logout（A1——登录在守门外，否则鸡生蛋）。

## A1 守门中间件（handlers::auth_middleware，layer 罩 /api/v1 全部）

三通道任一通过即放行：① x-devforge-token（DR-9 服务间通道，常量时间比较）；
② session cookie（HttpOnly SameSite=Lax，滑动 24h——EventSource 自动携 cookie，
SSE 免特判）；③ 回环豁免（auth_loopback_exempt AtomicBool，缺省 true = 本机
免鉴权的既有体验；远程永不豁免，DR-9 仍强制非回环必配 token）。验证成功注入
AuthedUser extensions（写审计的 actor 来源）。ConnectInfo 经
into_make_service_with_connect_info 注入（中间件从 extensions 取真实对端，
oneshot 测试无此层视作本机）。

## 启动序（main.rs 协作）

CLI 旗标解析（parse_cli_flag 三件套 --home/--addr/--token）→ 引导锚找
config.json（CLI --home 根或 $HOME/.devforge，T205 裁决）→ knobs::init →
root 解析（CLI > config home > $HOME/.devforge）→ logging → observability
set_root → store 加载 → ServerState（token/豁免开关/logs_root 注入）→ serve
（优雅停机 + 调度循环监督）。

## 测试

http_tests.rs（启动 token 契约）；各端点测试分域见 tests/ 目录
（auth_login/server_ops/agents_config/stats/logs_query/sops/
dependencies_endpoint/integration_settings/project_mgmt 等）。
