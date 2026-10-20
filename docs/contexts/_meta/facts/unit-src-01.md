# unit-src-01 · 勘察事实（只记「有什么」，不含「为什么」）

- 单元：`unit-src-01`，档位 `complete`
- 目标仓：`/home/trtyr/code-work/devforge`（只读）
- 覆盖文件：`src/config/installer.rs`（198 行）、`src/config/manifest.rs`（888 行）
- 采集方式：codegraph `explore`（返回 verbatim 源码，视为已读）+ 定点 Read 补行号
- 本文件只登记「有什么」；解释与置信度见 `_meta/confidence.md` 与各模块文档正文

---

## 1. src/config/installer.rs

### 1.1 公开符号

| 符号 | 行 | 签名 / 形态 |
|---|---|---|
| `build_install_command` | 12 | `fn(profile_dir: &Path, package: &str) -> Command` |
| `PackageInstaller` | 47 | `trait`，超trait `Send + Sync`；方法 `install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>` |
| `PiInstallBridge` | 52 | `struct`（单元体，无字段）；`impl PackageInstaller` |
| `FakeInstaller` | 71 | `struct`（单元体，`#[derive(Default)]`）；`impl PackageInstaller` |

### 1.2 内部符号

| 符号 | 行 | 形态 |
|---|---|---|
| `install_failure_error` | 23 | `fn(pkg: &str, output: &std::process::Output) -> DevforgeError`（未 `pub`） |

### 1.3 `use` 依赖

- `std::path::Path`（installer.rs:6）、`std::process::Command`（installer.rs:7）
- `crate::error::{Context as _, DevforgeError, Result}`（installer.rs:9）
- `serde_json`（`Value` / `json!` / `to_string_pretty` / `from_str`）
- 测试依赖：`tempfile`（installer.rs:136）

### 1.4 控制流

- `build_install_command`：`Command::new("pi")` → `env("PI_CODING_AGENT_DIR", profile_dir)` → `arg("install")` → `arg(package)`（installer.rs:12-18）
- `install_failure_error`：`String::from_utf8_lossy(&output.stderr)` → `trim()`；长度 > `MAX_STDERR_BYTES`（=500，installer.rs:24）时取尾部并回退到 UTF-8 字符边界，前缀 `…`；空串时用 `（stderr 为空）`；否则整段（installer.rs:25-37）。错误类型 `DevforgeError::Config`，消息含 `pi install {pkg} 失败（exit: {status}）` 与 `stderr 尾部: {tail}`（installer.rs:38-41）
- `PiInstallBridge::install`：对 `packages` 逐个 `build_install_command(...).output()`，IO 失败经 `.ctx("执行 pi install {pkg}")` 包装；`!output.status.success()` → 返回 `install_failure_error`；全部成功 → `Ok(())`（installer.rs:55-65）
- `FakeInstaller::install`：
  - 记录 1：读 `<profile>/settings.json`（不存在或解析失败 → `json!({})`），`packages` 数组逐个 push，写回（installer.rs:76-92）
  - 记录 2：`<profile>/npm/` 目录 `create_dir_all`；读 `<profile>/npm/package.json`（不存在 → `{"name":"pi-extensions","private":true,"dependencies":{}}`），`dependencies` 对象逐个 `insert(pkg, "*")`，写回（installer.rs:95-115）
- `PackageInstaller` 的 `Send + Sync` 约束写在 trait 定义的文档注释里（installer.rs:46），注释给出的理由是并发渲染时四个 worker 共享 `&dyn PackageInstaller`

