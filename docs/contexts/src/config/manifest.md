---
type: module
unit: unit-src-01
module: src/config/manifest.rs
---

# src/config/manifest.rs

配置清单的 schema 与内置内容：清单是「配置即代码」的真源，模板层写死、模型层可变，本模块同时是这两层的唯一定义处。

## 职责

**负责**：

- 定义清单 schema：5 个可序列化结构（`ConfigManifest` / `AgentManifest` / `ModelEntry` / `ProviderDef` / `McpServer`），并派生 `Serialize` / `Deserialize`（`src/config/manifest.rs:42`、`src/config/manifest.rs:400`、`src/config/manifest.rs:415`、`src/config/manifest.rs:426`、`src/config/manifest.rs:449`）。
- 给出内置默认清单内容：1 个 provider、3 个模型池条目、9 个 Agent、4 个必装包（`src/config/manifest.rs:465`）。
- 解析引用关系：Agent → 模型 → provider 三级解析，找不到即报错（`src/config/manifest.rs:650`、`src/config/manifest.rs:663`）。
- 清单的读写：序列化成 pretty JSON 落盘、反序列化回来（`src/config/manifest.rs:680`、`src/config/manifest.rs:688`）。
- 提供包名常量、MCP 定义工厂与 7 段 Agent 提示词的唯一定义处（`src/config/manifest.rs:14` 起）。

**不负责**：

- 把清单渲染成 profile 目录（写 `<profile>/AGENTS.md`、`<profile>/mcp.json`、`<profile>/models.json`、复制 skills、装包）——渲染层 `src/config/render.rs` 的事。
- 决定装包机制（`pi install` 怎么调）——装包层 `src/config/installer.rs` 的事。
- 决定清单落盘路径与进程内状态归属：路径 `<home>/.devforge/manifest.json` 由应用层 `src/main.rs:151` 决定，HTTP 入口在 `src/server/handlers.rs:340`。
- 调模型 API：本模块只描述「哪个 Agent 用哪个模型」，不发任何请求。

## 公开成员

