---
type: module
unit: unit-src-01
module: src/config/manifest.rs
---

# src/config/manifest.rs

配置清单 schema：系统「配置即代码」的真源——模板层写死，模型层可变；一份清单同时充当 9 个 Agent 的提示词/包/skills/MCP 来源与模型池定义。

## 职责

**负责**：

- 定义清单 schema（`ConfigManifest` 及其四个子结构 `AgentManifest`、`ModelEntry`、`ProviderDef`、`McpServer`）与它们的 JSON 形态。
- 提供内置默认清单 `builtin`：9 个 Agent、3 个模型池条目、1 个 provider、4 个必装包（`src/config/manifest.rs:466`）。
- 提供两个内置 MCP server 工厂：`codegraph_mcp`（`src/config/manifest.rs:54`）、`context7_mcp`（`src/config/manifest.rs:64`）。
- 提供清单内引用解析与查表：`resolve_model`（`src/config/manifest.rs:651`）、`resolve_provider`（`src/config/manifest.rs:664`）、`get_agent`（`src/config/manifest.rs:674`）。
- 提供清单落盘与读盘：`save`（`src/config/manifest.rs:681`）、`load`（`src/config/manifest.rs:689`）。
- 集中定义 5 组包名常量与 7 段 Agent 提示词常量（全仓唯一提示词定义处）。

**不负责**：

- 把清单渲染成 profile 目录（写 `<profile>/AGENTS.md`、`<agent-dir>/mcp.json`、`<profile>/models.json`、复制 skills、装包）——渲染层 `src/config/render.rs` 的事。
- 决定装包机制（`pi install` 怎么调）——装包层 `src/config/installer.rs` 的事。
- 决定清单落盘路径与进程内状态归属：路径 `<home>/.devforge/manifest.json` 由应用层 `src/main.rs:151` 决定，HTTP 入口在 `src/server/handlers.rs:340`。
- 内容合规性校验：本模块只校验「引用可解析」（模型 id、provider 名存在），不校验包名是否可装、提示词是否越权。

## 公开成员

