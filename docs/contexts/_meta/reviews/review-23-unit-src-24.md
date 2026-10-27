# 文档审查报告 — unit-src-24（第 23 轮）

- 单元：`unit-src-24`（文档 frontmatter 记 `unit-src-25` / alias `unit-src-24`）→ `src/config/manifest.rs`；文档 `docs/contexts/src/config/manifest.rs.md`（438 行）
- 引擎审计（确定性，采信）：要求 5 条 / 已覆盖 5 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：不打开源码；以单元文档、`_meta/facts/unit-src-25.md`、`_meta/gaps.md`、plan tree、引擎审计与实跑输出为准

## 1. 覆盖与 findings 处置

- 覆盖：引擎 5/5 采信。文档「公开成员」两张表覆盖 31 个符号，与 glossary / inventory 的 46 条（module×1 + symbol×31 + behavior×14）对齐，无悬空锚点。
- 工作区无 `testing/findings/`，kickoff 未附本单元 findings → 待处置清单 N/A。
- `gaps.md` 本单元 5 条均有对应处置：①`doc-contract` 实际 5 持有 / 断言 4（`:96` 明写）；②无 schema 版本（`:97` 标 🔴 并指回 gaps）；③提示词锚点只覆盖部分散文约束（`:94` 标 🟡）；④`doc-agent` 模型与 P008 D018 落差（`:93` 明写）；⑤行号随提示词外置漂移（facts 按 948 行重核，文档行号已对齐）。无漏项。

## 2. plan tree 与完整性

- P002 T002（ConfigManifest 结构、四件套、ask 差异化、DeepSeek 池）逐条对应。P008 D012/D018 要求 `doc-review` 挂 codegraph，与 DR-12 冲突；文档 `:92` 记「10-23 用户拍板推翻 D012/D018」，属显式决策而非冲突。D018 的 `doc-agent` 模型落差也已在 `:93` 与 gaps 披露。
- 模块三件（职责 / 公开成员 / 依赖）齐备；公开函数六件、公开类五件、配置常量七件基本齐备；数据模型表含字段、类型、可空、默认、生命周期与迁移注意。
- Runbook 七件套均有落点：环境（Rust 1.85+ / `RUSTUP_TOOLCHAIN`）、依赖（`cargo fetch`、`tempfile`、夹具）、配置项（N/A：只定义 schema、无运行时开关）、启停与健康检查（N/A：库内模块）、种子数据（N/A：`builtin()` 即种子）、测试运行与隔离、公开接口（Rust 成员表 + HTTP 归 server）。N/A 均给了理由。
- **缺**：`builtin()` 的 15 Agent 全量供给表。文档只零散给出 extra/model 与部分 skills/mcp/thinking；`python-best-practices`、各 Agent 的 4 个语言 skill、逐 Agent 的 `thinking_level`（medium/high）没有成表。facts 有 15 行表，文档未承接；第三方无法据文档写出 `builtin_has_eleven_agents_and_four_base_packages` 的等价断言。

## 3. 内部自洽与真实性

- **假命题（主因）**：`:97`、`:375`、`:393` 三处称「带 default 的只有 env/mcp/thinking_level，其余缺字段直接报错」。`api_key` 是 `Option<String>` 且无 `#[serde(default)]`，serde 对缺失的 Option 字段返回 `None`，不报错。实跑 clean HEAD 的 `ConfigManifest::load`（缺 `api_key` 的四字段 JSON）返回 `Ok(None)`。这与数据模型「`api_key` 可空」也冲突；第三方若按文档写「缺 api_key 必报错」的测试会直接失败。
- `:96`「只断言前 4 个里除 doc-agent 之外的四个集合」自相矛盾：4 个里去掉 1 个只剩 3 个；应为「5 个持有里除 doc-agent 之外的四个」。
- `:187` 只写 `context7_mcp` 的 args 是「`-y` 与 context7 的 npm 包名」，未给字面量 `@upstash/context7-mcp`；第三方无法逐字断言 args。
- `:93` 的断言行号写 `:692`-695，gaps 记 `:693`-695（差一行，非实质）。
- 其余：15 个 `#[test]` 与「14 passed + 1 ignored」一致；`save` 的「有父目录才 `create_dir_all`」与 facts 一致；`doc-review` 空 MCP 与 DR-12 一致；`doc-contract` 5/4、`builtin_has_eleven…` 名字残留均已诚实披露。🟢 大多可回指 path:line。

## 4. 可跑（照 Runbook 从零）

- 环境：`TMPDIR=/var/tmp/r23t/tmp`、`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、全新空 `CARGO_TARGET_DIR`；`rustc 1.99.0` / `cargo 1.99.0`。
- 主工作区 `cargo fetch`（offline）EXIT=0；直接跑 `cargo test --lib config::manifest` 编译失败：13 个错误（`retry` 不存在、`map_intervention` 不存在、`policy.rs` 泛型参数不匹配），EXIT=101，全部在 `src/engine/*` 与 `src/server/*`，不在 `config::manifest`。原因是工作区正被 ARC-R5 在途重构（`retry.rs→policy.rs`）改写，非本单元文档可修。
- 在 HEAD `566dd63` 的干净 worktree（源码与 facts 基线 `470b283` 无差异）复跑同一命令：`14 passed; 0 failed; 1 ignored; 0 measured; 583 filtered out`，EXIT=0，从零编译 33.03s，与文档预期逐字吻合（36.95s 为机器相关值）。
- 结论：Runbook 命令在干净 checkout 上可跑；但文档未写「须在干净 checkout / 全仓可编译时执行」这一前置条件，当前工作区证据不可复现，故按「跑不起来 = FAIL」记一条。

## 5. 第三方可测标准

错误语义、`save`→`load` 往返、夹具重建、测试隔离、模型/extra 分配足以覆盖大部分接口；但 15 Agent 的 skills/mcp/thinking 全量值缺失、`context7_mcp` args 字面量缺失、`api_key` 缺字段语义写反，第三方无法完全据此写出等价测试。

## 结论

覆盖 5/5 与 lint 0 采信，findings 无遗漏，plan tree 无未披露冲突；但文档存在 **15 Agent 全量供给表缺失**、**`api_key` 缺字段语义错误（三处）**、**Runbook 未声明干净 checkout 前置条件（当前工作区实跑 FAIL）**，另有 context7 args 字面量与「前 4 个里」措辞两处小问题。按完整性 / 真实性 / 可跑面判 **FAIL**，修复点集中，回注 doc-fix。
