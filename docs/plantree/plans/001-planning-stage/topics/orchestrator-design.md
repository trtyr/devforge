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

## 保活语义

- Plan Agent 的「活着」= 进程/会话存活 + 完整上下文在内存中，不是 session 文件可重放
- 暂停 = 等输入，不销毁。编排器持有它的句柄（进程 / RPC 连接 / SDK 引用，机制见 Q001）
- 阶段完成判定（Q004）触发后，编排器向 Plan Agent 发终止消息，回收句柄

## 编排器职责（devforge 本体）

1. spawn 子 pi 进程，注入 `PI_CODING_AGENT_DIR=<profile 目录>`
2. 持有活 Agent 句柄，实现暂停/唤醒/终止
3. Review 结果的接收与回注（形态见 Q002）
4. 状态机推进（含 Q004 的「通过」判定）
5. 不做：任何硬限制、任何内容层面的判断（判断力全在两个 Agent 里）

## 已拍板设计哲学

无硬限制：循环不封顶、token 不设限、step 不设限（decisions/003）。信任模型收敛，限制是错误治理。
