<doc-verdict>PASS</doc-verdict>
<route>none</route>
<issues>无</issues>

# 文档审查报告 — unit-src-25（第 23 轮）

- 单元：`unit-src-25`（alias `unit-src-24`）→ `src/config/manifest.rs`；文档 `docs/contexts/src/config/manifest.rs.md`（478 行）
- 引擎审计（确定性，采信）：要求 32 条 / 已覆盖 32 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：不打开源码；事实以单元文档、`_meta/facts/unit-src-25.md`、`_meta/gaps.md`、plan tree、引擎审计与实跑输出为准。说明：`_meta/state.json` 记本稿为「第 24 轮复核（review-23 FAIL 的 5 条已落地）」，kickoff 记第 23 轮；按 kickoff 落盘 `review-23.md`，原 review-23（unit-src-24）已备份为 `review-23-unit-src-24.md`。

## 1. 覆盖（32/32 采信）

要求集合 = module ×1 + 公开符号 ×31：5 个包常量、2 个 MCP 构造函数、13 个提示词常量、5 个类型（`McpServer` / `ModelEntry` / `ProviderDef` / `AgentManifest` / `ConfigManifest`）、`builtin` 与 5 个方法。文档「公开成员」两张表逐行链到独立小节，31 个符号全部有落点；inventory 的 46 条（module ×1 + symbol ×31 + behavior ×14）中，14 条 behavior 由「测试覆盖」表承接。无悬空条目，与引擎 32/32 自洽。

## 2. findings 与 gaps 处置

工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → 待处置清单 **N/A**。`gaps.md` 本单元 5 条均有对应处置：

1. `doc_contract` 实际 5 个持有 / 断言 4 个：`核心口径` 与测试表均明写，并给 `:705` 用例与 5 个持有行。
2. 无 schema 版本字段：`数据模型` 以 🔴 GAP 披露，说明仅 `env`/`mcp`/`thinking_level` 带 `serde(default)`，并指回 gaps。
3. 13 份提示词与机械门字面量耦合：`核心口径` 标 🟡，逐份提示词列硬约束锚点，明确未锚定的散文约束无兜底。
4. `doc-agent` 模型与 P008 D018 落差：按代码记 `minimax-m3.1-flash`，同时明写 plan tree 仍写 `deepseek-flash` 并指回 gaps。
5. 行号随外置漂移：facts 以 HEAD `158b64f` / 948 行重核，文档行号与该基线一致。

无漏项。

## 3. 完整性 / 必填节 / N-A

- 模块三件齐备：职责（负责 / 不负责）、公开成员、依赖（上游消费方、向下依赖、外部依赖）。
- 8 个公开函数/方法（`codegraph_mcp`、`context7_mcp`、`builtin`、`resolve_model`、`resolve_provider`、`get_agent`、`save`、`load`）六件齐备：摘要、行为语义、参数、返回、异常、示例；纯构造函数的异常均写 `N/A` 并给理由。
- 5 个公开类型五件齐备：职责、构造/初始化、公开成员、生命周期、典型用法；`McpServer` 另列派生 trait。
- 5 个包常量按配置项七字段写（名称/类型、默认值、是否必填、影响、示例；常量无运行时可配范围，语义已说明）。
- 数据模型表含字段、类型、约束/可空、默认、生命周期/关系与迁移注意；唯一约束、重复 name 取第一个等口径明确。
- Runbook 七件套均有落点：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN`）、依赖（`cargo fetch`、`tempfile`、e2e 夹具）、配置项（N/A：只定义 schema，无运行时开关）、启停与健康检查（N/A：库内模块）、种子数据（N/A：`builtin()` 即内置种子）、测试运行（命令 + 预期输出 + 隔离）、公开接口清单（Rust 成员表；HTTP 归 server）。所有 N/A 均给理由。

## 4. 真实性与置信度

- 全部 🟢 断言均带 `path:line` 或可重放命令；关键行号与 facts 的 948 行基线一致，未出现沿用旧版 1159/1504 行行号的残留。
- 15 个 Agent 供给矩阵逐行与 facts 对齐；我用文档 API 另写独立探针重放（不读源码）：15 个 Agent 的 `extra_packages` / `model` / `skills` / `mcp` / `thinking_level` 全部匹配，`python-best-practices` 7 个 Agent、`thinking_level` 5/7/3 分布、`doc-contract` 5 持有均成立；探针 EXIT=0。
- `api_key` 语义已改正：`Option<String>` 缺字段 → `None`（不是报错）；`base_packages` 缺字段 → `Err`。探针实测：缺 `api_key` 返回 `Ok` + `None`；缺 `base_packages` 返回 `Err`，EXIT=0（按语义断言）。
- `context7_mcp` 的 args 已给字面量 `-y` / `@upstash/context7-mcp`；`codegraph_mcp` 的 `CODEGRAPH_TELEMETRY=0` 亦与 facts 一致。
- 测试计数正确：15 个 `#[test]`，其中 `dump_change_agents_fixture` 为 `#[ignore]`；实跑 14 passed / 1 ignored。
- 文档的「实测文案」两处是**消息内文**而非 `Display` 全串：`get_agent` / `resolve_model` / `resolve_provider` 的 `to_string()` 前置 `配置错误: `；serde 缺字段实际为 ``missing field `base_packages` at line 1 column 44``。这不改变行为语义，但第三方若做字面相等断言会差前缀/反引号，列为非阻塞建议（见 notes）。

