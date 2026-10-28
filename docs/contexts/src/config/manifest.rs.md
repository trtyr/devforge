---
type: module
module_path: src/config/manifest.rs
unit: unit-src-25
unit_alias: unit-src-24
description: 配置清单真源——必装包/差异化包/MCP/13 份外置角色提示词/模型池，内置 15 个 Agent 的 builtin 清单
---

# src/config/manifest.rs

配置即代码的**真源**（P002）：必装四件套 + 差异化包 + MCP server + 13 份角色提示词常量 + 模型池/Provider 结构，以及把它们装配成 15 个 Agent 的 `builtin()`。渲染器只读这份清单产出 profile 目录（`<profiles_root>/<agent-name>/`）。

两层划分（文件头注释 `src/config/manifest.rs:3`-5）：**模板层写死**（`role_prompt`、必装四件套、`skills`、MCP/Key 内联于此 schema）；**模型层可变**（`model_pool` + Agent 的 `model` 引用）。

## 职责

**负责**：
- 包供给的事实源：`BASE_PACKAGES`（四件套）与四个差异化包常量（`CODING_AGENT_EXTRA` / `CODING_REVIEW_EXTRA` / `PLAN_AGENT_EXTRA` / `TEST_AGENT_EXTRA`）。
- MCP 供给的事实源：`McpServer` 结构 + `codegraph_mcp` / `context7_mcp` 两个构造函数。
- 13 份角色提示词常量（渲染成各 profile 的 AGENTS.md 正文；正文都不在本文件，见[提示词常量的共同口径](#提示词常量的共同口径)）。
- 清单 schema：`ModelEntry` / `ProviderDef` / `AgentManifest` / `ConfigManifest`。
- 内置清单装配：`builtin()`（15 Agent + 1 provider + 3 模型）；查表与解析：`get_agent` / `resolve_model` / `resolve_provider`；持久化：`save` / `load`。

**不负责**：不渲染目录（`src/config/render.rs`）；不装包（`src/config/installer.rs`）；不校验提示词与机械门常量是否一致——那是本文件 15 个测试函数的活，机械门常量本体在 `src/change/plan_audit.rs`；不做 HTTP 读写（`src/server/handlers.rs`）；不做清单迁移（无版本字段，见[数据模型](#数据模型configmanifest-落盘-schema)）。

## 公开成员

包与 MCP：

| 成员 | 类型 | 文档 |
|---|---|---|
| `BASE_PACKAGES` | 常量 | [BASE_PACKAGES](#base_packages) |
| `PLAN_AGENT_EXTRA` | 常量 | [PLAN_AGENT_EXTRA](#plan_agent_extra) |
| `CODING_AGENT_EXTRA` | 常量 | [CODING_AGENT_EXTRA](#coding_agent_extra) |
| `CODING_REVIEW_EXTRA` | 常量 | [CODING_REVIEW_EXTRA](#coding_review_extra) |
| `TEST_AGENT_EXTRA` | 常量 | [TEST_AGENT_EXTRA](#test_agent_extra) |
| `McpServer` | 结构体 | [McpServer](#mcpserver) |
| `codegraph_mcp` | 函数 | [codegraph_mcp](#codegraph_mcp) |
| `context7_mcp` | 函数 | [context7_mcp](#context7_mcp) |

提示词常量（13 个，全部 `pub const &'static str`；共同字段见[提示词常量的共同口径](#提示词常量的共同口径)）：

| 成员 | 送达 Agent（`builtin()` 内） | 文档 |
|---|---|---|
| `CODING_AGENT_PROMPT` | coding-agent | [CODING_AGENT_PROMPT](#coding_agent_prompt) |
| `CODING_REVIEW_PROMPT` | coding-review | [CODING_REVIEW_PROMPT](#coding_review_prompt) |
| `TEST_AGENT_PROMPT` | test-agent | [TEST_AGENT_PROMPT](#test_agent_prompt) |
| `TEST_REVIEW_PROMPT` | test-review | [TEST_REVIEW_PROMPT](#test_review_prompt) |
| `DOC_FIX_PROMPT` | doc-fix | [DOC_FIX_PROMPT](#doc_fix_prompt) |
| `DOC_AGENT_PROMPT` | doc-agent | [DOC_AGENT_PROMPT](#doc_agent_prompt) |
| `DOC_REVIEW_PROMPT` | doc-review | [DOC_REVIEW_PROMPT](#doc_review_prompt) |
| `CHANGE_PLAN_AGENT_PROMPT` | change-plan-agent | [CHANGE_PLAN_AGENT_PROMPT](#change_plan_agent_prompt) |
| `CHANGE_PLAN_REVIEW_PROMPT` | change-plan-review | [CHANGE_PLAN_REVIEW_PROMPT](#change_plan_review_prompt) |
| `CHANGE_CODING_AGENT_PROMPT` | change-coding-agent | [CHANGE_CODING_AGENT_PROMPT](#change_coding_agent_prompt) |
| `CHANGE_CODING_REVIEW_PROMPT` | change-coding-review | [CHANGE_CODING_REVIEW_PROMPT](#change_coding_review_prompt) |
| `CHANGE_TEST_AGENT_PROMPT` | change-test-agent | [CHANGE_TEST_AGENT_PROMPT](#change_test_agent_prompt) |
| `CHANGE_TEST_REVIEW_PROMPT` | change-test-review | [CHANGE_TEST_REVIEW_PROMPT](#change_test_review_prompt) |

类型与方法：

| 成员 | 类型 | 文档 |
|---|---|---|
| `ModelEntry` | 结构体 | [ModelEntry](#modelentry) |
| `ProviderDef` | 结构体 | [ProviderDef](#providerdef) |
| `AgentManifest` | 结构体 | [AgentManifest](#agentmanifest) |
| `ConfigManifest` | 结构体 | [ConfigManifest](#configmanifest) |
| `builtin` | 关联函数 | [builtin](#builtin) |
| `resolve_model` | 方法 | [resolve_model](#resolve_model) |
| `resolve_provider` | 方法 | [resolve_provider](#resolve_provider) |
| `get_agent` | 方法 | [get_agent](#get_agent) |
| `save` | 方法 | [save](#save) |
| `load` | 关联函数 | [load](#load) |

本文件无私有符号：所有非 `pub` 项都在 `#[cfg(test)] mod tests`（`src/config/manifest.rs:483`）里。

## 依赖

- 上游 / 消费方：
  - `src/config/mod.rs:7` `pub mod manifest;`；`:12` `pub use manifest::{AgentManifest, ConfigManifest, ModelEntry, ProviderDef};`——**只有四个类型被转发**；包常量与提示词常量不经 re-export。渲染侧只拿类型 `ConfigManifest`（`src/config/render.rs:9`），读的是字段（`base_packages` / `role_prompt`），`BASE_PACKAGES` 的跨模块引用只出现在 render 的测试里（`src/config/render_tests.rs:17`）。
  - `src/config/render.rs:30` `Renderer.manifest: &'a ConfigManifest`；`:39` `get_agent`；`:40` `resolve_model`；`:50` `role_prompt` → profile 的 AGENTS.md 正文；`:69` `resolve_provider`；`:72` `model_pool`（拼 models.json）；`:198` 读 `base_packages` 复制成包列表。
  - `src/server/mod.rs:27` 引入类型；`:125` `pub manifest: Mutex<ConfigManifest>`；`:127` `manifest_path: Mutex<Option<PathBuf>>`（清单落盘路径）。
  - `src/server/handlers.rs:611` `get_manifest`（GET 整份）、`:618`-`:620` `put_manifest`（PUT 整份，`:624`-625` 用 `resolve_model`/`resolve_provider` fail-fast）。
  - `src/main.rs:155`-162 启动装配：`<root>/manifest.json` 存在则用 `ConfigManifest` 的 `load`（`:158`），否则 `builtin()`（`:160`）后 `save`（`:161`）落盘。
  - 跨模块比对：`CHANGE_*` 提示词里的问询与协议锚点与 `src/change/plan_audit.rs` 的 `PLAN_BLOCKS`、`VERDICT_MUST_ASK`、`VERDICT_SELF`、`VERDICT_ANSWER_FIELD` 逐字比对（`src/config/manifest_tests.rs:8`、`:510`-512、`:528`-529）——常量语义见该模块条目，本文不重复。
- 向下依赖：`Path`（`src/config/manifest.rs:7`）、`serde` derive（`:9`）、crate 错误模块的 `Context` trait 与 `DevforgeError`（`:11`）、`serde_json`（`save`/`load` 与测试）、`BTreeMap`（`McpServer` 的 `env` 字段；声明处（`:50`）写标准库全路径，文件顶部没有对应 `use`）。
- 外部依赖：无进程、无网络、无端口。编译期依赖 `assets/prompts/` 下 13 个 Markdown（`include_str!`，文件缺失即编译失败）。测试依赖 `tempfile`（`Cargo.toml:27`）与仓内夹具 `tests/e2e/fixtures/change_agents.json`。

## 核心口径

- **必装四件套所有 Agent 都装**：`base_packages` 由 `BASE_PACKAGES` 逐个转 `String`（`src/config/manifest.rs:215`），渲染时作为起点再叠加 `extra_packages`（`src/config/render.rs:198`-199）。🟢 CONFIRMED `src/config/manifest.rs:14`、`:215`
- **审查方不与用户交互、干活方能问**：`ask-user-question` 包经 `PLAN_AGENT_EXTRA` 到 `plan-agent`（`:251`）与 `change-plan-agent`（`:369`），经 `CODING_AGENT_EXTRA` 到 `coding-agent`（`:271`）、`doc-fix`（`:323`）、`doc-agent`（`:338`）、`change-coding-agent`（`:390`）、`change-test-agent`（`:417`）；审查侧唯一非空供给是 `coding-review` 的 `CODING_REVIEW_EXTRA`（`npm:pi-lens`，`:287`），其余六个审查 Agent 的 `extra_packages` 为空（`:260`、`:313`、`:353`、`:379`、`:406`、`:426`）。🟢 CONFIRMED `src/config/manifest_tests.rs:61`-568（六个 change agent 钉死）、`:569`-593（其余九个钉死）
- **审查官不读码（DR-12）**：`doc-review` 的 `mcp` 为空向量（`:359`，注释 `:356`-358 记 10-23 用户拍板推翻 D012/D018）🟢 CONFIRMED `src/config/manifest_tests.rs:160`（用例 `doc-review 不得持有读码权限`）
- **模型固定分配**：审查系一律 `grok-4.7`；编码/规划系 `deepseek-flash`；`doc-agent` 用 `minimax-m3.1-flash`（`:339`）。池内三条模型参数相同（窗口 `1_000_000`、输出 `65_536`，`:226`-246）。**plan tree 的 P008 D018 仍写着 `deepseek-flash`**，仓内没有对应变更决策记录，落差登记在 `_meta/gaps.md`（unit-src-25 段）。🟢 CONFIRMED `src/config/manifest.rs:224`-246（模型池）、`:339`（doc-agent 的 model）、`:692`-695（断言消息「自 2026-10-20 起用 MiniMax-M3.1-Flash」）
- **提示词不是自由文本**：13 份提示词的关键锚点被单测逐字钉住（六块计划、问询标记、doc-lint 命令、`<stage-complete>`/`<stage-verdict>`/`<reroute>` 协议块…）；但**只有被锚点覆盖的片段**会撞红灯——散文约束（如 change-coding 的 `<impact-gap>` 字段格式）没有机械兜底，登记为 🟡（见 `_meta/gaps.md` unit-src-25 段）。🟢 CONFIRMED `src/config/manifest_tests.rs:6`-918（用例区）
- **用例名有历史残留**：`builtin_has_eleven_agents_and_four_base_packages`（`src/config/manifest_tests.rs:113`）名字还写着「eleven」，断言实际是 `agents.len() == 15`（`:597`-599）。按名找断言时别被名字误导。🟢 CONFIRMED `src/config/manifest_tests.rs:113`
- **skill 分发存在「断言比事实小」**：`doc-contract` 实际挂在 **5** 个 Agent 上（`coding-agent` `:274`、`coding-review` `:290`、`doc-fix` `:326`、`doc-agent` `:341`、`doc-review` `:355`），而钉死用例 `doc_contract_skill_is_shared_by_four_profiles`（`src/config/manifest_tests.rs:224`）只覆盖 **这 5 个持有者里除 `doc-agent` 之外的其余四个**（断言集：`coding-agent`、`coding-review`、`doc-fix`、`doc-review`）——`doc-agent` 在断言之外，摘掉它不会红。🟢 CONFIRMED `src/config/manifest_tests.rs:224`-716
- **清单没有 schema 版本字段**：`ConfigManifest` 五个字段里没有版本号（`src/config/manifest.rs:145`-156），`load` 只做 JSON 反序列化（`:476`-479）。字段增删后旧 `<root>/manifest.json` 能否续用完全交给 serde 规则——带 `#[serde(default)]` 的只有 `env`（`:49`）、`mcp`（`:134`）、`thinking_level`（`:139`）；**`api_key` 是例外**：它没有该属性，但类型是 `Option<String>`（`:155`），serde 对缺失的 `Option` 字段直接给 `None`（不报错）；其余四个字段（`base_packages` / `providers` / `model_pool` / `agents`）缺字段即 `load` 报错（实测文案 `解析配置清单: JSON: missing field base_packages`）。🟢 CONFIRMED `src/config/manifest.rs:155`（`api_key: Option<String>`）、`:476`-479（`load` 只做反序列化）；**升级兼容性本身是 🔴 GAP**（仓内无迁移代码可查，影响面见 `_meta/gaps.md` unit-src-25 段）。
- **夹具与内置同源**：`tests/e2e/fixtures/change_agents.json` 是 6 个 change agent 的序列化拷贝，`change_agents_fixture_matches_builtin`（`src/config/manifest_tests.rs:327`）保证不漂移；改了 change agent 配置要重跑 `dump_change_agents_fixture`（`:790`）。🟢 CONFIRMED `src/config/manifest_tests.rs:327`-827

---

### BASE_PACKAGES

- 名称 / 类型：`pub const BASE_PACKAGES: &[&str]`。（🟢 CONFIRMED `src/config/manifest.rs:14`）
- 默认值 / 取值范围：编译期常量，四项原文照录（`:15`-18）：

  ```text
  npm:@ff-labs/pi-fff
  npm:@juicesharp/rpiv-web-tools
  npm:pi-blackhole
  git:github.com/NVlabs/SoL-Pi
  ```
- 是否必填：常量无「必填」语义；`builtin()` 无条件用它初始化 `base_packages`（`:215`），因此对每个 profile 都是必装。
- 影响：所有 15 个 Agent 的 profile 都装这四个包（渲染时再叠加各自 `extra_packages`）；改一行等于改全部 profile 的依赖面。
- 示例：`assert_eq!(m.base_packages, BASE_PACKAGES.iter().map(|s| s.to_string()).collect::<Vec<_>>())`（用例 `src/config/manifest_tests.rs:115`）——逐元素相等，不是只比长度。

### PLAN_AGENT_EXTRA

- 名称 / 类型：`pub const PLAN_AGENT_EXTRA: &str`（单值）。（🟢 CONFIRMED `src/config/manifest.rs:22`）
- 默认值（单值）：

  ```text
  npm:@juicesharp/rpiv-ask-user-question
  ```
- 是否必填：常量无必填语义；挂到 `plan-agent`（`:251`）与 `change-plan-agent`（`:369`）。
- 影响：问询环的供给层——没有它这两个 Agent 手里就没有 ask 工具，计划阶段「必须问用户」的问不出去。
- 示例：`m.get_agent("plan-agent").unwrap().extra_packages == vec![PLAN_AGENT_EXTRA.to_string()]`；钉死用例 `src/config/manifest_tests.rs:61`（change-plan-agent 一行）、`:569`（plan-agent 一行）。

### CODING_AGENT_EXTRA

- 名称 / 类型：`pub const CODING_AGENT_EXTRA: &[&str]`。（🟢 CONFIRMED `src/config/manifest.rs:25`）
- 默认值：三项（`:26`-28）：

  ```text
  npm:@juicesharp/rpiv-ask-user-question
  npm:@juicesharp/rpiv-todo
  npm:pi-lens
  ```
- 是否必填：常量无必填语义；挂到 `coding-agent`（`:271`）、`doc-fix`（`:323`）、`doc-agent`（`:338`）、`change-coding-agent`（`:390`）、`change-test-agent`（`:417`）。
- 影响：coding 家族的「问询 + 待办 + 诊断」三件供给；改它等于同时改五个 Agent 的工具面。
- 示例：`coding-agent.extra_packages.len() == 3`（`src/config/manifest_tests.rs:131`）；`doc-fix` 与本常量同源（`:632` 断言 `df.extra_packages == ca.extra_packages`）。

### CODING_REVIEW_EXTRA

- 名称 / 类型：`pub const CODING_REVIEW_EXTRA: &[&str]`。（🟢 CONFIRMED `src/config/manifest.rs:32`）
- 默认值：一项——`npm:pi-lens`。
- 是否必填：常量无必填语义；只挂 `coding-review`（`:287`）。
- 影响：审查侧只需要诊断证据，不需要问询包与待办包；摘掉它 `coding-review` 就没有 lens 诊断能力。
- 示例：`coding-review.extra_packages == ["npm:pi-lens"]`（断言口径见 `src/config/manifest_tests.rs:135`-617）。

### TEST_AGENT_EXTRA

- 名称 / 类型：`pub const TEST_AGENT_EXTRA: &[&str]`。（🟢 CONFIRMED `src/config/manifest.rs:35`）
- 默认值：两项（`:36`-37）：

  ```text
  npm:@juicesharp/rpiv-todo
  npm:@juicesharp/rpiv-ask-user-question
  ```

  **不含 pi-lens**（黑盒纪律）。
- 是否必填：常量无必填语义；只挂 `test-agent`（`:304`）。
- 影响：测试 Agent 有待办包与偏差出口，但没有代码诊断工具；注意 `change-test-agent` 反而用 `CODING_AGENT_EXTRA`（`:417`），两者**不同源**。
- 示例：`test-agent.extra_packages == TEST_AGENT_EXTRA`（用例 `src/config/manifest_tests.rs:141`）。

### McpServer

- 职责：一个 stdio MCP server 的声明，渲染成 `<agent-dir>/mcp.json` 的 `mcpServers` 条目（渲染逻辑在 `src/config/render.rs`）。（🟢 CONFIRMED `src/config/manifest.rs:42`）
- 构造 / 初始化：字段直构，或用 `codegraph_mcp()` / `context7_mcp()`。
- 公开成员：`name`（工具名前缀 `mcp__<name>__<tool>`，`:44`）、`command`（`:46`）、`args`（`:47`）、`env: BTreeMap<String,String>`（`:50`，`#[serde(default, skip_serializing_if = "...BTreeMap::is_empty")]` `:49`——空则清单 JSON 里不出现 `env` 键）。
- 生命周期：值对象，随 `AgentManifest` 的 `mcp` 字段进清单 JSON，可经 `save`/`load` 往返；无内部可变状态。
- 典型用法：`AgentManifest { mcp: vec![codegraph_mcp(), context7_mcp()], .. }`（`src/config/manifest.rs:281`）；`mcp` 为空向量时不生成 mcp.json（字段注释 `:133`）。
- 派生 trait：`Debug, Clone, PartialEq, Serialize, Deserialize`（`src/config/manifest.rs:41`）。

### codegraph_mcp

- 摘要：CodeGraph 事实层 server 的标准构造。（🟢 CONFIRMED `src/config/manifest.rs:54`）
- 行为语义：返回 `name=codegraph`（`:56`）、`command=codegraph`、`args=[serve,--mcp]`、`env={CODEGRAPH_TELEMETRY: "0"}`（遥测关闭）；纯构造、每次新值。
- 参数：N/A（无参数）。
- 返回：`McpServer`。
- 异常：N/A（纯构造，不失败）。
- 示例：`coding-agent.mcp[0].name == "codegraph"` 且 `mcp[0].env.get("CODEGRAPH_TELEMETRY") == Some("0")`（断言口径 `src/config/manifest_tests.rs:128`-611）。

### context7_mcp

- 摘要：第三方库最新文档 server 的标准构造。（🟢 CONFIRMED `src/config/manifest.rs:64`）
- 行为语义：返回 `name=context7`（`:66`）、`command=npx`，`args` 两项（`:68`-69）：

  ```text
  -y
  @upstash/context7-mcp
  ```

  `env` 为空表（代码用默认值构造，`:70`）；纯构造、每次新值。
- 参数：N/A（无参数）。
- 返回：`McpServer`。
- 异常：N/A（纯构造，不失败）。
- 示例：`test-agent` 只有它（`ta.mcp.len() == 1`，`ta.mcp[0].name == "context7"`，断言口径 `src/config/manifest_tests.rs:142`-624）。

### 提示词常量的共同口径

13 份提示词字段相同，逐条不再重复：

- **名称 / 类型**：`pub const <NAME>: &str`，值来自 `include_str!("../../assets/prompts/<name>.md")`（`src/config/manifest.rs:74`-174）——**13 份全部外置**，本文件只剩一行常量声明（ARC-R4「提示词外置」已收官）。
- **默认值**：`builtin()` 把对应常量填进同名 Agent 的 `role_prompt`（`:250`-425）——其中 `plan-agent`（`:250`）与 `plan-review`（`:259`）用函数内局部字符串，其余 13 个 Agent 直接填常量。每个 Agent 的 AGENTS.md 正文就是资产文件全文。
- **是否必填**：常量本身无此语义；漏填 `role_prompt` 不会编译失败，只会让该 profile 的 AGENTS.md 为空——靠渲染测试发现（`src/config/render_tests.rs:32` 断言 AGENTS.md 等于 `role_prompt`）。
- **取值范围**：任意 Markdown 文本；但被单测锚点命中并钉死的片段不能随意改（见各常量小节「硬约束锚点」）。
- **影响**：改提示词 = 改对应 Agent 的行为边界，同时可能撞上 `src/config/manifest_tests.rs:6`-918 的锚点用例；改 change 系还会撞夹具校验（`:808`），需重跑 `dump_change_agents_fixture`（`:790`）。
- **示例**：填进 Agent 是 `role_prompt: CODING_AGENT_PROMPT.into()`（`:270`）；直接读正文用就取该 Agent 的 `role_prompt` 字段（渲染侧 `src/config/render.rs:50` 写进 AGENTS.md）。

### CODING_AGENT_PROMPT

- 用途：MVP coding-agent 的角色提示词（编码 Agent：勘察→自定阶段→落待办清单→开发→伴写文档→预审→`<stage-complete>N</stage-complete>`）。（🟢 CONFIRMED `src/config/manifest.rs:74`）
- 资产文件：`assets/prompts/coding-agent.md`。章节骨架：开工（顺序强制）/ 开发纪律 / 工具纪律 / plan 偏差协议 / 提交前预审 / 工程纪律（引用 `coding-discipline` skill）/ 阶段完成协议 / 工作方式。
- 硬约束锚点：`coding-discipline`、`行为断言`、`先证后报`、`只做内部重构`、`提交前预审`、`绝对词`、`断言必须能失败`、`预审证据`、`isVisible()`、`doc-contract`、`devforge doc-lint`、`退出码必须 0`（用例 `src/config/manifest_tests.rs:275`、`:853`）。
- 送达面：`coding-agent` 的 `role_prompt`（`:270`）。

### CODING_REVIEW_PROMPT

- 用途：MVP coding-review 的角色提示词（无状态审查：只管审查、只出报告）。（🟢 CONFIRMED `src/config/manifest.rs:77`）
- 资产文件：`assets/prompts/coding-review.md`。章节骨架：审查输入 / 审查面（逐项过）/ 工具纪律 / 输出协议。
- 硬约束锚点：`先证后报`、`doc-lint 复核`、`自己跑一遍`、`环境复用`、`测试只跑一遍`、`问题一次列全`、`批量调用`、`报告密度`、`不复述代码`（用例 `src/config/manifest_tests.rs:275`、`:853`、`:869`）。
- 送达面：`coding-review` 的 `role_prompt`（`:286`）。

### TEST_AGENT_PROMPT

- 用途：MVP test-agent 的角色提示词（黑盒测试工程师）。（🟢 CONFIRMED `src/config/manifest.rs:80`）
- 资产文件：`assets/prompts/test-agent.md`。章节骨架：开工 / 黑盒纪律 / 测试写作纪律 / 测试自身的可靠性 / 发现协议 / 追溯矩阵 / 阶段完成协议 / 工作方式。
- 硬约束锚点：`禁止读取任何实现文件内容`、`路径可以看，内容不能读`、`白名单`、`coverage-matrix`、`pytest-rerunfailures`、`Runbook`、`依赖获取`、`findings`、`严禁写单元测试`、`禁止在套件内运行整套测试`、`进程组`、`短超时`（用例 `src/config/manifest_tests.rs:388`）。
- 送达面：`test-agent` 的 `role_prompt`（`:303`）。

### TEST_REVIEW_PROMPT

- 用途：MVP test-review 的角色提示词（审测试与证据，不写测试）。（🟢 CONFIRMED `src/config/manifest.rs:83`）
- 资产文件：`assets/prompts/test-review.md`。章节骨架：审查输入 / 审查面 / 根因判定 / 审查纪律 / 输出协议。
- 硬约束锚点：`<reroute>`（`test-side` / `doc-side`）、`<stage-verdict>`、`<next>`、`业务关系`、`极端用户面`、`coverage-matrix`、`一票否决`、`发现挂死风险先停跑`、`批量调用`、`环境复用`、`问题一次列全`（用例 `src/config/manifest_tests.rs:388`）。
- 送达面：`test-review` 的 `role_prompt`（`:312`）。

### DOC_FIX_PROMPT

- 用途：doc-fix 的角色提示词（编码 Agent 的文档模式：只改文档、不改代码）。（🟢 CONFIRMED `src/config/manifest.rs:86`）
- 资产文件：`assets/prompts/doc-fix.md`。章节骨架：开工 / 文档纪律 / 工作方式。
- 硬约束锚点：`只改文档，不改代码`、`第三方`（可测标准）、`Runbook`、`findings`（用例 `src/config/manifest_tests.rs:388`）。
- 送达面：`doc-fix` 的 `role_prompt`（`:322`）。

### DOC_AGENT_PROMPT

- 用途：doc-agent 的角色提示词（doc-from-zero 干活方：勘察→解释→生成，发 `<doc-complete>unit-xxx</doc-complete>`）。（🟢 CONFIRMED `src/config/manifest.rs:89`）
- 资产文件：`assets/prompts/doc-agent.md`。章节骨架：开工（顺序强制）/ 纪律 / 阶段流水（事实 / 推断分离）/ 交审前自检 / 阶段完成协议 / 工作方式。
- 硬约束锚点：`只写 docs/contexts/`、`codegraph-first`、`事实与推断分离`、`🟢`、path:line、`_meta/gaps.md`、`N/A`、`<doc-complete>unit-xxx</doc-complete>`、`devforge doc-lint`、`按 route 回去修`、`_meta/facts/<unit>.md`、`汇编层只合成`、`Mermaid 源码`、`严禁写“为什么”`、`不得静默省略`（用例 `src/config/manifest_tests.rs:348`）。
- 送达面：`doc-agent` 的 `role_prompt`（`:337`）。

### DOC_REVIEW_PROMPT

- 用途：doc-review 的角色提示词（不读代码，只审文档质量与一致性）。（🟢 CONFIRMED `src/config/manifest.rs:92`）
- 资产文件：`assets/prompts/doc-review.md`。章节骨架：审查输入 / 审查面 / 输出协议（裁决前置防截断）。
- 硬约束锚点：`plan tree`、`Runbook`、`findings`、`<stage-verdict>`（用例 `src/config/manifest_tests.rs:388` 末段）。
- 送达面：`doc-review` 的 `role_prompt`（`:352`）；配套供给刻意最小：无 MCP（`:359`）、只带 `doc-contract` skill（`:355`）。

### CHANGE_PLAN_AGENT_PROMPT

- 用途：change-plan-agent 的角色提示词（在既有仓上把需求变成六块可机械校验的计划）。（🟢 CONFIRMED `src/config/manifest.rs:159`）
- 资产文件：`assets/prompts/change-plan-agent.md`。章节骨架：你会拿到 / 工具纪律（codegraph-first）/ 产出（change-plan.md）/ 歧义分类契约 / 明确不做 / 收尾。
- 硬约束锚点：`PLAN_BLOCKS`（`src/change/plan_audit.rs`）六块逐字出现、`codegraph-first`、`不写实施顺序`、`不写回归命令`、path:line；问询契约三常量 `VERDICT_MUST_ASK` / `VERDICT_SELF` / `VERDICT_ANSWER_FIELD` 与 `ask-user-question`（用例 `src/config/manifest_tests.rs:6`：块循环 `:489`、标记 `:510`-512，**字面量必须与 `src/change/plan_audit.rs` 同步**）。
- 送达面：`change-plan-agent` 的 `role_prompt`（`:366`）。

### CHANGE_PLAN_REVIEW_PROMPT

- 用途：change-plan-review 的角色提示词（只审计划：不看 diff、不跑命令、不写文件）。（🟢 CONFIRMED `src/config/manifest.rs:162`）
- 资产文件：`assets/prompts/change-plan-review.md`。章节骨架：你审什么（8 条判据）/ 你不审什么 / 收尾。
- 硬约束锚点：`不看 diff`、`不跑命令`、`不写文件`、`<stage-verdict>`、`留痕核对`、`分类恰当性`、`结构缺失`，以及 `VERDICT_MUST_ASK` / `VERDICT_ANSWER_FIELD` 字面量（用例 `src/config/manifest_tests.rs:6`，标记引用 `:528`-529）。
- 送达面：`change-plan-review` 的 `role_prompt`（`:378`）。

### CHANGE_CODING_AGENT_PROMPT

- 用途：change-coding-agent 的角色提示词（按已审计划改存量代码，不越界、不设计）。（🟢 CONFIRMED `src/config/manifest.rs:165`）
- 资产文件：`assets/prompts/change-coding-agent.md`。章节骨架：输入 / 开工前基线必须绿 / 执行纪律 / 提交前预审 / 计划不够时的路由 / 收尾。
- 硬约束锚点：`coding-discipline`、`行为断言`、`先证后报`、`只做内部重构`（用例 `src/config/manifest_tests.rs:275`）；`<impact-gap>` 一类字段格式**没有**机械锚点（见 🟡 缺口）。
- 送达面：`change-coding-agent` 的 `role_prompt`（`:389`）；供给 `CODING_AGENT_EXTRA`（`:390`）+ codegraph/context7（`:400`）。

### CHANGE_CODING_REVIEW_PROMPT

- 用途：change-coding-review 的角色提示词（对账式审查：diff ↔ 计划 ↔ impl-steps）。（🟢 CONFIRMED `src/config/manifest.rs:168`）
- 资产文件：`assets/prompts/change-coding-review.md`。章节骨架：审什么（对账式审查，7 条）/ 收尾。
- 硬约束锚点：`先证后报`（用例 `src/config/manifest_tests.rs:275`）。
- 送达面：`change-coding-review` 的 `role_prompt`（`:405`）；只带 codegraph（`:410`），不带 context7。

### CHANGE_TEST_AGENT_PROMPT

- 用途：change-test-agent 的角色提示词（黑盒证明「改对了」且「没弄坏别的」）。（🟢 CONFIRMED `src/config/manifest.rs:171`）
- 资产文件：`assets/prompts/change-test-agent.md`。章节骨架：你要证明两件事 / 顺序不能颠倒（回归优先→只对变更点写测试→验收对照表→至少一次反向验证）/ 纪律 / 收尾。
- 硬约束锚点：`回归优先`、`验收对照表`、`不改产品代码`、`对照表里只准有验收项`（断言在 `src/config/manifest_tests.rs:203`-687，位于 `builtin_has_eleven_agents_and_four_base_packages` 内）；回归日志与对账引擎见 `src/change/testing_gate.rs`。
- 送达面：`change-test-agent` 的 `role_prompt`（`:416`）；供给最薄：无 MCP（`:420`，黑盒纪律）。

### CHANGE_TEST_REVIEW_PROMPT

- 用途：change-test-review 的角色提示词（审测试与证据是否真证明变更）。（🟢 CONFIRMED `src/config/manifest.rs:174`）
- 资产文件：`assets/prompts/change-test-review.md`。章节骨架：审什么（5 条）/ 收尾。
- 硬约束锚点：`缺一条即 FAIL`（断言 `src/config/manifest_tests.rs:207`）。
- 送达面：`change-test-review` 的 `role_prompt`（`:425`）。

### ModelEntry

- 职责：模型池的一条——Agent 通过 `id` 引用它，渲染器据此写 pi 的 `defaultProvider`/`defaultModel` 与 models.json。（🟢 CONFIRMED `src/config/manifest.rs:96`）
- 构造 / 初始化：字段直构；`builtin()` 内置三条（`:224`-246：`grok-4.7` `:226`、`deepseek-flash` `:233`、`minimax-m3.1-flash` `:240`）。
- 公开成员：`id`（池内引用 id，`:98`）、`provider`（对应 `ProviderDef` 的 `name` 字段，`:100`）、`model_id`（上游真实模型名，`:102`）、`context_window: u64`（`:104`）、`max_tokens: u64`（`:106`）。
- 生命周期：随清单落盘（`save`/`load`）与 HTTP GET/PUT 往返；`resolve_model` 返回借用。
- 典型用法：`{ id: "grok-4.7", provider: "newapi", model_id: "mokeywork/grok-4.7", context_window: 1_000_000, max_tokens: 65_536 }`（`:226`-231）。

### ProviderDef

- 职责：provider 定义（渲染 models.json 时告诉 pi baseUrl 与协议族）。（🟢 CONFIRMED `src/config/manifest.rs:111`）
- 构造 / 初始化：字段直构；`builtin()` 内置一项 `newapi`（`:217`-221）。
- 公开成员：`name`（settings 的 `defaultProvider` / auth 键，`:113`）、`api`（协议族，`:115`）、`base_url`（`:117`）。
- 生命周期：随清单往返；无内部状态。
- 典型用法：`{ name: "newapi", api: "openai-completions", base_url: "https://newapi.trtyr.top/v1" }`（`:217`-221）。

### AgentManifest

- 职责：单个 Agent 的配置（模板层 + 模型引用 + 供给）。
- 构造 / 初始化：字段直构；`builtin()` 造 15 个（`:247`-432）。（🟢 CONFIRMED `src/config/manifest.rs:122`）
- 公开成员：`name`（= profile 目录名，`:124`）、`role_prompt`（AGENTS.md 正文，`:126`）、`extra_packages`（四件套之上的追加，`:128`）、`model`（`ModelEntry` 的 `id` 引用，`:130`）、`skills`（分发到 profile 的 skill 名，`:132`）、`mcp: Vec<McpServer>`（空 = 不生成 mcp.json，`:135`）、`thinking_level: Option<String>`（`defaultThinkingLevel` 覆盖；None = 全局 high，`:140`）。
- 生命周期：随清单往返；`get_agent` 返回借用（`&AgentManifest`）。
- 典型用法：见 `builtin()` 的 `coding-agent` 段（`src/config/manifest.rs:268`-283）。

### ConfigManifest

- 职责：整份配置清单（真源）——包的底座、模型管理面、Agent 管理面、明文 Key。（🟢 CONFIRMED `src/config/manifest.rs:145`）
- 构造 / 初始化：`builtin()`（内置默认）、`load`（从 JSON）或字段直构；无 `Default` 实现。
- 公开成员：`base_packages`（`:147`）、`providers`（`:149`）、`model_pool`（`:151`）、`agents`（`:153`）、`api_key: Option<String>`（明文不设防，`:155`）。
- 生命周期：进程内住在 `ServerState` 的 `manifest` 字段（`Mutex`，`src/server/mod.rs:125`）；落盘由 `save`/`load` 管；`PUT /config/manifest` 先同步校验引用（`src/server/handlers.rs:624`-625，失败 400），再整份替换内存并向 `manifest_path` 落盘（`:628`-631），随后后台线程重渲染（`:637`-641）并返回 `202 ACCEPTED`（`:643`-646）。`GET /config/manifest` 返回内存里的 `clone()`（`:611`-613）。
- 典型用法：`let m = ConfigManifest::builtin(); let a = m.get_agent("coding-agent")?; let model = m.resolve_model(a)?;`（三段分别见 `:179`、`:461`、`:438`）。

### builtin

- 摘要：内置默认清单——15 个 Agent + 1 个 provider + 3 个模型。（🟢 CONFIRMED `src/config/manifest.rs:179`）
- 行为语义：纯构造，无 IO；每次调用新建一份。`plan-agent` / `plan-review` 的提示词是函数内局部字符串（`plan_agent_prompt` `:180`-196、`review_prompt` `:197`-213），其余 13 个引用上方常量；`api_key` 为 `None`（`:433`）。
- 参数：N/A（关联函数，无 `self`）。
- 返回：`ConfigManifest`。
- 异常：N/A（不失败）。
- 示例：`ConfigManifest::builtin().agents.len() == 15`（`src/config/manifest_tests.rs:116`-599）；`get_agent("doc-agent").unwrap().model == "minimax-m3.1-flash"`（`:692`-695）。逐 Agent 的全量供给见下一节[供给矩阵](#供给矩阵15-个-agent)。

### 供给矩阵（15 个 Agent）

`builtin()` 里每个 Agent 的五个供给字段逐行事实（行号为该 Agent 块的 `name` 字段所在行）。第三方可据此写出 `builtin_has_eleven_agents_and_four_base_packages`（`src/config/manifest_tests.rs:113`）与两条钉死用例（`:542`、`:569`）的等价断言。

| # | Agent | 行 | `extra_packages` | `model` | `skills` | `mcp` | `thinking_level` |
|---|---|---|---|---|---|---|---|
| 1 | `plan-agent` | `:249` | `PLAN_AGENT_EXTRA`（1 项） | `deepseek-flash` | `plan-tree`（1） | 无 | `None`（全局 high） |
| 2 | `plan-review` | `:258` | 空 | `grok-4.7` | `plan-tree`（1） | 无 | `medium` |
| 3 | `coding-agent` | `:269` | `CODING_AGENT_EXTRA`（3 项） | `deepseek-flash` | `doc-contract`、`coding-discipline` + 4 个语言 skill（6） | `codegraph` + `context7` | `None`（全局 high） |
| 4 | `coding-review` | `:285` | `CODING_REVIEW_EXTRA`（1 项，`npm:pi-lens`） | `grok-4.7` | 同 `coding-agent`（6） | `codegraph` + `context7` | `medium` |
| 5 | `test-agent` | `:302` | `TEST_AGENT_EXTRA`（2 项） | `deepseek-flash` | `python-best-practices`（1） | `context7` | `None`（全局 high） |
| 6 | `test-review` | `:311` | 空 | `grok-4.7` | 空 | 无 | `medium` |
| 7 | `doc-fix` | `:321` | `CODING_AGENT_EXTRA`（3 项） | `deepseek-flash` | `doc-contract` + 4 个语言 skill（5，**无** `coding-discipline`） | `codegraph` + `context7` | `None`（全局 high） |
| 8 | `doc-agent` | `:336` | `CODING_AGENT_EXTRA`（3 项） | `minimax-m3.1-flash` | 同 `doc-fix`（5） | `codegraph` + `context7` | `None`（全局 high） |
| 9 | `doc-review` | `:351` | 空 | `grok-4.7` | `doc-contract`（1） | 无 | `medium` |
| 10 | `change-plan-agent` | `:365` | `PLAN_AGENT_EXTRA`（1 项） | `deepseek-flash` | `plan-tree`（1） | `codegraph` | `high` |
| 11 | `change-plan-review` | `:377` | 空 | `grok-4.7` | `plan-tree`（1） | `codegraph` | `medium` |
| 12 | `change-coding-agent` | `:388` | `CODING_AGENT_EXTRA`（3 项） | `deepseek-flash` | `coding-discipline` + 4 个语言 skill（5，**无** `doc-contract`） | `codegraph` + `context7` | `high` |
| 13 | `change-coding-review` | `:404` | 空 | `grok-4.7` | `coding-discipline`（1） | `codegraph` | `medium` |
| 14 | `change-test-agent` | `:415` | `CODING_AGENT_EXTRA`（3 项） | `deepseek-flash` | `python-best-practices`（1） | 无 | `high` |
| 15 | `change-test-review` | `:424` | 空 | `grok-4.7` | 空 | 无 | `medium` |

- 「4 个语言 skill」逐字为 `rust-best-practices`、`go-best-practices`、`python-best-practices`、`ts-js-best-practices`；五个 Agent 全拿（`coding-agent` `:276`-279、`coding-review` `:292`-295、`doc-fix` `:327`-330、`doc-agent` `:342`-345、`change-coding-agent` `:394`-397）。🟢 CONFIRMED `src/config/manifest.rs:276`-279
- `python-best-practices` 单独出现在 **7** 个 Agent 上：上面 5 个之外，还有 `test-agent`（`:306`）与 `change-test-agent`（`:419`——与黑盒纪律配套）。🟢 CONFIRMED `src/config/manifest.rs:306`、`:419`
- `thinking_level` 分布：`None`（= 全局 high）5 个、`medium` 7 个、`high` 3 个；`None` 只出现在干活方（`plan-agent` `:255`、`coding-agent` `:282`、`test-agent` `:308`、`doc-fix` `:333`、`doc-agent` `:348`）。🟢 CONFIRMED `src/config/manifest.rs:255`、`:282`、`:308`、`:333`、`:348`
- 空 `mcp` = 不生成 `<agent-dir>/mcp.json`（字段注释 `:133`）；空 `skills` = 不建任何 skill 目录（渲染侧只建 skills 骨架目录，`src/config/render.rs:43`）。

### resolve_model

- 摘要：按 Agent 的 `model` 引用在 `model_pool` 里找模型。（🟢 CONFIRMED `src/config/manifest.rs:438`）
- 行为语义：线性查找 `m.id == agent.model`；命中返回借用；无副作用、不改清单。
- 参数：`agent: &AgentManifest`。
- 返回：`Result<&ModelEntry>`。
- 异常：`DevforgeError` 的 `Config` 变体（引用不存在的模型），文案含「不在模型池中」以及 Agent 名与模型 id。
- 示例：`resolve_model(get_agent("plan-agent"))` → `model_id == "mokeywork/deepseek-flash"`（用例 `src/config/manifest_tests.rs:439`）；把 `model` 改成 `ghost-model` → `Err`，错误串含「不在模型池中」（`:932`）。

### resolve_provider

- 摘要：先解析 Agent 的模型，再按模型的 `provider` 字段找 provider 定义。（🟢 CONFIRMED `src/config/manifest.rs:451`）
- 行为语义：`resolve_model(agent)` 失败则原样上抛；随后线性查 `p.name == model.provider`；无副作用。
- 参数：`agent: &AgentManifest`。
- 返回：`Result<&ProviderDef>`。
- 异常：`Config`（模型不在池 / provider 无定义），后者文案为「模型 `{id}` 引用的 provider `{name}` 无定义」。
- 示例：`put_manifest` 的 fail-fast 校验用它（`src/server/handlers.rs:625`）；`render_agent` 用它取 base_url（`src/config/render.rs:69`）。

### get_agent

- 摘要：按名字取 Agent 配置。（🟢 CONFIRMED `src/config/manifest.rs:461`）
- 行为语义：线性查 `a.name == name`；未命中即错；无副作用。
- 参数：`name: &str`（即 profile 目录名）。
- 返回：`Result<&AgentManifest>`。
- 异常：`Config`（未知 Agent），文案 `清单中无 Agent `{name}``。
- 示例：`render_agent` 的第一步（`src/config/render.rs:39`）；单测大量使用（如 `src/config/manifest_tests.rs:121` 的 `m.get_agent("plan-agent")`）。

### save

- 摘要：把整份清单以 pretty JSON 写到指定路径。（🟢 CONFIRMED `src/config/manifest.rs:468`）
- 行为语义：`to_string_pretty` → 若有父目录则 `create_dir_all`（`:470`-472）→ 覆盖写目标文件（`:474`，无临时文件 + rename，**非原子**）。`skip_serializing_if` 的字段（空 `mcp`、`None` 的 `thinking_level`、空 `env`）不落盘；`api_key: None` 会写成 `null`（`Option` 没有 skip）。
- 参数：`path: &Path`（如 `<root>/manifest.json`）。
- 返回：`Result<()>`。
- 异常：`Config`（序列化 / 建目录 / 写文件失败，各带上下文标注，`:470`-474）。
- 示例：`builtin().save(&path)` 后 `load` 读回并 `assert_eq!`（用例 `src/config/manifest_tests.rs:459`）；`src/main.rs:161` 首次运行落盘。

### load

- 摘要：从 JSON 文件读回清单。（🟢 CONFIRMED `src/config/manifest.rs:476`）
- 行为语义：读全文 → `serde_json` 反序列化；不做 schema 版本校验、不做模型引用校验（后者在 `resolve_*` 与 PUT handler，`src/server/handlers.rs:624`-625）；缺字段的处理分两种——`mcp` / `thinking_level` / `env` 带 `#[serde(default)]` 可缺省，`api_key` 是 `Option<String>`（缺字段 → `None`），其余四个字段缺即报错。
- 参数：`path: &Path`。
- 返回：`Result<ConfigManifest>`。
- 异常：`Config`（读失败 / JSON 不合法，各带上下文）。
- 示例：`src/main.rs:157`-159 启动时若 `<root>/manifest.json` 存在则读它，取代 `builtin()`。

## 数据模型（ConfigManifest 落盘 schema）

落盘文件：`<root>/manifest.json`（`src/main.rs:156`），由 `save` 写、`load` 读；HTTP 面是 `GET`/`PUT /config/manifest`（`src/server/handlers.rs:611`、`:618`）。

| 字段 | 类型 | 约束 / 可空 | 默认 | 生命周期 / 关系 |
|---|---|---|---|---|
| `base_packages` | `Vec<String>` | 非空（`builtin()` 给 4 项）；无唯一约束 | 无（缺字段即 `load` 报错） | 渲染时作为每个 profile 的包起点（`src/config/render.rs:198`） |
| `providers` | `Vec<ProviderDef>` | 可为空；`ProviderDef` 的 `name` 被 `model_pool` 的 `provider` 引用 | 无 | `resolve_provider` 在这里查（`src/config/manifest.rs:451`） |
| `model_pool` | `Vec<ModelEntry>` | 可为空；`ModelEntry` 的 `id` 被 `agents` 的 `model` 引用 | 无 | `resolve_model` 在这里查（`:438`） |
| `agents` | `Vec<AgentManifest>` | 可为空；`name` 天然是 profile 目录名（未做唯一性校验） | 无 | `get_agent` 在这里查（`:461`）；`name` 重复时取第一个 |
| `api_key` | `Option<String>` | 可空；明文；**缺字段时反序列化为 `None`（不是报错）** | 无（`builtin()` 给 `None`；旧清单缺该键也读得进来） | 渲染时写进 profile 的 `<profile>/auth.json`（`src/config/render.rs:136`-141），本模块不加密 |

- 子结构约束：`AgentManifest` 的 `mcp`（默认空）与 `thinking_level`（默认 `None`）、`McpServer` 的 `env`（默认空）三项带 `#[serde(default)]`，缺字段可读；`ConfigManifest` 的 `api_key` 因类型是 `Option<String>`，缺字段也读成 `None`（实测见下方「测试运行与环境」的探针）；其余字段（`base_packages` / `providers` / `model_pool` / `agents`）缺即错。🟢 CONFIRMED `src/config/manifest.rs:49`、`:134`、`:139`、`:155`
- **迁移注意**：无 schema 版本字段、无迁移代码；升级后旧清单若缺新增的必填字段会 `load` 失败、进程起不来。🔴 GAP（影响面登记在 `_meta/gaps.md` unit-src-25 段）——文档只能提示「删旧清单让其重生成」，无法判断旧清单能否续用。

## 测试覆盖（`#[cfg(test)]`，15 个 `#[test]`，其中 1 个 `#[ignore]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `change_plan_prompts_carry_six_blocks_and_forbidden_items` | 六块 + 纪律锚点 + 问询标记与门常量逐字一致 | `src/config/manifest_tests.rs:6` |
| `change_six_agents_have_pinned_supplies` | 六个 change agent 差异化包钉死 | `src/config/manifest_tests.rs:61` |
| `other_agents_supplies_are_unchanged` | 其余九个 Agent 差异化包钉死 | `src/config/manifest_tests.rs:88` |
| `builtin_has_eleven_agents_and_four_base_packages` | 15 Agent / 四件套 / 各 agent 的 mcp·包·模型·skills | `src/config/manifest_tests.rs:113` |
| `doc_contract_skill_is_shared_by_four_profiles` | `doc-contract` 只断言 4 个 profile（实际 5 个持有，`doc-agent` 不在断言集） | `src/config/manifest_tests.rs:224` |
| `coding_discipline_skill_goes_to_exactly_the_four_coding_agents` | 4 正 11 负 | `src/config/manifest_tests.rs:237` |
| `coding_prompts_carry_discipline_anchors` | 两份 coding + 两份 review 的纪律锚点 | `src/config/manifest_tests.rs:275` |
| `dump_change_agents_fixture`（`#[ignore]`，代码生成器） | 重建 e2e 夹具（写仓内文件） | `src/config/manifest_tests.rs:309` |
| `change_agents_fixture_matches_builtin` | 夹具与 `builtin()` 逐字相等 | `src/config/manifest_tests.rs:327` |
| `doc_agent_prompt_carries_protocol_and_guards` | doc-agent 提示词 17 个锚点 | `src/config/manifest_tests.rs:348` |
| `coding_agent_prompt_has_preflight_audit` | 预审与 doc-lint 硬门禁锚点 | `src/config/manifest_tests.rs:372` |
| `testing_prompts_carry_protocol_and_discipline` | test / test-review / coding-review / doc 双 Agent 锚点 | `src/config/manifest_tests.rs:388` |
| `model_resolution_from_pool` | 池内解析（含固定分配） | `src/config/manifest_tests.rs:439` |
| `unknown_model_rejected` | 未知模型被拒 | `src/config/manifest_tests.rs:451` |
| `manifest_json_roundtrip` | `save`→`load` 全等 | `src/config/manifest_tests.rs:459` |

## 测试运行与环境

- 测试与被测代码同文件：`#[cfg(test)] mod tests`（`src/config/manifest.rs:483` 起）。
- **环境要求**：edition 2024（`Cargo.toml:4`）⇒ Rust 1.85+ 且 toolchain 已安装（无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`）。本轮实测 `rustc 1.99.0` / `cargo 1.99.0`。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧 `tempfile`（`Cargo.toml:27`）。测试还读仓内夹具 `tests/e2e/fixtures/change_agents.json`（缺失会 panic 并提示重跑生成器）。
- **前置条件：整仓可编译**（硬）——本模块用例是 lib 级用例，**任何一个兄弟模块的编译错误都会让整轮拿不到 `test result`**。实测反例（审查轮）：ARC-R5 在途重构期间（横切策略模块改名成 `src/engine/policy.rs` + `map_intervention` 方法平移为自由函数，改动未全部提交）整包 lib 有 13 处编译错误，`cargo test --lib config::manifest` 返回 EXIT=101、根本不进入测试；此时「红」与本文件无关。所以：
  1) 先验编译：`cargo test --lib --no-run`（或 `cargo check --lib`）必须通过；
  2) 工作区不干净 / 正在改其它模块时，改用干净检出跑——`git archive HEAD | tar -x -C <临时目录>`（不写回仓，比 `git worktree` 安全）。🟢 CONFIRMED `git archive HEAD` 导出后实测结果见下条
- **测试运行**（🟢 CONFIRMED，干净树实测 `cargo test --lib config::manifest`）：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib config::manifest
  ```

  预期输出（机器相关）：`test result: ok. 14 passed; 0 failed; 1 ignored; 0 measured; 583 filtered out`——`1 ignored` 就是 `dump_change_agents_fixture`；`filtered out` 随仓内测试总数变化。实测两路同值：①干净导出（`git archive HEAD`，基线 158b64f）→ `14 passed / 1 ignored / 583 filtered`，EXIT=0；②仓库工作区在岸改动可编译时 → 同值，EXIT=0（从零编译 36.95s，机器相关）。复跑记录：工作区 HEAD 184abb8（`src/config/manifest.rs` 自 19b3717 未变，948 行）→ `14 passed; 0 failed; 1 ignored; 583 filtered`，EXIT=0。
- **`api_key` 缺字段探针**（验证数据模型里「可空、缺字段不报错」这一条，在 `git archive HEAD` 的临时副本里跑一次即可，不写仓）：
  1) 写 `{"base_packages":[],"providers":[],"model_pool":[],"agents":[]}` → `ConfigManifest` 的 `load` 返回 `Ok`，且 `api_key` 为 `None`；
  2) 写缺 `base_packages` 的 JSON → `load` 返回 `Err`，文案 `解析配置清单: JSON: missing field base_packages`。🟢 CONFIRMED 两种输入均实跑命中（`cargo test --test <探针文件>`）
- **重建夹具**（改了 change 系提示词或供给后必须做，否则 `change_agents_fixture_matches_builtin` 红；这是唯一会写仓内文件的用例，故 `#[ignore]`）：

  ```bash
  cargo test --lib dump_change_agents_fixture -- --ignored
  ```
- **隔离方式**：绝大多数用例是纯内存断言（读 `builtin()`）；`manifest_json_roundtrip` 用 `tempdir`。无网络、无端口、无服务、无子进程。
- **配置项 / 启停与健康检查 / 种子数据**：配置项——本模块**定义**配置 schema（`ConfigManifest` 即配置本体），但没有运行时读取的环境变量或 CLI 开关（**N/A**，无外部可配键）；启停与健康检查——库内模块，无独立进程（**N/A**，服务级启停归 `src/server`）；种子数据——内置数据就是 `builtin()`（15 Agent / 3 模型 / 1 provider），无外部种子文件（**N/A**）。
- **公开接口清单**：本文件不暴露 HTTP/CLI 端点；对外可用的 Rust 面就是上文「公开成员」两张表（HTTP 面是 `GET`/`PUT /config/manifest`，归 `src/server/handlers.rs`）。
- **文档自检（本单元证据）**：`devforge doc-lint docs/contexts --repo <目标仓>` → 退出码 0（输出 `doc-lint: 通过（0 违规）`）；测试实跑同上（14 passed / 1 ignored，EXIT=0，干净树与工作区两路）。
