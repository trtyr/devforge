# devforge

AI 自动化开发流水线的编排器（MVP）：以 [pi](https://pi.dev) 为执行底座，
驱动「规划 → 审查」SOP 的端到端流水线——创建项目、派发任务、真 pi 子进程做规划、
Review Agent 独立审查、verdict 通过后自动落账。

## 形态

devforge 是**服务器**（非 CLI）：axum 常驻，REST + SSE 五能力域——

| 能力域 | 入口 |
|--------|------|
| 编排控制 | `POST/GET /api/v1/projects`，任务 CRUD、`start`、`cancel` |
| 产物管理 | `GET /projects/{p}/tasks/{t}/runs`、`/graph` |
| 交互中继 | Agent 的 ask-user-question 经 SSE `ui.request` 推送，`POST /ui/{id}/answer` 回填 |
| 可观测 | `GET /api/v1/events`（SSE，全事件序号化，支持 `Last-Event-ID` 断线续传） |
| 配置管理 | `GET/PUT /api/v1/config/manifest`（整份替换，异步渲染 profile，202 + SSE 通知） |

## 架构一览

```
src/
├── config/    # 清单模型 + profile 渲染器（auth/models/settings/skills/提示词，pi 安装器）
├── domain/    # Store（内存台账）、Task/Run、RunControl（取消/超时干预面）
├── engine/    # RPC 编排引擎：JSONL 协议、spawn 命令（三重身份标记）、进程台账（孤儿清场）
├── sop/       # 规划 SOP：PlanningLoop 状态机（规划→审查循环→verdict 解析）、RPC 桥接
├── server/    # axum API 服务器：五能力域 + 事件总线（EventBus：序号/留档/广播）
└── error.rs   # thiserror 双层错误（Agent 失败 vs 系统错误）
```

编排模型（decisions/003/004/007/008）：

- **零硬限制**：无内建轮次/时长上限；run 超时默认不限时，`DEVFORGE_RUN_TIMEOUT_MS` 显式开启
- **零校验自动流转**：Review 打回后问题连同失败明细回注（plan→`fix_plan`、coding→`fix_coding`、
testing→回注 coding），有界（各 3 轮后判质量失败），不验修复质量
- **精确清场**：取消/超时按 `DEVFORGE_MANAGED=run-id` 精确 SIGKILL 自家 pi，不碰用户进程
- **会话留档**：每个 Agent 会话 JSONL 按 run 归档于 `~/.devforge/sessions/<run-id>/<角色>/`；
  plan tree 产物落 `~/.devforge/plans/<项目>/<任务>/`

## 运行

```bash
cargo run            # 启动 API（默认 127.0.0.1:7888，DEVFORGE_ADDR 可改）
```

可选环境变量：`DEVFORGE_TOKEN`（启用 `x-devforge-token` 鉴权）、`DEVFORGE_RUN_TIMEOUT_MS`（run 超时）。

## 测试

```bash
cargo test                          # 单元测试 + doctest
python3 tests/e2e/run_all.py        # 端到端全量（真 pi + 真模型，--quick 跳过长链路）
```

端到端用隔离 HOME 起真服务器 + 真 pi 子进程 + 真模型（规划 Agent deepseek-flash /
审查 Agent grok-4.7，经自建中转）；AI 自动应答全部交互（`dialog_responder.py` 探测制回填）。
现状与历史结论见 `tests/e2e/TEST_REPORT.md`，项目过程见 `docs/plantree/`。
