# DevForge API 对接契约（M5 · T5.4）

> 前端对接面稳定化（2026-10-24 用户拍板：后端纯 API，前端直接消费）。
> 本文档是**前端消费契约**：核心端点、SSE 事件 schema、错误码规范。
> 变更本文件的规则：新增字段 = 兼容（可加）；删改字段 = 破坏（须在 roadmap 记录版本）。

## 核心端点

| 方法 | 路径 | 语义 |
|---|---|---|
| GET | `/api/v1/config/manifest` / PUT | 配置清单读 / 整份替换（202 = 渲染异步受理，SSE `config.rendered` 通知） |
| POST/GET | `/api/v1/projects` | 项目登记 / 列表 |
| POST/GET | `/api/v1/projects/{name}/tasks` | 建任务（body 含 `contract`/`depends_on`/`exclusive`）/ 列表 |
| POST | `/api/v1/projects/{name}/tasks/{task}/start` | 发起 run（202；`fresh: true` 清断点重跑） |
| POST | `.../cancel` | 取消（置位 + watchdog 精确 SIGKILL） |
| GET | `.../runs`、`.../runs/{run}/events` | run 列表 / 事件流（JSONL 分页 `limit/offset/event`） |
| GET | `.../runs/{run}/transcript` | 会话原文 |
| GET | `/api/v1/events` | **SSE 全局事件流**（Last-Event-ID 回放） |
| GET | `/api/v1/scheduler/status` | **调度态快照**：`running[]`（project/task/run_id/elapsed_ms）、`runnable[]`、`blocked[]`（task+reason）、`queue[]`（project/total/pending/integrated/needs_rebase/needs_repair）、`max_concurrent_runs` |
| POST/GET | `/projects/{name}/integration/queue` | 集成候选入队 / 队列查询 |

## SSE 事件 schema（`/events`；`{seq, type, ...}` 信封）

**业务事件（前端消费主体）**：

| type | 关键字段 | 语义 |
|---|---|---|
| `project.created` / `task.created` | project/task | 登记与建任务 |
| `run.started` / `run.finished` | project/task/run_id/ok | run 生命周期（finished.ok = 成功布尔） |
| `run.retry` / `run.attempt_failed` / `run.attempt_exhausted` / `run.error` | attempts/reason | 基础设施重试链（DR-8） |
| `phase.change` | phase | SOP 阶段推进（spec/implement/verify/integrate 或 plan/coding/testing/done） |
| `boundary.ok` / `boundary.violation` | files / violations+forbidden | 契约终校验通过 / 越界或禁改命中 |
| `scheduler.budget_fused` | project/task/budget_minutes | 超预算熔断 |
| `integration.enqueued` / `integration.auto_enqueued` | project/task | 集成候选入队（手动/自动） |
| `workspace.orphans_cleaned` | — | 注册时孤儿 worktree 清理 |
| `config.rendered` / `config.render_failed` | — | manifest 渲染完成/失败 |
| `ui.request` / `ui.response` | id/question | 需求裁决中继（POST `/ui/{id}/answer` 回填） |
| `doc.unit_start` / `doc.unit_done` / `doc.first_unit_overdue` | unit | 文档流水线单元进度 |
| `gate.baseline` / `gate.self_check` / `gate.step_commits` | — | change 确定性硬门结果 |
| `review.open` | — | 审查实例开启 |

**进程遥测（观测面，前端可选展示）**：`agent.step`（tool/phase/ms）、`agent.turn_end`（turn_ms/chars/tokens）、`agent.stall`（失活 WARN）、`fix.start` / `fix.end`、`profile.render_without_api_key`、`api_key`。

**内部协议（不上前端）**：`prompt`/`response`/`hello`/`session`/`get_state`/`message`/`message_update`/`text`/`text_delta`/`agent_start`/`agent_end`/`agent_settled`/`tool_execution_start`/`tool_execution_end`/`extension_ui_request`/`extension_ui_response`——pi RPC 上行转发，前端忽略。

## 错误码规范

HTTP 标准状态码 + JSON `{"message": "..."}`（无自定义错误码域——状态码即契约）：

| 状态 | 语义 | 触发例 |
|---|---|---|
| 400 | 请求不合法 | 未知 SOP / 模型引用不可解析 / JSON 坏 |
| 404 | 资源不存在 | 项目/任务/run 未登记 |
| 409 | 状态冲突 | 并发上限已满（P012 M4 起 = 可配置 N，非固定 1）/ 任务 Running 中禁改 SOP |
| 500 | 系统错误 | 存储读写失败等（`DevforgeError` 未归类） |

## T5.2 失活收割指纹复审（结论）

idle 判据（`agent.stall` WARN → 收割）基于 **per-run session 指纹**（observability 的
心跳按 run-id 落点，`obs_root`/`<sessions>` 隔离）。多 worktree 复审：每个 run 的
工作目录彼此隔离（`{repo}-worktrees/{task}`），会话原文与心跳同 run-id 绑定，**判定
不依赖 cwd 全局态 → 多 worktree 并行下指纹口径成立，无需改动**。遗留观察项：单测
`the_run_writes_a_heartbeat...` 的负载敏感 flake 与本口径无关（已登记 arc-r3 计划收尾节）。
