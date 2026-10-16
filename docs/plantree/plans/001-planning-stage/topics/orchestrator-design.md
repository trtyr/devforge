# Topic · 编排器核心设计

P001 的心脏。状态机、Agent 生命周期、消息传递三大块的方案图。

## 角色定义

| 角色 | 底座 | 职责 | 生命周期 |
|------|------|------|----------|
| Plan Agent | pi + 独立配置（plan-tree skill） | 与用户沟通，产出/修订 plan tree 到 `~/.devforge/plans/<plan>/` | **干活者：阶段完成前不死**。暂停保活，可被 Review 结果唤醒 |
| Plan Review Agent | pi + 独立配置（plan-tree skill，只读视角） | 检查 plan tree 语义一致性：上下文通顺、前后无矛盾 | **检查者：用完即弃**。发现问题→扔给 Plan Agent→死；通过→直接死 |

职责边界：Review **只管语义通不通顺**。「这里有坑」「没沟通好」「设计不好」这类潜在问题不在职责内——那是用户的判断，不是 Review 的。

## 状态机

```
                 ┌──────────────────────────────────────────┐
                 ▼                                          │
[SPAWN plan-agent] → [PLAN_EDITING]（与用户对话，产出 plan tree）
                 │                                          │
                 ▼ 产出完成                                  │
          [PLAN_HOLD]（Plan Agent 暂停保活，上下文完整）        │
                 │                                          │
                 ▼                                          │
[SPAWN review-agent] → [REVIEWING] ──通过──→ [STAGE_DONE]     │
                 ▲                            （两 Agent 均停） │
                 │ 不通过                                       │
                 ▼                                            │
     [问题清单→Plan Agent]（Review Agent 死）                   │
                 │                                            │
                 ▼                                            │
     [PLAN_REVISING]（Plan Agent 唤醒续写，修完暂停）────────────┘
                 （修复完成 → 自动推进：SPAWN 全新 review-agent 从头查）
```

关键不变量：
1. **同一时刻最多一个 Review Agent 存在**——每轮 review 都是全新实例，不带上一轮记忆（用户明确要求：重新启动一个 PI Agent 从头查）
2. **Plan Agent 全程唯一且不死**——直到 [STAGE_DONE] 才与最后一任 Review Agent 一起停掉
3. **修完自动推进**——Plan Agent 修订完成停止后，编排器自动 spawn 下一轮 review，不需要人推

## 保活语义（机制已定：RPC 模式）

Q001 已解决：所有 Agent 进程 = `pi --mode rpc` 长驻子进程，编排器经 stdin/stdout JSONL 控制。

- Plan Agent 的「活着」= RPC 子进程存活，会话上下文在进程内存中。暂停 = `agent_settled` 事件后进程 idle 等下一条 prompt，不销毁
- 唤醒 = 对同一进程再发 `prompt`（同一会话续写，非 session 文件重放）——RPC 原生语义，无需自建
- 阶段完成判定（Q004）触发后，关 stdin 体面终止子进程
- 先例参考：pi-agents-team 的 worker 生命周期（idle 用 fresh prompt 唤醒、`agent_settled` 为完成判据）

## 进程身份与自我管理（红线）

devforge 必须能清楚区分「自己启动的 pi」和「用户自己的 pi」——绝不许扫全系统看到 pi 进程就杀，那是灾难不是管理（decisions/007）。

机制三层：
1. **spawn 时注入身份标记**：环境变量 `DEVFORGE_MANAGED=<run-id>`（每条流水线唯一）+ `DEVFORGE_ROLE=<plan-agent|plan-review>`。用户的 pi 没有这两个变量，一眼可辨
2. **运行台账（spawn ledger）**：每个子进程的 PID / role / 启动时间 / run-id 记入 `~/.devforge/` 状态文件；清理只按台账精确执行（PID + 进程组），不猜不扫
3. **崩溃对账**：编排器重启先对账——台账 PID 是否存活 + 存活进程环境变量是否真有自家标记，双验证才动

违反本节 = 架构级 bug，不是小瑕疵。

## 编排器职责（devforge 本体）

1. spawn `pi --mode rpc` 子进程，注入 `PI_CODING_AGENT_DIR=<profile 目录>` + 自家身份标记（见上节）
2. JSONL 协议驱动：prompt / 事件流监听 / `agent_settled` 同步（协议见 pi docs rpc.md、rpc-commands.md）
3. Plan 阶段用户交互中继：用户输入 → prompt → text_delta 流式回显（RPC 无 TUI，见 Q003）
4. Review 结果接收（`get_last_assistant_text` + verdict 块）与回注
5. 状态机推进（含 Q004「通过」判定）；spawn 台账维护与按台账清理
6. 不做：任何硬限制、任何内容层面的判断（判断力全在两个 Agent 里）

## DAG 流程模型与可观测性（原则 §4 在本阶段的落地）

流程对用户完全透明（用户拍板）：图模型建模 + 节点可下钻 + 流转可见。

- **数据模型**：编排器维护一张运行图——节点 = Agent 实例（带 role、run-id、状态：spawned/running/idle/dead），边 = 流转事件（spawn 触发、结果回注、阶段推进、review 回边）。review 循环是真实的循环边，图模型不假装无环
- **节点数据**：每个节点挂它的运行数据——RPC 事件流（text/think/tool 调用）、结构化日志、verdict、起止时间。原料就是 RPC 事件，编排器边监听边落盘（按 run-id/节点组织，落 `~/.devforge/logs/`）
- **MVP 边界**：本阶段先做数据层（图状态 + 事件落盘，结构化可查询），流程图可视化渲染后续迭代——但数据模型从第一天按图设计，不事后重构

## 已拍板设计哲学

无硬限制：循环不封顶、token 不设限、step 不设限（decisions/003）。信任模型收敛，限制是错误治理。