### 1.5 模块内测试（`mod tests`，installer.rs:120-198）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `install_command_carries_profile_env` | 125 | 程序名 = `pi`；`PI_CODING_AGENT_DIR` = 传入的 profile 目录；参数 = `install` + 包名 |
| `fake_installer_writes_dual_records` | 135 | `settings.json` 的 `packages` 数组长度 2；`npm/package.json` 的 `dependencies` 含两个包名 |
| `failure_error_carries_stderr_tail` | 174 | 错误含 `pi install npm:pi-lens 失败` 且含 stderr 原文片段 |
| `failure_error_truncates_to_tail` | 183 | 超长 stderr 只保尾部（断言 `err.len() < 800`），尾部中文完整 |
| `failure_error_handles_empty_stderr` | 193 | 空 stderr → 错误含 `stderr 为空` |

测试辅助：`output_with_stderr(stderr: &[u8], raw_exit: i32)`（installer.rs:161），unix/windows 分支构造 `ExitStatus`。

---

## 2. src/config/manifest.rs

### 2.1 常量

| 符号 | 行 | 值 / 形态 |
|---|---|---|
| `BASE_PACKAGES` | 14 | `&[&str]`，4 项：`npm:@ff-labs/pi-fff`、`npm:@juicesharp/rpiv-web-tools`、`npm:pi-blackhole`、`git:github.com/NVlabs/SoL-Pi` |
| `PLAN_AGENT_EXTRA` | 22 | `&str` = `npm:@juicesharp/rpiv-ask-user-question` |
| `CODING_AGENT_EXTRA` | 25 | `&[&str]`，3 项：ask-user-question、rpiv-todo、`npm:pi-lens` |
| `CODING_REVIEW_EXTRA` | 32 | `&[&str]`，1 项：`npm:pi-lens` |
| `TEST_AGENT_EXTRA` | 35 | `&[&str]`，2 项：rpiv-todo、ask-user-question |
| `CODING_AGENT_PROMPT` | 74 | `&str` 提示词（`\n\` 续行拼接） |
| `CODING_REVIEW_PROMPT` | 135 | `&str` 提示词 |
| `TEST_AGENT_PROMPT` | 178 | `&str` 提示词（raw string） |
| `TEST_REVIEW_PROMPT` | 241 | `&str` 提示词（raw string） |
| `DOC_FIX_PROMPT` | 288 | `&str` 提示词（raw string） |
| `DOC_AGENT_PROMPT` | 312 | `&str` 提示词（raw string） |
| `DOC_REVIEW_PROMPT` | 370 | `&str` 提示词（raw string） |

### 2.2 函数

| 符号 | 行 | 签名 |
|---|---|---|
| `codegraph_mcp` | 54 | `fn() -> McpServer` |
| `context7_mcp` | 64 | `fn() -> McpServer` |

### 2.3 数据结构

| 结构 | 行 | 字段 |
|---|---|---|
| `McpServer` | 42 | `name: String`、`command: String`、`args: Vec<String>`、`env: BTreeMap<String,String>`（`#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`） |
| `ModelEntry` | 400 | `id: String`、`provider: String`、`model_id: String`、`context_window: u64`、`max_tokens: u64` |
| `ProviderDef` | 415 | `name: String`、`api: String`、`base_url: String` |
| `AgentManifest` | 426 | `name: String`、`role_prompt: String`、`extra_packages: Vec<String>`、`model: String`、`skills: Vec<String>`、`mcp: Vec<McpServer>`（`skip_serializing_if = "Vec::is_empty"`）、`thinking_level: Option<String>`（`skip_serializing_if = "Option::is_none"`） |
| `ConfigManifest` | 449 | `base_packages: Vec<String>`、`providers: Vec<ProviderDef>`、`model_pool: Vec<ModelEntry>`、`agents: Vec<AgentManifest>`、`api_key: Option<String>` |

四个结构均 `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`。

### 2.4 `ConfigManifest` 方法