## 5. 内部自洽与 plan tree

- 两层划分（模板层 / 模型层）、`base_packages` 起点 + `extra_packages` 叠加、`resolve_model` 先于 `resolve_provider`、`load` 不校验引用而 PUT handler fail-fast、夹具与 `builtin()` 同源且重建命令一致——前后无矛盾。
- plan tree：P002 T002（四件套、ask 差异化、DeepSeek 池）逐条对应。P008 D012/D018 关于 `doc-review` 挂 codegraph 的要求与 DR-12 冲突，文档明记 10-23 用户拍板；D018 的 `doc-agent` 模型落差已登记 gaps，属已披露事实而非文档伪造。未发现**未披露**的 plan tree 冲突。

## 6. 可跑（照 Runbook 从零 boot）

按文档命令执行，结果如下：

- 环境：`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`；`rustc 1.99.0` / `cargo 1.99.0`；`TMPDIR` 与 `CARGO_TARGET_DIR` 均使用仓外独立空目录。
- 依赖：`cargo fetch`（offline）→ EXIT=0。
- 前置编译：`cargo test --lib --no-run` → EXIT=0，从零编译 52.06s（文档 36.95s 已注机器相关）。
- 主命令：干净导出 `git archive HEAD`（HEAD `2ea3969`）到 `/var/tmp/r24clean`，执行
  `cargo test --lib config::manifest` → `14 passed; 0 failed; 1 ignored; 583 filtered out`，EXIT=0。
- 工作区复跑（HEAD `2ea3969`，`src/config/manifest.rs` 自 `19b3717` 未变、948 行）→ 同值 14/0/1/583，EXIT=0。
- `api_key` 探针（在干净导出副本中跑，不写仓）：缺 `api_key` → `Ok` + `None`；缺 `base_packages` → `Err`，语义与文档一致。
- 种子数据：`builtin()` 内置 15 Agent / 3 模型 / 1 provider；无外部种子文件；测试无网络、无端口、无服务、无子进程。
- 结论：Runbook 从零可跑，预期输出与实测逐项吻合。

## 7. 第三方可测标准

第三方可据文档写出：15 Agent 全量供给断言、`BASE_PACKAGES` 逐元素断言、`context7` args 断言、错误语义（未知 Agent / 未知模型 / 未知 provider）、`save`→`load` 往返、临时目录隔离与夹具重建。独立探针已证明供给矩阵与 `api_key` 语义可重放；仅错误文案的字面格式需按 `contains` 口径断言，或按 notes 补全后再做全串断言。

## 结论

覆盖 32/32 与 lint 0 采信；findings 无遗漏；必填节与 Runbook 七件套齐备；关键事实、行号、供给矩阵与测试计数均与 facts/实跑一致；Runbook 在干净树与当前工作区两路均跑通。判 **PASS**。非阻塞建议见下。

<notes>
- `load` 段与数据模型段的「实测文案」建议补全 serde 的反引号与 `at line … column …`，或明确写成「含 `missing field base_packages`」的 contains 口径；`get_agent` / `resolve_model` / `resolve_provider` 若引用 `to_string()` 全串，需补 `配置错误: ` 前缀。
- P008 D018 仍写 `doc-agent` = `deepseek-flash`，代码与文档为 `minimax-m3.1-flash`；文档已披露并登记 gaps，建议后续回写 plan tree 或决策记录，消除长期漂移。
- 测试证据中的「工作区 HEAD 184abb8」是历史复跑值；当前 HEAD `2ea3969` 同值，可在下次维护时刷新标注，避免读者误以为未复跑。
- `api_key` 为 `None` 时 `auth.json` 不写也不删的清理缺口属 `render.rs` 单元（unit-src-27），本文可加一行交叉引用，便于部署方查找。
</notes>