| 符号 | 种类 | 行 | 一句话 | 详见 |
|---|---|---|---|---|
| `BASE_PACKAGES` | 常量 | `src/config/manifest.rs:14` | 必装四件套（所有 Agent） | [BASE_PACKAGES](#base_packages) |
| `PLAN_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:22` | Plan Agent 差异化包（单包） | [PLAN_AGENT_EXTRA](#plan_agent_extra) |
| `CODING_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:25` | 编码家族差异化包（3 个） | [CODING_AGENT_EXTRA](#coding_agent_extra) |
| `CODING_REVIEW_EXTRA` | 常量 | `src/config/manifest.rs:32` | 编码审查差异化包（1 个） | [CODING_REVIEW_EXTRA](#coding_review_extra) |
| `TEST_AGENT_EXTRA` | 常量 | `src/config/manifest.rs:35` | 测试 Agent 差异化包（2 个） | [TEST_AGENT_EXTRA](#test_agent_extra) |
| `McpServer` | 结构体 | `src/config/manifest.rs:42` | MCP server 定义（pi 原生 schema） | [McpServer](#mcpserver) |
| `codegraph_mcp` | 函数 | `src/config/manifest.rs:54` | CodeGraph 事实层 MCP 定义 | [codegraph_mcp](#codegraph_mcp) |
| `context7_mcp` | 函数 | `src/config/manifest.rs:64` | 外部库文档 MCP 定义 | [context7_mcp](#context7_mcp) |
| `CODING_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:74` | 编码 Agent 提示词 | [提示词常量](#提示词常量) |
| `CODING_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:135` | 编码审查 Agent 提示词 | [提示词常量](#提示词常量) |
| `TEST_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:178` | 测试 Agent 提示词 | [提示词常量](#提示词常量) |
| `TEST_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:241` | 测试审查 Agent 提示词 | [提示词常量](#提示词常量) |
| `DOC_FIX_PROMPT` | 常量 | `src/config/manifest.rs:288` | 文档修补 Agent 提示词 | [提示词常量](#提示词常量) |
| `DOC_AGENT_PROMPT` | 常量 | `src/config/manifest.rs:312` | 文档 Agent 提示词 | [提示词常量](#提示词常量) |
| `DOC_REVIEW_PROMPT` | 常量 | `src/config/manifest.rs:370` | 文档审查 Agent 提示词 | [提示词常量](#提示词常量) |
| `ModelEntry` | 结构体 | `src/config/manifest.rs:400` | 模型池条目 | [ModelEntry](#modelentry) |
| `ProviderDef` | 结构体 | `src/config/manifest.rs:415` | provider 定义 | [ProviderDef](#providerdef) |
| `AgentManifest` | 结构体 | `src/config/manifest.rs:426` | 单个 Agent 的配置 | [AgentManifest](#agentmanifest) |
| `ConfigManifest` | 结构体 | `src/config/manifest.rs:449` | 清单真源 | [ConfigManifest](#configmanifest) |
| `builtin` | 方法 | `src/config/manifest.rs:465` | 造出内置默认清单 | [builtin](#builtin) |
| `resolve_model` | 方法 | `src/config/manifest.rs:650` | Agent → 模型池条目 | [resolve_model](#resolve_model) |
| `resolve_provider` | 方法 | `src/config/manifest.rs:663` | Agent → provider 定义 | [resolve_provider](#resolve_provider) |
| `get_agent` | 方法 | `src/config/manifest.rs:673` | 按名取 Agent | [get_agent](#get_agent) |
| `save` | 方法 | `src/config/manifest.rs:680` | 落盘清单 | [save](#save) |
| `load` | 方法 | `src/config/manifest.rs:688` | 读回清单 | [load](#load) |

## 依赖

- **同仓模块**：`src/error.rs`——错误类型 `DevforgeError`、别名 `Result`、上下文扩展 `Context`（`src/config/manifest.rs:11`）。
- **外部 crate**：`serde`（`Serialize` / `Deserialize` 派生）、`serde_json`（`to_string_pretty` / `from_str`）。
- **标准库**：`Path`（std::path，落盘路径，`src/config/manifest.rs:9`）。
- **无网络依赖**：本模块不发起任何请求，也不读环境变量。
- **上游调用方**：`src/main.rs:153`（启动时 `load`）、`src/main.rs:155`（首次启动 `builtin` + `save`）、`src/server/handlers.rs:340`（GET 清单）、`src/server/handlers.rs:347`（PUT 清单，落盘 `src/server/handlers.rs:359`）、渲染层 `src/config/render.rs`。

## 数据模型

### `ConfigManifest`

#### 字段

| 字段 | 类型 | 可空 | 说明 | 行 |
|---|---|---|---|---|
| `base_packages` | `Vec<String>` | 否 | 必装四件套，所有 Agent 共用 | `src/config/manifest.rs:451` |
| `providers` | `Vec<ProviderDef>` | 否 | provider 定义表（模型管理面） | `src/config/manifest.rs:453` |
| `model_pool` | `Vec<ModelEntry>` | 否 | 模型池（模型管理面） | `src/config/manifest.rs:455` |
| `agents` | `Vec<AgentManifest>` | 否 | Agent 清单（Agent 管理面） | `src/config/manifest.rs:457` |
| `api_key` | `Option<String>` | 是 | API Key，明文存放 | `src/config/manifest.rs:459` |

#### 约束

- 五个字段在反序列化时都必填——`api_key` 是 `Option`，缺失按 `None` 处理；其余四个字段缺失直接报 missing field——🟡 依据 serde 派生语义，本仓无「缺字段加载」单测覆盖。
- `api_key` 没有标 `skip_serializing_if`，因此落盘 JSON 里该键恒存在（值为 `null` 也写出来）（`src/config/manifest.rs:459`）。🟢
- 结构体本身不校验引用完整性：`agents[].model` 指向不存在的池条目、池条目指向不存在的 provider，都要等到 `resolve_model` / `resolve_provider` 才报错（`src/config/manifest.rs:650`、`src/config/manifest.rs:663`）。🟢
- 唯一性无结构级约束：`model_pool[].id`、`agents[].name` 重复时，解析取首个匹配（`src/config/manifest.rs:651`、`src/config/manifest.rs:674`）。🟢

#### 生命周期

- 落盘位置：`<home>/.devforge/manifest.json`（路径由应用层决定，`src/main.rs:151`）。
- 写：启动时不存在则由 `builtin()` 生成并 `save`（`src/main.rs:155`、`src/main.rs:156`）；HTTP PUT 清单成功后由 `save` 覆盖（`src/server/handlers.rs:359`）。
- 读：启动时 `load`（`src/main.rs:153`）。
- 删：代码里没有删除清单的路径——只找到两处写盘（`src/main.rs:156`、`src/server/handlers.rs:360`）。🟢

#### 关系

- `ConfigManifest` 1—N `AgentManifest`（`agents` 内嵌）。
- `AgentManifest` N—1 `ModelEntry`（按 `model` 字段的 id 引用，不是指针）。
- `ModelEntry` N—1 `ProviderDef`（按池条目的 `provider` 字段引用）。
- `AgentManifest` 1—N `McpServer`（内嵌，非引用）。

#### 迁移注意

- 改字段名/增删必填字段会让已落盘的 `<home>/.devforge/manifest.json` 反序列化失败，启动即报错（错误来自 `load` 的 `ctx("解析配置清单")`，`src/config/manifest.rs:690`）。🟢
- 新增可选字段应配 `#[serde(default)]`，否则旧清单读不出来。🟡 依据 serde 派生语义与本模块既有写法（`mcp` / `thinking_level` / `env` 三个可选字段都带 `default`）。

### `AgentManifest`

#### 字段

| 字段 | 类型 | 可空 | 说明 | 行 |
|---|---|---|---|---|
| `name` | `String` | 否 | Agent 名，等于 profile 目录名 | `src/config/manifest.rs:428` |
| `role_prompt` | `String` | 否 | AGENTS.md 正文（模板层，写死） | `src/config/manifest.rs:430` |
| `extra_packages` | `Vec<String>` | 否 | 在必装四件套之上追加的差异化包 | `src/config/manifest.rs:432` |
| `model` | `String` | 否 | 引用的模型池条目 id | `src/config/manifest.rs:434` |
| `skills` | `Vec<String>` | 否 | 需要分发到 profile 的 skills 名 | `src/config/manifest.rs:436` |
| `mcp` | `Vec<McpServer>` | 否 | MCP servers；空则不生成该文件 | `src/config/manifest.rs:439` |
| `thinking_level` | `Option<String>` | 是 | 思考档位覆盖；`None` = 全局默认 high | `src/config/manifest.rs:444` |

#### 约束

- `mcp` 带 `#[serde(default, skip_serializing_if = "Vec::is_empty")]`：缺字段按空数组处理，为空则该键不出现在 JSON 里（`src/config/manifest.rs:438`）。🟢
- `thinking_level` 带 `#[serde(default, skip_serializing_if = "Option::is_none")]`：`None` 时该键不出现在 JSON 里（`src/config/manifest.rs:443`）。🟢
- `model` 只是字符串 id，指向关系靠 `resolve_model` 在运行期解析（`src/config/manifest.rs:434`）。🟢
- `thinking_level` 的取值集合由 pi 决定，本模块只透传字符串，内置值只有 `medium`（审查类 Agent）与 `None`（干活方 Agent）（`src/config/manifest.rs:550`）。🟢

#### 生命周期

由 `builtin()` 一次性构造，之后随清单整体加载/落盘；无独立增删接口（增删 Agent = 改整份清单）。

#### 关系

- 引用 `model_pool` 中的一个 `ModelEntry`（id 匹配）。
- 内嵌 `mcp` 列表；`skills` 只是名字列表，skill 正文由技能资产层提供。

### `ModelEntry`

#### 字段

| 字段 | 类型 | 可空 | 说明 | 行 |
|---|---|---|---|---|
| `id` | `String` | 否 | 池内引用 id，Agent 用它选模型 | `src/config/manifest.rs:402` |
| `provider` | `String` | 否 | 对应的 provider 名 | `src/config/manifest.rs:404` |
| `model_id` | `String` | 否 | pi settings 的 `defaultModel` | `src/config/manifest.rs:406` |
| `context_window` | `u64` | 否 | 上下文窗口（渲染 `<profile>/models.json`） | `src/config/manifest.rs:408` |
| `max_tokens` | `u64` | 否 | 最大输出 tokens | `src/config/manifest.rs:410` |

#### 约束

五个字段全部必填、无 `skip_serializing_if`（`src/config/manifest.rs:400`）。🟢
`context_window` / `max_tokens` 是渲染层写进 `<profile>/models.json` 的预算值（`src/config/manifest.rs:407`）。🟢

#### 生命周期

随清单整体增删；内置三条见「内置清单快照」。

#### 关系

`provider` 字段指向 `providers` 中的一条 `ProviderDef`。

### `ProviderDef`

#### 字段

| 字段 | 类型 | 可空 | 说明 | 行 |
|---|---|---|---|---|
| `name` | `String` | 否 | provider 名（pi settings 的 `defaultProvider`） | `src/config/manifest.rs:417` |
| `api` | `String` | 否 | 协议族，如 `openai-completions` | `src/config/manifest.rs:419` |
| `base_url` | `String` | 否 | 服务地址 | `src/config/manifest.rs:421` |

#### 约束

三字段必填（`src/config/manifest.rs:415`）。🟢 内置只有一条：`newapi` / `openai-completions` / https://newapi.trtyr.top/v1（`src/config/manifest.rs:503`）。🟢

#### 生命周期

随清单整体增删。

#### 关系

被池条目的 `provider` 字段引用；`resolve_provider` 按名匹配（`src/config/manifest.rs:665`）。🟢

### `McpServer`

#### 字段

| 字段 | 类型 | 可空 | 说明 | 行 |
|---|---|---|---|---|
| `name` | `String` | 否 | server 名，工具名前缀为 `mcp__<name>__<tool>` | `src/config/manifest.rs:44` |
| `command` | `String` | 否 | 可执行程序（stdio） | `src/config/manifest.rs:46` |
| `args` | `Vec<String>` | 否 | 启动参数 | `src/config/manifest.rs:47` |
| `env` | `BTreeMap<String, String>` | 否 | 注入的环境变量 | `src/config/manifest.rs:50` |

#### 约束

- `env` 带 `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`：缺字段按空表处理，为空则该键不出现（`src/config/manifest.rs:49`）。🟢
- `args` 必填且不设默认值（`src/config/manifest.rs:47`）。🟢

#### 生命周期

由 `codegraph_mcp` / `context7_mcp` 工厂产出，或在清单里手写；随 Agent 一起落盘。

#### 关系

被 Agent 的 `mcp` 列表内嵌引用；渲染时写成 `<agent-dir>/mcp.json`（`src/config/manifest.rs:40`）。🟢

## 包清单常量

### `BASE_PACKAGES`

必装四件套（用户拍板 2026-10-16），所有 Agent 必装，元素为 pi 的包标识字符串（`src/config/manifest.rs:14`）：

- npm:@ff-labs/pi-fff
- npm:@juicesharp/rpiv-web-tools
- `npm:pi-blackhole`
- git:github.com/NVlabs/SoL-Pi

清单的 `base_packages` 字段就是它的字符串化副本（`src/config/manifest.rs:501`）。🟢

### `PLAN_AGENT_EXTRA`

单包字符串：npm:@juicesharp/rpiv-ask-user-question（`src/config/manifest.rs:22`）。只有 Plan Agent 用它；注释写明理由是完成确认机制依赖 ask-user-question。🟢

### `CODING_AGENT_EXTRA`

3 个包的切片（`src/config/manifest.rs:25`），被 coding-agent、doc-fix、doc-agent 三个 Agent 共用（`src/config/manifest.rs:557`、`src/config/manifest.rs:607`、`src/config/manifest.rs:622`）。🟢

```text
npm:@juicesharp/rpiv-ask-user-question
npm:@juicesharp/rpiv-todo
npm:pi-lens
```

### `CODING_REVIEW_EXTRA`

1 个包的切片：`npm:pi-lens`（`src/config/manifest.rs:32`），只给 coding-review（`src/config/manifest.rs:572`）。🟢

### `TEST_AGENT_EXTRA`

2 个包的切片（`src/config/manifest.rs:35`），只给 test-agent（`src/config/manifest.rs:588`）。🟢

```text
npm:@juicesharp/rpiv-todo
npm:@juicesharp/rpiv-ask-user-question
```

## MCP 工厂函数

### `codegraph_mcp`

#### 摘要

返回 CodeGraph 事实层的 MCP 定义（`src/config/manifest.rs:54`）。

#### 行为语义

返回固定值：name `codegraph`、command `codegraph`、args `serve` 与 `--mcp`、env `CODEGRAPH_TELEMETRY=0`（`src/config/manifest.rs:55`）。🟢 无参数、无副作用、可重复调用。🟢

#### 参数

N/A（无参数）。

#### 返回

`McpServer`——上面那组固定值。

#### 异常

N/A（不返回 `Result`，不 panic）。

#### 示例

```rust
let server = codegraph_mcp();
assert_eq!(server.name, "codegraph");
```

### `context7_mcp`

#### 摘要

返回外部库文档 MCP 的定义（`src/config/manifest.rs:64`）。

#### 行为语义

返回固定值：name `context7`、command `npx`、args `-y` 与 `@upstash/context7-mcp`、env 空表（`src/config/manifest.rs:65`）。🟢 无参数、无副作用。🟢

#### 参数

N/A（无参数）。

#### 返回

`McpServer`——上面那组固定值。

#### 异常

N/A（不返回 `Result`，不 panic）。

#### 示例

```rust
let server = context7_mcp();
assert_eq!(server.command, "npx");
```

## 提示词常量

7 个 `&str` 常量，是各 Agent 提示词的唯一定义处；渲染层把它们写进 profile 的 `<profile>/AGENTS.md`。各常量的小节标题（便于第三方知道「这段提示词管什么」，不整段复述正文）：

| 常量 | 行 | 首行标题 | 小节标题（顺序） |
|---|---|---|---|
| `CODING_AGENT_PROMPT` | `src/config/manifest.rs:74` | Coding Agent（编码 Agent） | 开工（顺序强制）/ 开发纪律（强制）/ 工具纪律 / plan 偏差协议（发现 plan 有问题：停下，不擅自绕）/ 提交前预审（强制；不过关不发 stage-complete）/ 阶段完成协议（强制，逐字遵守）/ 工作方式 |
| `CODING_REVIEW_PROMPT` | `src/config/manifest.rs:135` | Coding Review Agent（编码审查 Agent） | 审查输入 / 审查面（逐项过，不许跳）/ 工具纪律 / 输出协议（强制，逐字遵守） |
| `TEST_AGENT_PROMPT` | `src/config/manifest.rs:178` | Test Agent（测试 Agent） | 开工（顺序强制）/ 黑盒纪律（强制，违者审查打回）/ 测试写作纪律 / 测试自身的可靠性（自指 / 超时 / 清理 / 假绿）——违反任一 = 审查直接打回 / 发现协议（测试与文档不符时）/ 追溯矩阵（维护义务）/ 阶段完成协议（强制，逐字遵守）/ 工作方式 |
| `TEST_REVIEW_PROMPT` | `src/config/manifest.rs:241` | Test Review Agent（测试审查 Agent） | 审查输入 / 审查面（逐项过，不许跳）/ 根因判定（FAIL 时必须标注）/ 审查纪律（省时硬纪律，与编码审查同口径）/ 输出协议（强制，逐字遵守） |
| `DOC_FIX_PROMPT` | `src/config/manifest.rs:288` | Doc Fix Agent（文档修补 Agent） | 开工 / 文档纪律 / 工作方式 |
| `DOC_AGENT_PROMPT` | `src/config/manifest.rs:312` | Doc Agent（文档 Agent） | 开工（顺序强制）/ 纪律（违者审查直接打回）/ 阶段流水（事实 / 推断分离，D006/D010）/ 交审前自检（缺证据不发完成）/ 阶段完成协议（强制，逐字遵守）/ 工作方式 |
| `DOC_REVIEW_PROMPT` | `src/config/manifest.rs:370` | Doc Review Agent（文档审查 Agent） | 审查输入 / 审查面（逐项过，不许跳）/ 输出协议（强制，逐字遵守） |

- 提示词正文的**存在性**由单测逐串断言守护（如 `DOC_AGENT_PROMPT` 必须含 codegraph-first、path:line、gaps.md、`<doc-complete>unit-xxx</doc-complete>`、`devforge doc-lint` 等 15 个关键串，`src/config/manifest.rs:773`）。🟢
- plan-agent 与 plan-review 的提示词不是模块级常量，而是 `builtin()` 内的局部字面量（`src/config/manifest.rs:466`、`src/config/manifest.rs:487`）。🟢
- 🟡 提示词按「定稿即定死」维护，改动应先改 plan tree；依据是代码注释「勿手改——改决策先改 plan tree」（`src/config/manifest.rs:73`）与仓库内决策记录 `docs/plantree/plans/002-config-management/decisions/README.md`。

## 方法

### `builtin`

#### 摘要

造出内置默认清单（`src/config/manifest.rs:465`）。

#### 行为语义

- 纯构造，不读文件、不发请求；每次调用得到内容相同的新清单（`ConfigManifest` 派生 `Clone`，`src/config/manifest.rs:449`）。🟢
- 内容固定：4 个必装包、1 个 provider、3 个模型池条目、9 个 Agent、`api_key` 为 `None`（`src/config/manifest.rs:645`）。🟢
- Agent 顺序固定：plan-agent、plan-review、coding-agent、coding-review、test-agent、test-review、doc-fix、doc-agent、doc-review（`src/config/manifest.rs:533`）。🟢

#### 参数

N/A（无参数）。

#### 返回

`ConfigManifest`——内置默认清单。

#### 异常

N/A（不返回 `Result`，不 panic）。

#### 示例

```rust
let manifest = ConfigManifest::builtin();
assert_eq!(manifest.agents.len(), 9);
```

### `resolve_model`

#### 摘要

按 Agent 引用的模型 id 在模型池里找条目（`src/config/manifest.rs:650`）。

#### 行为语义

- 线性扫描 `model_pool`，比较池条目的 `id` 是否等于 Agent 的 `model` 字段；取首个匹配（`src/config/manifest.rs:651`）。🟢
- 找不到即返回错误，不 panic、不返回默认值（`src/config/manifest.rs:654`）。🟢

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `agent` | `&AgentManifest` | 只需其 `model` 与 `name` 字段 | 无默认值，必填 |

#### 返回

`Result<&ModelEntry>`——命中的池条目引用（生命周期借自 `self`）；未命中为 `Err`。

#### 异常

`DevforgeError` 的 `Config` 变体，消息形如「Agent `x` 引用的模型 `y` 不在模型池中」（`src/config/manifest.rs:655`）。🟢

#### 示例

```rust
let manifest = ConfigManifest::builtin();
let agent = manifest.get_agent("plan-agent")?;
let model = manifest.resolve_model(agent)?;
```

### `resolve_provider`

#### 摘要

由 Agent 找到其 provider 定义（`src/config/manifest.rs:663`）。

#### 行为语义

- 先调 `resolve_model`，再按池条目的 `provider` 字段在 `providers` 里找同名项（`src/config/manifest.rs:664`）。🟢
- 两级都可能失败：模型缺失的报错来自 `resolve_model`；provider 缺失报「模型 `x` 引用的 provider `y` 无定义」（`src/config/manifest.rs:666`）。🟢

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `agent` | `&AgentManifest` | 需能解析到模型 | 无默认值，必填 |

#### 返回

`Result<&ProviderDef>`——命中的 provider 定义；未命中为 `Err`。

#### 异常

`DevforgeError` 的 `Config` 变体（`src/config/manifest.rs:666`）。🟢

#### 示例

```rust
let manifest = ConfigManifest::builtin();
let agent = manifest.get_agent("plan-agent")?;
let provider = manifest.resolve_provider(agent)?;
```

### `get_agent`

#### 摘要

按名取一个 Agent（`src/config/manifest.rs:673`）。

#### 行为语义

- 线性扫描 `agents`，比较 `name`；取首个匹配（`src/config/manifest.rs:674`）。🟢
- 找不到即报错，不 panic（`src/config/manifest.rs:677`）。🟢

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `name` | `&str` | Agent 名，须与 `agents[].name` 完全一致 | 无默认值，必填 |

#### 返回

`Result<&AgentManifest>`——命中的 Agent 引用；未命中为 `Err`。

#### 异常

`DevforgeError` 的 `Config` 变体，消息「清单中无 Agent `x`」（`src/config/manifest.rs:677`）。🟢

#### 示例

```rust
let manifest = ConfigManifest::builtin();
let agent = manifest.get_agent("coding-agent")?;
```

### `save`

#### 摘要

把清单序列化成 pretty JSON 落盘（`src/config/manifest.rs:680`）。

#### 行为语义

- 序列化用 serde_json 的 pretty 格式输出；序列化失败（理论上不会发生，字段都是自描述类型）会被 `ctx("序列化配置清单")` 包装（`src/config/manifest.rs:681`）。🟢
- 父目录不存在时先 `create_dir_all`（`src/config/manifest.rs:682`）。🟢
- 写文件用标准库写文件接口——整份覆盖，不做合并、不做备份（`src/config/manifest.rs:685`）。🟢
- 幂等性：同一对象反复 `save` 结果相同（序列化输出确定，`src/config/manifest.rs:681`）。🟢

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `path` | `&Path` | 目标文件路径；父目录会被自动创建 | 无默认值，必填 |

#### 返回

`Result<()>`——成功为 `Ok(())`。

#### 异常

`DevforgeError`：IO 失败经 `ctx("创建清单目录")` / `ctx("写入清单 <path>")` 包装；JSON 失败经 `ctx("序列化配置清单")` 包装（`src/config/manifest.rs:683`）。🟢

#### 示例

```rust
manifest.save(Path::new("/tmp/demo/manifest.json"))?;
```

### `load`

#### 摘要

从 JSON 文件读回清单（`src/config/manifest.rs:688`）。

#### 行为语义

- 读文件 → serde_json 反序列化；不做版本迁移、不做默认值修补（`src/config/manifest.rs:689`）。🟢
- 与 `save` 互为逆操作：`save` 后 `load` 得到的对象与原对象相等（单测 `manifest_json_roundtrip`，`src/config/manifest.rs:880`）。🟢

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `path` | `&Path` | 清单文件路径 | 无默认值，必填 |

#### 返回

`Result<Self>`——解析出的清单。

#### 异常

`DevforgeError`：文件不存在/读失败经 `ctx("读取清单 <path>")` 包装；JSON 不合法或字段不匹配经 `ctx("解析配置清单")` 包装（`src/config/manifest.rs:690`）。🟢

#### 示例

```rust
let manifest = ConfigManifest::load(Path::new("/home/u/.devforge/manifest.json"))?;
```

## 内置清单快照（种子数据）

`builtin()` 产出的固定内容（`src/config/manifest.rs:465`）。这是「零配置起步」的全部输入：profile 目录、装包清单、模型分配、提示词都从这里出。

**provider**（1 条，`src/config/manifest.rs:503`）：name `newapi`、api `openai-completions`、base_url https://newapi.trtyr.top/v1。

**模型池**（3 条，`src/config/manifest.rs:510`）：

| id | provider | model_id | context_window | max_tokens |
|---|---|---|---|---|
| `grok-4.7` | `newapi` | mokeywork/grok-4.7 | 1_000_000 | 65_536 |
| `deepseek-flash` | `newapi` | mokeywork/deepseek-flash | 1_000_000 | 65_536 |
| `minimax-m3.1-flash` | `newapi` | MiniMax-M3.1-Flash | 1_000_000 | 65_536 |

**Agent**（9 条，`src/config/manifest.rs:533`）：

| name | model | extra_packages | skills | mcp | thinking_level | 行 |
|---|---|---|---|---|---|---|
| `plan-agent` | `deepseek-flash` | PLAN_AGENT_EXTRA（1） | plan-tree | — | — | `src/config/manifest.rs:535` |
| `plan-review` | `grok-4.7` | — | plan-tree | — | medium | `src/config/manifest.rs:544` |
| `coding-agent` | `deepseek-flash` | CODING_AGENT_EXTRA（3） | doc-contract + 4 个语言 best-practices | codegraph, context7 | — | `src/config/manifest.rs:555` |
| `coding-review` | `grok-4.7` | CODING_REVIEW_EXTRA（1） | doc-contract + 4 个语言 best-practices | codegraph, context7 | medium | `src/config/manifest.rs:570` |
| `test-agent` | `deepseek-flash` | TEST_AGENT_EXTRA（2） | python-best-practices | context7 | — | `src/config/manifest.rs:586` |
| `test-review` | `grok-4.7` | — | — | — | medium | `src/config/manifest.rs:595` |
| `doc-fix` | `deepseek-flash` | CODING_AGENT_EXTRA（3） | doc-contract + 4 个语言 best-practices | codegraph, context7 | — | `src/config/manifest.rs:605` |
| `doc-agent` | `minimax-m3.1-flash` | CODING_AGENT_EXTRA（3） | doc-contract + 4 个语言 best-practices | codegraph, context7 | — | `src/config/manifest.rs:620` |
| `doc-review` | `grok-4.7` | — | doc-contract | codegraph | medium | `src/config/manifest.rs:635` |

读表要点：

- 审查类 Agent（plan-review / coding-review / test-review / doc-review）固定用 `grok-4.7`、思考档位 `medium`、不装额外包（`src/config/manifest.rs:546`、`src/config/manifest.rs:547`）。🟢
- 干活方 Agent 思考档位为 `None`，即用全局默认 high（字段注释 `src/config/manifest.rs:441`）。🟢
- `doc-contract` 这一个 skill 被 coding-agent、coding-review、doc-fix、doc-review 四个 profile 共用（单测 `doc_contract_skill_is_shared_by_four_profiles`，`src/config/manifest.rs:760`）。🟢
- `doc-agent` 的模型自 2026-10-20 起是 `minimax-m3.1-flash`（`src/config/manifest.rs:623`）。🟢
- 🟡 「审查类 → Grok、干活方 → DeepSeek/MiniMax」是用户拍板后写死的终态配置，不是自动推导出来的（依据：字段注释与决策记录 `docs/plantree/plans/002-config-management/decisions/README.md`）。

## 行为规格

对应清单 `src/config/manifest.rs` 的 behavior 条目：

| 行为 | 断言 | 证据 |
|---|---|---|
| `builtin_has_nine_agents_and_four_base_packages` | base_packages 等于 `BASE_PACKAGES`；Agent 数 = 9；逐 Agent 校验 extra/mcp/skills/model/thinking_level | 🟢 `src/config/manifest.rs:699` |
| `doc_contract_skill_is_shared_by_four_profiles` | coding-agent / coding-review / doc-fix / doc-review 四个 profile 都带 doc-contract skill | 🟢 `src/config/manifest.rs:760` |
| `doc_agent_prompt_carries_protocol_and_guards` | DOC_AGENT_PROMPT 命中 15 个硬约束关键串 | 🟢 `src/config/manifest.rs:773` |
| `coding_agent_prompt_has_preflight_audit` | 编码/审查提示词命中「提交前预审」与 doc-lint 门禁关键串 | 🟢 `src/config/manifest.rs:793` |
| `testing_prompts_carry_protocol_and_discipline` | 测试/文档双 Agent 提示词命中黑盒禁令、reroute 协议、省时纪律、一票否决等关键串 | 🟢 `src/config/manifest.rs:809` |
| `model_resolution_from_pool` | plan-agent 解析到 provider `newapi` + model_id mokeywork/deepseek-flash；plan-review 解析到 mokeywork/grok-4.7 | 🟢 `src/config/manifest.rs:860` |
| `unknown_model_rejected` | 模型 id 不在池中 → `resolve_model` 报错，消息含「不在模型池中」 | 🟢 `src/config/manifest.rs:872` |
| `manifest_json_roundtrip` | `save` 后 `load` 与原对象相等 | 🟢 `src/config/manifest.rs:880` |

补充事实：

- HTTP PUT 清单在落盘前会逐 Agent 做模型与 provider 解析校验，任一失败即 400、不落盘（校验在 `src/server/handlers.rs:352`，配置类错误映射 400 在 `src/server/handlers.rs:54`）。🟢
- 清单落盘后，后台线程异步重渲染 profile（含装包，分钟级），完成/失败经 SSE 事件通知（类型名分别是 config.rendered 与 config.render_failed，`src/server/handlers.rs:365`）。🟢
- 首次启动若 `<home>/.devforge/manifest.json` 不存在，用 `builtin()` 生成并落盘（`src/main.rs:152`）。🟢

## 错误与失败模式

| 场景 | 触发条件 | 对外错误 | 证据 |
|---|---|---|---|
| Agent 名不存在 | `get_agent` 查不到 | `DevforgeError` 的 `Config` 变体：「清单中无 Agent `x`」 | 🟢 `src/config/manifest.rs:677` |
| 模型引用悬空 | Agent 的 `model` 不在池中 | `DevforgeError` 的 `Config` 变体：「Agent `x` 引用的模型 `y` 不在模型池中」 | 🟢 `src/config/manifest.rs:655` |
| provider 引用悬空 | 池条目的 `provider` 无定义 | `DevforgeError` 的 `Config` 变体：「模型 `x` 引用的 provider `y` 无定义」 | 🟢 `src/config/manifest.rs:666` |
| 清单文件读不到 | `load` 路径不存在/无权限 | `DevforgeError` 的 `Io` 变体，经 `ctx("读取清单 <path>")` 包装 | 🟢 `src/config/manifest.rs:689` |
| 清单文件不合法 | JSON 语法错/字段类型不符/缺必填字段 | `DevforgeError` 的 `Json` 变体，经 `ctx("解析配置清单")` 包装 | 🟢 `src/config/manifest.rs:690` |
| 落盘失败 | 目录无权限等 | `DevforgeError` 的 `Io` 变体，经 `ctx("写入清单 <path>")` 包装 | 🟢 `src/config/manifest.rs:685` |
| PUT 校验不过 | 客户端送来悬空引用 | HTTP 400（错误体由 API 层统一包装，形如 {"error": "..."}），清单不落盘 | 🟢 `src/server/handlers.rs:54` |

## 调用方与边界

- **上游**：`src/main.rs:153`（启动读清单）、`src/main.rs:155`（首次生成）、`src/server/handlers.rs:340`（GET /api/v1/config/manifest）、`src/server/handlers.rs:347`（PUT /api/v1/config/manifest）。
- **下游**：渲染层 `src/config/render.rs`——把清单摊平成 profile 目录（AGENTS.md、settings、models.json、mcp.json、skills、装包）。
- **边界**：本模块只描述「要什么」，不执行「怎么做」；渲染与装包各有归属模块。
- **与装包层的边界**：包名在本模块定义，实际安装由 `src/config/installer.rs` 执行。
- **与服务器单元的边界**：GET/PUT /api/v1/config/manifest 的请求/响应形状、错误码与鉴权由服务器单元负责（`src/server/handlers.rs:340`、`src/server/handlers.rs:347`），本模块只提供被调用的数据结构与解析方法，不重复描述 HTTP 契约。

## 测试落点

- 模块内单测 8 个：`src/config/manifest.rs:695` 起的测试模块（清单见「行为规格」表）。
- 渲染层用内置清单做端到端验证：`src/config/render.rs:411`（并发渲染保持清单顺序）、`src/config/render.rs:665`（9 个 Agent 全覆盖）。
- HTTP 层端到端：`tests/e2e/test_03_config.py`（GET/PUT 清单、悬空模型返回 400、清单落盘校验）。
- 可重放命令：`cargo test --lib config::`（本机未执行，原因见下方缺口登记）。
- 🔴 本单元单测未在本机执行：仓库是 Rust 工程（`Cargo.toml` 存在），但本机 `rustup toolchain list` 输出为 `no installed toolchains`，无法编译运行；上述断言均为源码与单测代码的静态事实。
