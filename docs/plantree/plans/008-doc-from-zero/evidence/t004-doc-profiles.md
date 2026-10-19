# T004/T005 · doc-agent 新建 + doc-review 供给改造 证据

- 日期：2026-10-19
- 依据：D009（两类 Agent）、D012（审查供给）、D018（profile 定稿表）

## 一、新 profile：`doc-agent`（干活方）

| 项 | 值 |
|---|---|
| model | `deepseek-flash`（与 coding-agent 同档：单元多、速度优先，质量由门禁兜） |
| thinking | 默认（全局 high） |
| MCP | `codegraph` + `context7`（与 coding-agent 完全相同） |
| skills | `doc-contract` + 四语言 best-practices |
| 包 | 与 coding-agent 相同（`CODING_AGENT_EXTRA`） |

角色提示词 `DOC_AGENT_PROMPT` 落定这些硬约束：
- **只写 `docs/contexts/`**，目标仓其余文件只读
- codegraph-first（MCP 显式 `projectPath`，不 grep+read 爬取）
- **事实/推断分离**：勘察阶段禁写"为什么"；解释阶段每条带 🟢/🟡/🔴，🟢 必须给 `path:line` 或可重放命令
- 按 `doc-contract` 写；不适用写 `N/A`；缺信息进 `_meta/gaps.md`，不编
- 单次上下文 ≈4K 片段，不够用图查询补
- 交审前自检五条 + `devforge doc-lint --repo` 退出码 0
- 协议：`<doc-complete>unit-xxx</doc-complete>`；打回按 `<route>` 回对应阶段

## 二、改造：`doc-review`（审查方）

| 项 | 改前 | 改后 |
|---|---|---|
| MCP | 无 | **`codegraph`**（查实体存在性必须有图）；**不给** context7 |
| thinking | medium | medium（不变） |
| skills | `doc-contract` | `doc-contract`（不变） |
| model | `grok-4.7` | 不变 |

## 三、单测

- `builtin_has_nine_agents_and_four_base_packages`：9 个 profile；doc-review 的 mcp 恰为 1 个 codegraph；doc-agent 的 model/mcp/包与 coding-agent 一致、thinking 为 None
- `doc_agent_prompt_carries_protocol_and_guards`：10 条锚点（只写 contexts / codegraph-first / 事实推断分离 / 🟢 / path:line / gaps.md / N/A / `<doc-complete>` / doc-lint / 按 route 回去修）
- `render_all_covers_every_agent`：渲染 9 个 profile

## 四、门禁

`cargo test` **221 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