| 方法 | 行 | 签名 / 行为 |
|---|---|---|
| `builtin` | 465 | `fn() -> Self`：内置 providers(1) + model_pool(3) + agents(9) + `api_key: None` |
| `resolve_model` | 650 | `fn(&self, agent: &AgentManifest) -> Result<&ModelEntry>`：按 `agent.model` 找 `model_pool`，找不到 → `DevforgeError::Config("Agent \`{}\` 引用的模型 \`{}\` 不在模型池中")` |
| `resolve_provider` | 663 | `fn(&self, agent: &AgentManifest) -> Result<&ProviderDef>`：先 `resolve_model`，再按 `model.provider` 找 `providers`，找不到 → `DevforgeError::Config("模型 \`{}\` 引用的 provider \`{}\` 无定义")` |
| `get_agent` | 673 | `fn(&self, name: &str) -> Result<&AgentManifest>`：按名找 `agents`，找不到 → `DevforgeError::Config("清单中无 Agent \`{name}\`")` |
| `save` | 680 | `fn(&self, path: &Path) -> Result<()>`：`to_string_pretty` → 有父目录则 `create_dir_all` → `fs::write` |
| `load` | 688 | `fn(path: &Path) -> Result<Self>`：`fs::read_to_string` → `serde_json::from_str` |

### 2.5 `builtin()` 内的固定内容

- `providers`（manifest.rs:503-508）：1 条 —— name `newapi`、api `openai-completions`、base_url `https://newapi.trtyr.top/v1`
- `model_pool`（manifest.rs:510-532）：3 条
  - `grok-4.7` → provider `newapi`、model_id `mokeywork/grok-4.7`、context_window 1_000_000、max_tokens 65_536
  - `deepseek-flash` → provider `newapi`、model_id `mokeywork/deepseek-flash`、1_000_000 / 65_536
  - `minimax-m3.1-flash` → provider `newapi`、model_id `MiniMax-M3.1-Flash`、1_000_000 / 65_536
- `agents`（manifest.rs:533-644）：9 条，顺序 plan-agent、plan-review、coding-agent、coding-review、test-agent、test-review、doc-fix、doc-agent、doc-review
  - plan-agent（:535）：extra = PLAN_AGENT_EXTRA、model deepseek-flash、skills [plan-tree]、mcp []、thinking None
  - plan-review（:544）：extra []、model grok-4.7、skills [plan-tree]、mcp []、thinking medium
  - coding-agent（:555）：extra = CODING_AGENT_EXTRA、model deepseek-flash、skills 5 个（doc-contract / rust / go / python / ts-js best-practices）、mcp [codegraph, context7]、thinking None
  - coding-review（:570）：extra = CODING_REVIEW_EXTRA、model grok-4.7、skills 同上 5 个、mcp [codegraph, context7]、thinking medium
  - test-agent（:586）：extra = TEST_AGENT_EXTRA、model deepseek-flash、skills [python-best-practices]、mcp [context7]、thinking None
  - test-review（:595）：extra []、model grok-4.7、skills []、mcp []、thinking medium
  - doc-fix（:605）：extra = CODING_AGENT_EXTRA、model deepseek-flash、skills 5 个、mcp [codegraph, context7]、thinking None
  - doc-agent（:620）：extra = CODING_AGENT_EXTRA、model minimax-m3.1-flash、skills 5 个、mcp [codegraph, context7]、thinking None
  - doc-review（:635）：extra []、model grok-4.7、skills [doc-contract]、mcp [codegraph]、thinking medium
- plan-agent / plan-review 的 `role_prompt` 是 `builtin()` 内的局部字符串字面量（manifest.rs:466-496），不是模块级常量

### 2.6 MCP 定义细节

- `codegraph_mcp()`（manifest.rs:54-61）：name `codegraph`、command `codegraph`、args `serve` `--mcp`、env `CODEGRAPH_TELEMETRY=0`
- `context7_mcp()`（manifest.rs:64-71）：name `context7`、command `npx`、args `-y` `@upstash/context7-mcp`、env 空

### 2.7 提示词常量的小节标题（`awk` 抽取，供文档索引用）