| 符号 | 种类 | 行 | 一句话 | 详见 |
|---|---|---|---|---|
| `BASE_PACKAGES` | 常量 | `src/config/manifest.rs:14` | 必装四件套（所有 Agent） | [`BASE_PACKAGES`](#base_packages) |
| `PLAN_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:22` | 差异化包：仅 Plan Agent | [`PLAN_AGENT_EXTRA`](#plan_agent_extra) |
| `CODING_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:25` | 差异化包：Coding Agent | [`CODING_AGENT_EXTRA`](#coding_agent_extra) |
| `CODING_REVIEW_EXTRA` | 常量 | `src/config/manifest.rs:32` | 差异化包：Coding Review | [`CODING_REVIEW_EXTRA`](#coding_review_extra) |
| `TEST_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:35` | 差异化包：Test Agent | [`TEST_AGENT_EXTRA`](#test_agent_extra) |
| `McpServer` | 结构体 | `src/config/manifest.rs:42` | MCP server 定义（stdio） | [`McpServer`](#mcpserver) |
| `codegraph_mcp` | 公开函数 | `src/config/manifest.rs:54` | 造一个 codegraph MCP server | [`codegraph_mcp`](#codegraph_mcp) |
| `context7_mcp` | 公开函数 | `src/config/manifest.rs:64` | 造一个 context7 MCP server | [`context7_mcp`](#context7_mcp) |
| `CODING_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:74` | Coding Agent 提示词 | [`CODING_AGENT_PROMPT`](#coding_agent_prompt) |
| `CODING_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:135` | Coding Review 提示词 | [`CODING_REVIEW_PROMPT`](#coding_review_prompt) |
| `TEST_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:178` | Test Agent 提示词 | [`TEST_AGENT_PROMPT`](#test_agent_prompt) |
| `TEST_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:241` | Test Review 提示词 | [`TEST_REVIEW_PROMPT`](#test_review_prompt) |
| `DOC_FIX_PROMPT` | 常量 | `src/config/manifest.rs:288` | Doc Fix 提示词 | [`DOC_FIX_PROMPT`](#doc_fix_prompt) |
| `DOC_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:312` | Doc Agent 提示词 | [`DOC_AGENT_PROMPT`](#doc_agent_prompt) |
| `DOC_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:371` | Doc Review 提示词 | [`DOC_REVIEW_PROMPT`](#doc_review_prompt) |
| `ModelEntry` | 结构体 | `src/config/manifest.rs:401` | 模型池条目 | [`ModelEntry`](#modelentry) |
| `ProviderDef` | 结构体 | `src/config/manifest.rs:416` | provider 定义 | [`ProviderDef`](#providerdef) |
| `AgentManifest` | 结构体 | `src/config/manifest.rs:427` | 单个 Agent 的配置清单 | [`AgentManifest`](#agentmanifest) |
| `ConfigManifest` | 结构体 | `src/config/manifest.rs:450` | 配置清单（真源） | [`ConfigManifest`](#configmanifest) |
| `builtin` | 方法 | `src/config/manifest.rs:466` | 内置默认清单 | [`builtin`](#builtin) |
| `resolve_model` | 方法 | `src/config/manifest.rs:651` | Agent 引用的模型 → 模型池条目 | [`resolve_model`](#resolve_model) |
| `resolve_provider` | 方法 | `src/config/manifest.rs:664` | Agent 引用的模型 → provider 定义 | [`resolve_provider`](#resolve_provider) |
| `get_agent` | 方法 | `src/config/manifest.rs:674` | 按名取 Agent | [`get_agent`](#get_agent) |
| `save` | 方法 | `src/config/manifest.rs:681` | 清单落盘为 JSON | [`save`](#save) |
| `load` | 方法 | `src/config/manifest.rs:689` | 从 JSON 读回清单 | [`load`](#load) |

`ConfigManifest`、`AgentManifest`、`ModelEntry`、`ProviderDef` 由 `src/config/mod.rs:12` 再导出到 config 模块根。

## 依赖

- **同仓模块**：`src/error.rs`——错误类型 `DevforgeError`、别名 `Result`、上下文扩展 `Context`（`ctx`），本模块三处引用（`src/config/manifest.rs:11`）。
- **外部 crate**：`serde`（`Serialize` / `Deserialize` derive）、`serde_json`（`save` / `load` 的 JSON 编解码）、`tempfile`（仅单测，用法见 `src/config/manifest.rs:883`）。
- **上游调用方**：应用层 `src/main.rs:153`（启动读清单）、`src/main.rs:155`（首次启动用内置清单并落盘）、HTTP 层 `src/server/handlers.rs:340`（GET 清单）、`src/server/handlers.rs:347`（PUT 清单，落盘在 `src/server/handlers.rs:360`）、渲染层 `src/config/render.rs:39`（`get_agent` + `resolve_model`）、`src/config/render.rs:69`（`resolve_provider`）。
- **下游消费者**：渲染层 `src/config/render.rs` 把本清单摊平成 profile 目录（AGENTS.md、settings、models.json、mcp.json、skills、装包）。
- **决策记录**：`docs/plantree/plans/002-config-management/decisions/README.md`——决策 001（Key 明文不设防）、002（模板化不搞通用 CRUD）、003（维护收敛律：最终只有模型可自定义）、004（模型固定分配）。
- **外部程序**：MCP server 的可执行程序 `codegraph`（`src/config/manifest.rs:57`）与 `npx`（`src/config/manifest.rs:67`）——本模块只声明命令，不负责拉起。

## 成员详述

### 数据模型

#### `McpServer`

**职责**：描述一个 stdio 型 MCP server（渲染进 `<agent-dir>/mcp.json`）；不做任何拉起或健康检查。

**构造或初始化**：N/A——纯数据结构，由 serde 反序列化或工厂函数构造，无 `new` 之类的构造函数。

**字段**（`src/config/manifest.rs:42`）：

| 字段 | 类型 | 约束 | 可空 | 说明 |
|---|---|---|---|---|
| `name` | `String` | server 名；工具名形如 `mcp__<name>__<tool>`（字母数字、下划线、连字符） | 否 | `src/config/manifest.rs:44` |
| `command` | `String` | 可执行程序名（stdio 传输） | 否 | `src/config/manifest.rs:46` |
| `args` | `Vec<String>` | 传给可执行程序的参数 | 否（可为空数组） | `src/config/manifest.rs:47` |
| `env` | `BTreeMap<String, String>` | `#[serde(default, skip_serializing_if = ...is_empty)]`：反序列化缺失时用空 map，序列化为空时整字段省略 | 否（缺省空 map） | `src/config/manifest.rs:50`（属性在 49 行） |

**约束**：仅 `env` 带 serde 默认/省略属性；其余字段缺失即反序列化失败。

**生命周期**：随 `AgentManifest` 的 `mcp` 字段一起存在清单里；渲染时写进 `<agent-dir>/mcp.json`，`mcp` 列表为空时该文件不生成（`src/config/manifest.rs:438`）。

**关系**：被 `AgentManifest` 的 `mcp` 字段（`src/config/manifest.rs:440`）以值内嵌持有；两个内置工厂 `codegraph_mcp`、`context7_mcp` 提供常用实例。

**典型用法**：

```rust
let server = codegraph_mcp();
assert_eq!(server.name, "codegraph");
```

#### `ModelEntry`

**职责**：模型池中的一条——把「池内引用 id」映射到具体 provider 与具体模型 id，并携带上下文窗口与输出预算。

**构造或初始化**：N/A——纯数据结构，无构造函数。

**字段**（`src/config/manifest.rs:401`）：

| 字段 | 类型 | 约束 | 可空 | 说明 |
|---|---|---|---|---|
| `id` | `String` | 池内唯一引用 id（Agent 用它选模型） | 否 | `src/config/manifest.rs:403` |
| `provider` | `String` | 必须能在 `providers` 中按 `name` 找到一条 | 否 | `src/config/manifest.rs:405` |
| `model_id` | `String` | pi 侧的模型标识（写入 settings 的 defaultModel） | 否 | `src/config/manifest.rs:407` |
| `context_window` | `u64` | 上下文窗口（渲染进 `<profile>/models.json`） | 否 | `src/config/manifest.rs:409` |
| `max_tokens` | `u64` | 最大输出 tokens（渲染进 `<profile>/models.json`） | 否 | `src/config/manifest.rs:411` |

**约束**：`id` 唯一性由 `resolve_model` 的线性查找隐式保证（先命中者胜，`src/config/manifest.rs:652`）；重复 id 不会报错，只会取第一条。

**生命周期**：清单级数据，随清单落盘/读盘；渲染层把它写进 profile 的模型配置。

**关系**：`AgentManifest` 的 `model` 字段（`src/config/manifest.rs:435`）引用本结构的 `id`；本结构的 `provider` 字段引用 `ProviderDef` 的 `name` 字段（`src/config/manifest.rs:418`）。

**迁移注意**：改动 `id` 会让引用它的 Agent 解析失败（`resolve_model` 报「不在模型池中」，`src/config/manifest.rs:656`）；改动 `model_id` 不会破坏引用，但会让 profile 指向另一个真实模型。

#### `ProviderDef`

**职责**：provider 定义——告诉 pi 用什么协议族与什么服务地址（渲染进 `<profile>/models.json`）。

**构造或初始化**：N/A——纯数据结构，无构造函数。

**字段**（`src/config/manifest.rs:416`）：

| 字段 | 类型 | 约束 | 可空 | 说明 |
|---|---|---|---|---|
| `name` | `String` | provider 名（pi settings 的 defaultProvider / auth.json 键） | 否 | `src/config/manifest.rs:418` |
| `api` | `String` | 协议族（如 `openai-completions`） | 否 | `src/config/manifest.rs:420` |
| `base_url` | `String` | 服务地址 | 否 | `src/config/manifest.rs:422` |

**约束**：`name` 唯一性由 `resolve_provider` 的线性查找隐式保证（`src/config/manifest.rs:666`）。

**生命周期**：清单级数据，随清单落盘/读盘。

**关系**：被 `ModelEntry` 的 `provider` 字段反向引用；`api_key` 也按 provider 名分键写入渲染产物（`src/config/render.rs:138`）。

**迁移注意**：改 `name` 会让引用它的模型解析失败（`resolve_provider` 报「无定义」，`src/config/manifest.rs:667`）。

#### `AgentManifest`

**职责**：单个 Agent 的配置清单——它同时是「profile 目录名来源」与「渲染输入」。

**构造或初始化**：N/A——纯数据结构，无构造函数。

**字段**（`src/config/manifest.rs:427`）：

| 字段 | 类型 | 约束 | 可空 | 说明 |
|---|---|---|---|---|
| `name` | `String` | Agent 名 = profile 目录名（如 `plan-agent`） | 否 | `src/config/manifest.rs:429` |
| `role_prompt` | `String` | AGENTS.md 正文（模板层，写死） | 否 | `src/config/manifest.rs:431` |
| `extra_packages` | `Vec<String>` | 在必装四件套之上追加的差异化包 | 否（可为空数组） | `src/config/manifest.rs:433` |
| `model` | `String` | 引用模型池条目 id | 否 | `src/config/manifest.rs:435` |
| `skills` | `Vec<String>` | 需要分发到 profile 的 skills | 否（可为空数组） | `src/config/manifest.rs:437` |
| `mcp` | `Vec<McpServer>` | `skip_serializing_if = "Vec::is_empty"`；空 = 不生成 mcp.json | 否（缺省空数组） | `src/config/manifest.rs:440` |
| `thinking_level` | `Option<String>` | `skip_serializing_if = "Option::is_none"`；`None` = 全局默认 high | 是 | `src/config/manifest.rs:445` |

**约束**：`model` 必须能在 `model_pool` 中解析，否则 `resolve_model` 报错（`src/config/manifest.rs:651`）；`name` 在清单内应唯一（`get_agent` 取第一条命中，`src/config/manifest.rs:675`）。

**生命周期**：随清单创建/整体替换；`PUT /api/manifest` 整份替换时旧 Agent 条目随清单消失（`src/server/handlers.rs:358`）。

**关系**：引用 `ModelEntry` 的 `id` 字段；内嵌 `McpServer` 列表；被 `ConfigManifest` 的 `agents` 字段持有。

**迁移注意**：`name` 改名等于换 profile 目录名（渲染层按名建目录，`src/config/render.rs:38`），旧 profile 目录不会被自动清理。

#### `ConfigManifest`

**职责**：清单真源——顶层容器，聚合必装包、provider 表、模型池、Agent 清单与 API Key。

**构造或初始化**：N/A——纯数据结构；构造入口是 `builtin`（内置默认）或 `load`（读盘）。

**字段**（`src/config/manifest.rs:450`）：

| 字段 | 类型 | 约束 | 可空 | 说明 |
|---|---|---|---|---|
| `base_packages` | `Vec<String>` | 所有 Agent 必装的包（内置值 = `BASE_PACKAGES`） | 否 | `src/config/manifest.rs:452` |
| `providers` | `Vec<ProviderDef>` | provider 定义表 | 否（可为空数组） | `src/config/manifest.rs:454` |
| `model_pool` | `Vec<ModelEntry>` | 模型池 | 否（可为空数组） | `src/config/manifest.rs:456` |
| `agents` | `Vec<AgentManifest>` | Agent 清单 | 否（可为空数组） | `src/config/manifest.rs:458` |
| `api_key` | `Option<String>` | API Key——明文不设防（决策 001）；`None` = 未配 | 是 | `src/config/manifest.rs:460` |

**约束**：`base_packages` / `providers` / `model_pool` / `agents` 四个字段没有 serde 默认属性——缺任一字段即反序列化失败（`load` 报「解析配置清单」，`src/config/manifest.rs:691`）。🟡 `api_key` 是 `Option<String>`，按 serde 派生语义字段缺失时取 `None` 而非报错（本仓无对应单测，见 `_meta/gaps.md` #4）。

**生命周期**：应用启动时从 `<home>/.devforge/manifest.json` 读入（`src/main.rs:153`），缺失则用 `builtin` 生成并落盘（`src/main.rs:155`）；运行期由 `PUT /api/manifest` 整份替换并回写落盘（`src/server/handlers.rs:360`）。

**关系**：顶层持有其余四个结构；`agents` 里的每条引用 `model_pool` 与 `providers`。

**迁移注意**：加必填字段会让已落盘的旧清单反序列化失败 → 启动即报错；新字段若要向后兼容应做成 `Option` 并配 `#[serde(default)]`（本仓两处 `Option` 字段：`thinking_level` 配了默认属性，`src/config/manifest.rs:444`；`api_key` 未配，`src/config/manifest.rs:460`）——确切兼容边界见 `_meta/gaps.md` #4。

### 公开函数

#### `codegraph_mcp`

**摘要**：造一个指向本机 `codegraph` 可执行程序的 MCP server 定义。

**行为语义**：固定 `name = "codegraph"`、`command = "codegraph"`、`args = ["serve", "--mcp"]`；固定注入 `CODEGRAPH_TELEMETRY=0`（关闭遥测，决策 013 卫生条款）（`src/config/manifest.rs:54`—`src/config/manifest.rs:61`）。无副作用、每次调用返回等价新值。

**参数**：N/A——无参数。

**返回**：`McpServer`（值）。

**异常**：N/A——不返回 `Result`，无失败路径。

**示例**：

```rust
let mcp = codegraph_mcp();
assert_eq!(mcp.name, "codegraph");
assert_eq!(mcp.env.get("CODEGRAPH_TELEMETRY").map(String::as_str), Some("0"));
```

#### `context7_mcp`

**摘要**：造一个用 `npx` 拉起 context7 的 MCP server 定义（查第三方库最新文档用）。

**行为语义**：固定 `name = "context7"`、`command = "npx"`、`args = ["-y", "@upstash/context7-mcp"]`；`env` 为空 map（`src/config/manifest.rs:64`—`src/config/manifest.rs:71`）。无副作用。

**参数**：N/A——无参数。

**返回**：`McpServer`（值）。

**异常**：N/A——不返回 `Result`，无失败路径。

**示例**：

```rust
let mcp = context7_mcp();
assert_eq!(mcp.command, "npx");
```

### 方法

#### `builtin`

**摘要**：构造内置默认清单——规划/编码/测试/文档共 9 个 Agent + 固定模型池 + 单 provider。

**行为语义**：

- `base_packages` = `BASE_PACKAGES` 逐项转成 `String`（`src/config/manifest.rs:502`）。
- `providers` = 1 条 `newapi`，协议族 `openai-completions`，地址 `https://newapi.trtyr.top/v1`（`src/config/manifest.rs:504`—`src/config/manifest.rs:508`）。
- `model_pool` = 3 条：`grok-4.7`、`deepseek-flash`、`minimax-m3.1-flash`；均为 `provider = "newapi"`、`context_window = 1_000_000`、`max_tokens = 65_536`（`src/config/manifest.rs:511`—`src/config/manifest.rs:532`）。🟢 依据：三条条目的 `context_window` / `max_tokens` 取值见 `src/config/manifest.rs:516`。
- `agents` = 9 条（`src/config/manifest.rs:534`）：plan-agent、plan-review、coding-agent、coding-review、test-agent、test-review、doc-fix、doc-agent、doc-review。
- `api_key` = `None`（`src/config/manifest.rs:646`）——内置清单不带 key。
- plan 双 Agent 的 `role_prompt` 是函数内局部字符串 `plan_agent_prompt`（`src/config/manifest.rs:467`）与 `review_prompt`（`src/config/manifest.rs:484`），其余 7 个 Agent 直接引用提示词常量。
- 🟡 模型分配是「用户拍板固定死」的终态配置，不是运行时推导——依据：`src/config/manifest.rs:548` 的行内注释与决策记录 `docs/plantree/plans/002-config-management/decisions/README.md` 决策 004。
- 无副作用、每次调用返回等价新清单（可安全多次调用）。

**参数**：N/A——无参数（`&self` 之外的关联函数）。

**返回**：`ConfigManifest`（值）。

**异常**：N/A——不返回 `Result`，构造过程无失败路径。

**示例**：

```rust
let manifest = ConfigManifest::builtin();
assert_eq!(manifest.agents.len(), 9);
assert_eq!(manifest.model_pool.len(), 3);
```

#### `resolve_model`

**摘要**：按 Agent 引用的模型 id 在模型池里找到对应条目。

**行为语义**：在 `model_pool` 中线性查找 `id == agent.model` 的第一条（`src/config/manifest.rs:652`—`src/config/manifest.rs:654`）；找不到即报错。纯查询，无副作用。

**参数**：

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `agent` | `&AgentManifest` | 其 `model` 字段须能在模型池中命中 | 无默认值，必填 |

**返回**：`Result<&ModelEntry>`——命中返回模型池中该条目的只读引用（生命周期绑定 `self`）。

**异常**：`DevforgeError` 的 `Config` 变体，消息 `Agent \`<name>\` 引用的模型 \`<model>\` 不在模型池中`（`src/config/manifest.rs:656`—`src/config/manifest.rs:659`）。触发条件：模型 id 不在池中。

**示例**：

```rust
let manifest = ConfigManifest::builtin();
let agent = manifest.get_agent("plan-agent").unwrap();
let model = manifest.resolve_model(agent).unwrap();
assert_eq!(model.provider, "newapi");
```

#### `resolve_provider`

**摘要**：按 Agent 引用的模型找到其 provider 定义。

**行为语义**：先调 `resolve_model`（模型不存在则直接返回该错误），再在 `providers` 中按 `name == model.provider` 找第一条（`src/config/manifest.rs:665`、`src/config/manifest.rs:666`）。纯查询，无副作用。

**参数**：

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `agent` | `&AgentManifest` | 其模型须能解析，且模型的 provider 名须有定义 | 无默认值，必填 |

**返回**：`Result<&ProviderDef>`——命中返回 provider 定义只读引用。

**异常**：`DevforgeError` 的 `Config` 变体，两种消息：模型不在池中（透传 `resolve_model` 的错误）；或 `模型 \`<id>\` 引用的 provider \`<name>\` 无定义`（`src/config/manifest.rs:667`—`src/config/manifest.rs:670`）。

**示例**：

```rust
let manifest = ConfigManifest::builtin();
let agent = manifest.get_agent("coding-agent").unwrap();
let provider = manifest.resolve_provider(agent).unwrap();
assert_eq!(provider.name, "newapi");
```

#### `get_agent`

**摘要**：按 Agent 名取清单中那一条。

**行为语义**：在 `agents` 中线性查找 `name` 完全相等的第一条（`src/config/manifest.rs:675`—`src/config/manifest.rs:677`）；名字大小写敏感。纯查询，无副作用。

**参数**：

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `name` | `&str` | 须与某个 `AgentManifest` 的 `name` 字段完全相等 | 无默认值，必填 |

**返回**：`Result<&AgentManifest>`——命中返回该 Agent 条目的只读引用。

**异常**：`DevforgeError` 的 `Config` 变体，消息 `清单中无 Agent \`<name>\``（`src/config/manifest.rs:678`）。

**示例**：

```rust
let manifest = ConfigManifest::builtin();
assert!(manifest.get_agent("doc-review").is_ok());
```

#### `save`

**摘要**：把清单序列化成 pretty JSON 落盘。

**行为语义**：先由 `serde_json` 的 `to_string_pretty` 序列化（失败 → `ctx("序列化配置清单")`），再在 `path` 有父目录时 `create_dir_all` 建目录，最后用标准库写文件（`src/config/manifest.rs:682`—`src/config/manifest.rs:686`）。整体覆盖写（非追加）；同路径重复调用会覆盖旧内容。写盘后无任何回读校验。

**参数**：

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `path` | `&Path` | 目标文件路径；父目录不存在会被自动创建 | 无默认值，必填 |

**返回**：`Result<()>`——成功为 `Ok(())`。

**异常**：`DevforgeError` 的 `WithContext` 变体，上下文分别为 `序列化配置清单`、`创建清单目录`、`写入清单 <path>`（`src/config/manifest.rs:682`、`src/config/manifest.rs:684`、`src/config/manifest.rs:686`）。触发条件：序列化失败、目录创建失败、写文件失败（权限/磁盘满等）。

**示例**：

```rust
let manifest = ConfigManifest::builtin();
manifest.save(Path::new("/tmp/devforge-demo/manifest.json")).unwrap();
```

#### `load`

**摘要**：从 JSON 文件读回一份清单。

**行为语义**：先读全文（标准库文件读），再由 `serde_json` 的 `from_str` 解析（`src/config/manifest.rs:690`、`src/config/manifest.rs:691`）。不做字段级默认值填充：缺必填字段即整体失败。

**参数**：

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `path` | `&Path` | 清单文件路径 | 无默认值，必填 |

**返回**：`Result<ConfigManifest>`——成功返回解析出的清单（拥有所有权）。

**异常**：`DevforgeError` 的 `WithContext` 变体，上下文 `读取清单 <path>`（读失败）或 `解析配置清单`（JSON 非法或字段缺失）（`src/config/manifest.rs:690`、`src/config/manifest.rs:691`）。

**示例**：

```rust
let manifest = ConfigManifest::load(Path::new("/tmp/devforge-demo/manifest.json")).unwrap();
assert_eq!(manifest.agents.len(), 9);
```

### 包名常量

#### `BASE_PACKAGES`

**摘要**：所有 Agent 必装的四件套（用户拍板 2026-10-16，包名原文照录）。

**内容**（`src/config/manifest.rs:14`—`src/config/manifest.rs:19`）：

```
npm:@ff-labs/pi-fff
npm:@juicesharp/rpiv-web-tools
npm:pi-blackhole
git:github.com/NVlabs/SoL-Pi
```

**语义**：切片刻面（`&[&str]`），渲染层与 `builtin` 都从它取全量必装包。🟡 四件套的具体组合是「用户拍板」的终态选择，依据是 `src/config/manifest.rs:13` 的注释与决策记录 `docs/plantree/plans/002-config-management/decisions/README.md` 决策 002。

#### `PLAN_AGENT_EXTRA`

**摘要**：仅 Plan Agent 的差异化包——单个 `&str`（`src/config/manifest.rs:22`）：

```
npm:@juicesharp/rpiv-ask-user-question
```

🟡 依据是注释「完成确认机制依赖 ask-user-question」（`src/config/manifest.rs:21`）——意图声明，不是可执行约束。

#### `CODING_AGENT_EXTRA`

**摘要**：Coding Agent 的差异化包——3 项切片（`src/config/manifest.rs:25`）：

```
npm:@juicesharp/rpiv-ask-user-question
npm:@juicesharp/rpiv-todo
npm:pi-lens
```

🟡 依据是注释列出的三项来源：ask-user-question 偏差协议、待办纪律、诊断层（`src/config/manifest.rs:24`）。

#### `CODING_REVIEW_EXTRA`

**摘要**：Coding Review 的差异化包——1 项切片（`src/config/manifest.rs:32`）：

```
npm:pi-lens
```

🟡 依据是注释：只配诊断证据包、无状态一轮不配待办工具（`src/config/manifest.rs:31`）。

#### `TEST_AGENT_EXTRA`

**摘要**：Test Agent 的差异化包——2 项切片（`src/config/manifest.rs:35`）：

```
npm:@juicesharp/rpiv-todo
npm:@juicesharp/rpiv-ask-user-question
```

🟡 依据是注释：待办纪律 + 偏差出口，且刻意不给 pi-lens（黑盒纪律）（`src/config/manifest.rs:34`）。

### 提示词常量

七段提示词是全仓唯一的 Agent 指令定义处，渲染层把它们写进 profile 的 `<profile>/AGENTS.md`。本节只给「这段管什么 + 落点」，不复述正文（正文在源码里，第三方按 `src/config/manifest.rs:74` 等行号直读）。

| 常量 | 落点 | 管什么 |
|---|---|---|
| `CODING_AGENT_PROMPT` | `src/config/manifest.rs:74` | 编码 Agent：开工顺序、开发纪律、工具纪律、plan 偏差协议、提交前预审、阶段完成协议 |
| `CODING_REVIEW_PROMPT` | `src/config/manifest.rs:135` | 编码审查 Agent：8 项审查面（含 doc-lint 复核、待办清单实物审计）、输出协议 `stage-verdict` / `next` |
| `TEST_AGENT_PROMPT` | `src/config/manifest.rs:178` | 测试 Agent：黑盒纪律（白名单 / 绝对禁令）、测试写作纪律、测试自身可靠性 7 条、发现协议、追溯矩阵 |
| `TEST_REVIEW_PROMPT` | `src/config/manifest.rs:241` | 测试审查 Agent：对文档三判据、追溯矩阵审计、黑盒纪律审计、根因判定（test-side / doc-side） |
| `DOC_FIX_PROMPT` | `src/config/manifest.rs:288` | 文档修补 Agent：只改文档不改代码、按 findings 逐条处置、Runbook 七件套保持 |
| `DOC_AGENT_PROMPT` | `src/config/manifest.rs:312` | 文档 Agent（反向写文档的干活方）：只写 `docs/contexts/`、codegraph-first、事实与推断分离、阶段流水、交审前自检 |
| `DOC_REVIEW_PROMPT` | `src/config/manifest.rs:371` | 文档审查 Agent：findings 处置完整性、plan tree 一致性、内部自洽、Runbook 七件套契约、第三方可测标准 |

#### `CODING_AGENT_PROMPT`

**摘要**：编码 Agent 的完整系统提示词（`&str` 常量）。

**行为语义**：以 `\n\` 续行拼装的字符串常量，止于 `src/config/manifest.rs:132`；内容含「开工（顺序强制）」等 7 段。🟡 提示词按「定稿即定死」维护——依据是常量上方注释「勿手改——改决策先改 plan tree」（`src/config/manifest.rs:73`）。

**参数**：N/A——无参数。

**返回**：N/A——常量，非函数；取值方式是按名引用。

**异常**：N/A——无失败路径。

#### `CODING_REVIEW_PROMPT`

**摘要**：编码审查 Agent 的完整系统提示词（`&str` 常量，`src/config/manifest.rs:135`）。

**行为语义**：同上形态，止于 `src/config/manifest.rs:175`；含审查面 8 项与输出协议（`stage-verdict` / `next`）。🟡 定稿即定死，依据注释 `src/config/manifest.rs:134`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

#### `TEST_AGENT_PROMPT`

**摘要**：测试 Agent 的完整系统提示词（`&str` 原始字符串常量，`src/config/manifest.rs:178`）。

**行为语义**：原始字符串（`r#"…"#`）形式，止于 `src/config/manifest.rs:238`；含黑盒纪律的绝对禁令与「测试自身可靠性」7 条（禁自指跑整套测试、外部命令带短超时、进程组回收、禁假绿等）。🟡 定稿即定死，依据注释 `src/config/manifest.rs:177`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

#### `TEST_REVIEW_PROMPT`

**摘要**：测试审查 Agent 的完整系统提示词（`&str` 原始字符串常量，`src/config/manifest.rs:241`）。

**行为语义**：止于 `src/config/manifest.rs:285`；含根因判定标签（test-side / doc-side）与审查省时纪律。🟡 定稿即定死，依据注释 `src/config/manifest.rs:240`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

#### `DOC_FIX_PROMPT`

**摘要**：文档修补 Agent 的完整系统提示词（`&str` 原始字符串常量，`src/config/manifest.rs:288`）。

**行为语义**：止于 `src/config/manifest.rs:309`；核心约束是「本轮只改文档，不改代码」与「第三方可测标准」「Runbook 契约保持」。🟡 定稿即定死，依据注释 `src/config/manifest.rs:287`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

#### `DOC_AGENT_PROMPT`

**摘要**：文档 Agent（反向写文档的干活方）的完整系统提示词（`&str` 原始字符串常量，`src/config/manifest.rs:312`）。

**行为语义**：止于 `src/config/manifest.rs:368`；含 9 条硬纪律（只写 `docs/contexts/`、停进程只准按 PID、禁止删除非本单元产物、codegraph-first、事实与推断分离、按 doc-contract 写、单次上下文约 4K、一单元一提交、跨模块只建引用）与三阶段流水（勘察 → 解释 → 生成）、交审前自检 5 条、`<doc-complete>unit-xxx</doc-complete>` 完成协议。🟡 定稿即定死，依据注释 `src/config/manifest.rs:311`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

#### `DOC_REVIEW_PROMPT`

**摘要**：文档审查 Agent 的完整系统提示词（`&str` 原始字符串常量，`src/config/manifest.rs:371`）。

**行为语义**：止于 `src/config/manifest.rs:397`；含 5 项审查面与输出协议（`stage-verdict`，缺协议块按 FAIL 处理）。🟡 定稿即定死，依据注释 `src/config/manifest.rs:370`。

**参数**：N/A——常量。

**返回**：N/A——常量。

**异常**：N/A——无失败路径。

## 行为规格

| behavior | 落点 | 断言 | 置信度 |
|---|---|---|---|
| `builtin_has_nine_agents_and_four_base_packages` | `src/config/manifest.rs:700` | 内置清单 = 4 个必装包 + 9 个 Agent；逐个断言各 Agent 的模型、差异化包、skills、mcp、思考档位 | 🟢 `src/config/manifest.rs:700` |
| `doc_contract_skill_is_shared_by_four_profiles` | `src/config/manifest.rs:761` | `coding-agent`、`coding-review`、`doc-fix`、`doc-review` 四个 profile 的 skills 都含 `doc-contract` | 🟢 `src/config/manifest.rs:761` |
| `doc_agent_prompt_carries_protocol_and_guards` | `src/config/manifest.rs:774` | `DOC_AGENT_PROMPT` 含 16 个关键串（只写 `docs/contexts/`、codegraph-first、事实与推断分离、path:line、_meta/gaps.md、`N/A`、完成协议标记、doc-lint 命令、阶段流水锚点等） | 🟢 `src/config/manifest.rs:774` |
| `coding_agent_prompt_has_preflight_audit` | `src/config/manifest.rs:794` | `CODING_AGENT_PROMPT` 含提交前预审、绝对词、断言必须能失败、预审证据、doc-lint 退出码 0 等锚点；`CODING_REVIEW_PROMPT` 含 doc-lint 复核与「自己跑一遍」 | 🟢 `src/config/manifest.rs:794` |
| `testing_prompts_carry_protocol_and_discipline` | `src/config/manifest.rs:810` | 测试双 Agent 与审查双 Agent 的提示词含黑盒禁令、可靠性 7 条、reroute 协议、省时纪律、文档双 Agent 锚点 | 🟢 `src/config/manifest.rs:810` |
| `model_resolution_from_pool` | `src/config/manifest.rs:861` | plan-agent → provider `newapi` + 模型 id mokeywork/deepseek-flash；plan-review → mokeywork/grok-4.7 | 🟢 `src/config/manifest.rs:861` |
| `unknown_model_rejected` | `src/config/manifest.rs:873` | 模型 id 改成不存在的值后 `resolve_model` 报错，错误串含「不在模型池中」 | 🟢 `src/config/manifest.rs:873` |
| `manifest_json_roundtrip` | `src/config/manifest.rs:881` | `builtin` → `save` → `load` 后与原清单逐字段相等 | 🟢 `src/config/manifest.rs:881` |

## 错误矩阵

| 场景 | 触发点 | 返回 | 错误体形状 | 客户端应如何处理 |
|---|---|---|---|---|
| Agent 引用的模型不在池中 | `src/config/manifest.rs:655` | `Err` | `DevforgeError` 的 `Config` 变体：`Agent \`<name>\` 引用的模型 \`<model>\` 不在模型池中` | 把该 Agent 的 `model` 改成模型池里已存在的 id，或往 `model_pool` 补一条 |
| 模型引用的 provider 无定义 | `src/config/manifest.rs:666` | `Err` | `DevforgeError` 的 `Config` 变体：`模型 \`<id>\` 引用的 provider \`<name>\` 无定义` | 往 `providers` 补一条同名定义，或改模型的 `provider` 指向已有定义 |
| 清单里没有该 Agent | `src/config/manifest.rs:678` | `Err` | `DevforgeError` 的 `Config` 变体：`清单中无 Agent \`<name>\`` | 核对 Agent 名大小写与清单里的 `agents[].name` |
| 序列化失败 | `src/config/manifest.rs:682` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `序列化配置清单` | 实际不可达（字段只有 String/u64/Vec/Option/BTreeMap）——真出现即说明 schema 变了 |
| 建清单目录失败 | `src/config/manifest.rs:684` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `创建清单目录` | 检查父目录写权限 |
| 写清单文件失败 | `src/config/manifest.rs:686` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `写入清单 <path>` | 检查磁盘空间与文件权限（写为覆盖式） |
| 读清单文件失败 | `src/config/manifest.rs:690` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `读取清单 <path>` | 确认 `<home>/.devforge/manifest.json` 存在且可读；缺失时应用会自动用内置清单重建（`src/main.rs:155`） |
| 清单 JSON 非法 / 缺必填字段 | `src/config/manifest.rs:691` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `解析配置清单`（内层为 `Json`） | 按报错的 missing field 补齐字段；或删掉该文件让应用重建内置清单（`src/main.rs:155`） |

## 运行与验证

- **环境要求**：可编译的 Rust 工具链（edition 2024，依赖声明见 `Cargo.toml`）；本模块只提供数据与函数，没有常驻进程与端口。
- **依赖安装**：在仓库根目录执行 `cargo build` 拉齐 `serde`、`serde_json`、`thiserror`；跑单测还需 dev 依赖 `tempfile`（见 `Cargo.toml`）。
- **配置项**：本模块无运行时配置项；清单文件本身是配置（路径由应用层定为 `<home>/.devforge/manifest.json`，`src/main.rs:151`）。清单内影响渲染结果的键：`base_packages`、`agents[].extra_packages`、`agents[].skills`、`agents[].mcp`、`agents[].thinking_level`、`agents[].model`、`model_pool`、`providers`、`api_key`。
- **启停与健康检查**：N/A——本模块是库，没有可启停的进程；健康检查等价于「`load` 能读回、`resolve_model` / `resolve_provider` 对每个 Agent 都能解析」（服务端在 `PUT /api/manifest` 时就是这么校验的，`src/server/handlers.rs:353`）。
- **种子数据**：N/A——无数据库；`builtin`（`src/config/manifest.rs:466`）就是「无外部数据时的默认清单」。
- **测试运行**：`cargo test config::manifest`（覆盖本模块 8 个单测）。🔴 本机无 Rust 工具链（`rustup toolchain list` 输出 no installed toolchains），本轮未实际执行——见 `_meta/gaps.md` #1。
- **公开接口清单**：`BASE_PACKAGES`、`PLAN_AGENT_EXTRA`、`CODING_AGENT_EXTRA`、`CODING_REVIEW_EXTRA`、`TEST_AGENT_EXTRA`、`CODING_AGENT_PROMPT`、`CODING_REVIEW_PROMPT`、`TEST_AGENT_PROMPT`、`TEST_REVIEW_PROMPT`、`DOC_FIX_PROMPT`、`DOC_AGENT_PROMPT`、`DOC_REVIEW_PROMPT`、`McpServer`、`ModelEntry`、`ProviderDef`、`AgentManifest`、`ConfigManifest`、`codegraph_mcp`、`context7_mcp`、`builtin`、`resolve_model`、`resolve_provider`、`get_agent`、`save`、`load`——见「公开成员」表。