- CODING_AGENT_PROMPT：开工（顺序强制）/ 开发纪律（强制）/ 工具纪律 / plan 偏差协议（发现 plan 有问题：停下，不擅自绕）/ 提交前预审（强制；不过关不发 stage-complete）/ 阶段完成协议（强制，逐字遵守）/ 工作方式
- CODING_REVIEW_PROMPT：审查输入 / 审查面（逐项过，不许跳）/ 根因判定（FAIL 时必须标注）/ 审查纪律（省时硬纪律，与编码审查同口径）/ 输出协议（强制，逐字遵守）
- TEST_AGENT_PROMPT：开工（顺序强制）/ 黑盒纪律（强制，违者审查打回）/ 测试写作纪律 / 测试自身的可靠性（自指 / 超时 / 清理 / 假绿）——违反任一 = 审查直接打回 / 发现协议（测试与文档不符时）/ 追溯矩阵（维护义务）/ 阶段完成协议（强制，逐字遵守）/ 工作方式
- TEST_REVIEW_PROMPT：审查输入 / 审查面（逐项过，不许跳）/ 阶段完成协议
- DOC_FIX_PROMPT：开工 / 文档纪律 / 工作方式
- DOC_AGENT_PROMPT：开工（顺序强制）/ 纪律（违者审查直接打回）/ 阶段流水（事实 / 推断分离，D006/D010）/ 阶段完成协议（强制，逐字遵守）/ 工作方式
- DOC_REVIEW_PROMPT：审查输入 / 审查面（逐项过，不许跳）/ 输出协议（强制，逐字遵守）

### 2.8 模块内测试（`mod tests`，manifest.rs:694-888）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `builtin_has_nine_agents_and_four_base_packages` | 699 | base_packages == BASE_PACKAGES；agents.len() == 9；逐 Agent 校验 extra/mcp/skills/model/thinking_level |
| `doc_contract_skill_is_shared_by_four_profiles` | 760 | coding-agent / coding-review / doc-fix / doc-review 四个 profile 都含 doc-contract skill |
| `doc_agent_prompt_carries_protocol_and_guards` | 773 | DOC_AGENT_PROMPT 命中 15 个关键串（含 `只写 docs/contexts/`、`codegraph-first`、`path:line`、`gaps.md`、`N/A`、`<doc-complete>unit-xxx</doc-complete>`、`devforge doc-lint` 等） |
| `coding_agent_prompt_has_preflight_audit` | 793 | CODING_AGENT_PROMPT / CODING_REVIEW_PROMPT 命中预审与 doc-lint 门禁关键串 |
| `testing_prompts_carry_protocol_and_discipline` | 809 | 测试/文档双 Agent 提示词命中黑盒禁令、reroute 协议、省时纪律、一票否决等关键串 |
| `model_resolution_from_pool` | 860 | plan-agent → provider `newapi`、model_id `mokeywork/deepseek-flash`；plan-review → `mokeywork/grok-4.7` |
| `unknown_model_rejected` | 872 | 把 agents[0].model 改成 `ghost-model` → `resolve_model` 报错且消息含 `不在模型池中` |
| `manifest_json_roundtrip` | 880 | `save` 后 `load` 与原对象相等 |

---

## 3. 本单元符号的仓外/跨模块引用面（只登记「谁提到它」）

- `src/config/mod.rs:11` 导出 `build_install_command`、`PackageInstaller`、`PiInstallBridge`；`:12` 导出 `AgentManifest`、`ConfigManifest`、`ModelEntry`、`ProviderDef`
- `src/config/render.rs:8` 引入 `PackageInstaller`；`src/config/render.rs:301` 引入 `FakeInstaller`（测试用）
- `src/server/handlers.rs:340` `get_manifest`、`:347` `put_manifest`（PUT 前逐 Agent 调 `resolve_model` / `resolve_provider` 校验）
- `src/main.rs` 启动时读 `<home>/.devforge/manifest.json`，不存在则用 `builtin()` 生成并落盘；启动时注入 `PiInstallBridge` 作为装包器
