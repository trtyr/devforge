# unit-src-01 · 勘察事实

本文件只记「有什么」（结构、控制流、外部依赖、公开输出），不含任何「为什么」。
解释与置信度见 `../confidence.md`，缺口见 `../gaps.md`。

## 范围

清单（`_meta/inventory.json`）里属于 `unit-src-01` 的条目共 44 条：module 2 / symbol 29 / behavior 13，落在两个文件：

| 文件 | 行数 | module | symbol | behavior |
|---|---|---|---|---|
| `src/config/installer.rs` | 198 | 1 | 4 | 5 |
| `src/config/manifest.rs` | 889 | 1 | 25 | 8 |

依赖方向（codegraph 探查结果）：

- `src/config/render.rs` 引用 `src/config/installer.rs`（`PackageInstaller`）与 `src/config/manifest.rs`（`ConfigManifest`）。
- `src/server/mod.rs` 引用二者（`PackageInstaller`、`ConfigManifest`）。
- `src/server/handlers.rs` 引用 `src/config/manifest.rs`（`ConfigManifest`、`resolve_model`、`resolve_provider`）。
- `src/main.rs` 引用二者（`ConfigManifest::load`、`ConfigManifest::builtin`、`ConfigManifest::save`、`PiInstallBridge`）。

## src/config/installer.rs

### 文件级

- 模块头注释（`src/config/installer.rs:1`）：「包安装桥接：pi install 一把梭（用户拍板 2026-10-16）」。
- 模块头注释（`src/config/installer.rs:3`）：pi install 自动完成双记录（settings.json `packages` 数组 + npm/package.json dependencies）；桥接只在 profile 的配置目录语境下调它；单测用 `FakeInstaller` 模拟双记录，不碰网络。
- import：`std::path::Path`（`src/config/installer.rs:6`）、`std::process::Command`（`src/config/installer.rs:7`）、`crate::error::{Context as _, DevforgeError, Result}`（`src/config/installer.rs:9`）。
- 无 `pub use` 再导出；本文件符号经 `src/config/mod.rs:11` 再导出 `build_install_command`、`PackageInstaller`、`PiInstallBridge`（`FakeInstaller` 未在 `mod.rs` 再导出，测试经 `crate::config::installer::FakeInstaller` 路径引用，见 `src/config/render.rs:301`）。

### 公开符号

| 符号 | 种类 | 行 | 签名 / 定义要点 |
|---|---|---|---|
| `build_install_command` | 公开函数 | `src/config/installer.rs:12` | `fn build_install_command(profile_dir: &Path, package: &str) -> Command` |
| `PackageInstaller` | 公开 trait | `src/config/installer.rs:47` | `pub trait PackageInstaller: Send + Sync`；唯一方法 `fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>`（`src/config/installer.rs:48`） |
| `PiInstallBridge` | 公开结构体 | `src/config/installer.rs:52` | 零字段单元结构体；`impl PackageInstaller for PiInstallBridge` 起于 `src/config/installer.rs:54` |
| `FakeInstaller` | 公开结构体 | `src/config/installer.rs:71` | `#[derive(Default)]` 零字段单元结构体；`impl PackageInstaller for FakeInstaller` 起于 `src/config/installer.rs:73` |

### 内部符号

- `install_failure_error`（私有函数，`src/config/installer.rs:23`）：入参 `pkg: &str`、`output: &std::process::Output`，返回 `DevforgeError`。

### 控制流

`build_install_command`（`src/config/installer.rs:12`）：

1. `Command::new("pi")`（`src/config/installer.rs:13`）。
2. `.env("PI_CODING_AGENT_DIR", profile_dir)`（`src/config/installer.rs:14`）。
3. `.arg("install").arg(package)`（`src/config/installer.rs:15`、`src/config/installer.rs:16`）。
4. 返回 `Command`（`src/config/installer.rs:17`）——只构造不执行。

`PiInstallBridge::install`（`src/config/installer.rs:55`）：

1. `for pkg in packages` 逐包（`src/config/installer.rs:56`）。
2. `build_install_command(profile_dir, pkg).output()`（`src/config/installer.rs:57`、`src/config/installer.rs:58`），`.ctx(format!("执行 pi install {pkg}"))?`（`src/config/installer.rs:59`）。
3. `if !output.status.success()` → `return Err(install_failure_error(pkg, &output))`（`src/config/installer.rs:60`、`src/config/installer.rs:61`）。
4. 全部成功 → `Ok(())`（`src/config/installer.rs:64`）。

`install_failure_error`（`src/config/installer.rs:23`）：

1. `const MAX_STDERR_BYTES: usize = 500`（`src/config/installer.rs:24`）。
2. `String::from_utf8_lossy(&output.stderr)` + `.trim()`（`src/config/installer.rs:25`、`src/config/installer.rs:26`）。
3. `trimmed.len() > 500` → 起点 `trimmed.len() - 500`，`while` 推进到 UTF-8 字符边界，输出 `"…" + &trimmed[start..]`（`src/config/installer.rs:27`—`src/config/installer.rs:32`）。
4. `trimmed.is_empty()` → `"（stderr 为空）"`（`src/config/installer.rs:33`、`src/config/installer.rs:34`）。
5. 否则 → `trimmed.to_string()`（`src/config/installer.rs:35`、`src/config/installer.rs:36`）。
6. 构造 `DevforgeError::Config(format!("pi install {pkg} 失败（exit: {}）\nstderr 尾部: {tail}", output.status))`（`src/config/installer.rs:38`—`src/config/installer.rs:42`）。

`FakeInstaller::install`（`src/config/installer.rs:74`）：

1. 记录 1：`settings_path = profile_dir.join("settings.json")`（`src/config/installer.rs:76`）。
2. 存在则读并 `serde_json::from_str` 解析，解析失败 `unwrap_or(serde_json::json!({}))` 退化为空对象；不存在则用空对象（`src/config/installer.rs:77`—`src/config/installer.rs:82`）。
3. `as_object_mut().expect("settings 应为对象")`（`src/config/installer.rs:84`、`src/config/installer.rs:85`）→ `.entry("packages").or_insert_with(|| serde_json::json!([]))`（`src/config/installer.rs:86`、`src/config/installer.rs:87`）。
4. 逐包 `push` 进 `packages` 数组（`src/config/installer.rs:88`、`src/config/installer.rs:89`）。
5. `std::fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap())`，`.ctx("写 settings")?`（`src/config/installer.rs:91`、`src/config/installer.rs:92`）。
6. 记录 2：`npm_dir = profile_dir.join("npm")`，`std::fs::create_dir_all(&npm_dir).ctx("建 npm 目录")?`（`src/config/installer.rs:95`、`src/config/installer.rs:96`）。
7. `pkg_json_path = npm_dir.join("package.json")`（`src/config/installer.rs:97`）；存在则读解析（失败退化空对象），不存在则用 `{"name": "pi-extensions", "private": true, "dependencies": {}}`（`src/config/installer.rs:98`—`src/config/installer.rs:103`）。
8. `as_object_mut().expect("package.json 应为对象")`（`src/config/installer.rs:106`）→ `.entry("dependencies").or_insert_with(...)`（`src/config/installer.rs:107`、`src/config/installer.rs:108`）。
9. 逐包 `insert(pkg.clone(), serde_json::json!("*"))`（`src/config/installer.rs:109`—`src/config/installer.rs:112`）。
10. 写 `package.json`，`.ctx("写 package.json")?`（`src/config/installer.rs:114`、`src/config/installer.rs:115`）；`Ok(())`（`src/config/installer.rs:116`）。

### 单测（`#[cfg(test)] mod tests`，`src/config/installer.rs:120`）

| 单测名 | 行 | 断言要点 |
|---|---|---|
| `install_command_carries_profile_env` | `src/config/installer.rs:125` | 程序名 `pi`；环境变量 `PI_CODING_AGENT_DIR` 等于传入目录；参数序列为 `install`、包名 |
| `fake_installer_writes_dual_records` | `src/config/installer.rs:135` | 临时目录下 `settings.json` 的 `packages` 数组长度 2；`npm/package.json` 的 `dependencies` 含两个包名 |
| `failure_error_carries_stderr_tail` | `src/config/installer.rs:174` | 错误串含 `pi install npm:pi-lens 失败` 与 stderr 原文片段 |
| `failure_error_truncates_to_tail` | `src/config/installer.rs:183` | 2000 字符 stderr + 末行中文，错误串含末行内容且总长 < 800 |
| `failure_error_handles_empty_stderr` | `src/config/installer.rs:193` | 空 stderr 时错误串含 `stderr 为空` |

辅助函数 `output_with_stderr`（`src/config/installer.rs:161`）构造带指定 stderr 与退出码的 `std::process::Output`（unix/windows 分别用 `ExitStatusExt::from_raw`）。

## src/config/manifest.rs

### 文件级

- 模块头注释（`src/config/manifest.rs:1`）：「配置清单 schema（P002 · 配置即代码的真源）」。
- 模块头注释（`src/config/manifest.rs:3`—`src/config/manifest.rs:5`）：两层——模板层（写死：role_prompt、必装四件套、skills、MCP/Key 内联于 schema）与模型层（可变：模型池 + Agent 引用）。
- import：`std::path::Path`（`src/config/manifest.rs:7`）、`serde::{Deserialize, Serialize}`（`src/config/manifest.rs:9`）、`crate::error::{Context as _, DevforgeError, Result}`（`src/config/manifest.rs:11`）。

### 常量

| 常量 | 行 | 内容 |
|---|---|---|
| `BASE_PACKAGES` | `src/config/manifest.rs:14` | 4 项切片：`npm:@ff-labs/pi-fff`、`npm:@juicesharp/rpiv-web-tools`、`npm:pi-blackhole`、`git:github.com/NVlabs/SoL-Pi` |
| `PLAN_AGENT_EXTRA` | `src/config/manifest.rs:22` | 单个 `&str`：`npm:@juicesharp/rpiv-ask-user-question` |
| `CODING_AGENT_EXTRA` | `src/config/manifest.rs:25` | 3 项切片：`npm:@juicesharp/rpiv-ask-user-question`、`npm:@juicesharp/rpiv-todo`、`npm:pi-lens` |
| `CODING_REVIEW_EXTRA` | `src/config/manifest.rs:32` | 1 项切片：`npm:pi-lens` |
| `TEST_AGENT_EXTRA` | `src/config/manifest.rs:35` | 2 项切片：`npm:@juicesharp/rpiv-todo`、`npm:@juicesharp/rpiv-ask-user-question` |

### 提示词常量

| 常量 | 行 | 形态 |
|---|---|---|
| `CODING_AGENT_PROMPT` | `src/config/manifest.rs:74` | `&str`，以 `\n\` 续行拼装，止于 `src/config/manifest.rs:132` |
| `CODING_REVIEW_PROMPT` | `src/config/manifest.rs:135` | `&str`，同上，止于 `src/config/manifest.rs:175` |
| `TEST_AGENT_PROMPT` | `src/config/manifest.rs:178` | `&str` 原始字符串 `r#"…"#`，止于 `src/config/manifest.rs:238` |
| `TEST_REVIEW_PROMPT` | `src/config/manifest.rs:241` | `&str` 原始字符串，止于 `src/config/manifest.rs:285` |
| `DOC_FIX_PROMPT` | `src/config/manifest.rs:288` | `&str` 原始字符串，止于 `src/config/manifest.rs:309` |
| `DOC_AGENT_PROMPT` | `src/config/manifest.rs:312` | `&str` 原始字符串，止于 `src/config/manifest.rs:368` |
| `DOC_REVIEW_PROMPT` | `src/config/manifest.rs:371` | `&str` 原始字符串，止于 `src/config/manifest.rs:397` |

各提示词正文的章节构成（逐条读出）：

- `CODING_AGENT_PROMPT`：开工（顺序强制，4 步）、开发纪律（伴写文档 / 只写单测 / git 纪律 / 刚需架构条款）、工具纪律（codegraph-first / pi-lens 诊断 / context7-web / todo 纪律）、plan 偏差协议、提交前预审（强制；4 条）、阶段完成协议、工作方式。
- `CODING_REVIEW_PROMPT`：审查输入、审查面（8 项）、工具纪律、输出协议（`stage-verdict` / `next`）。
- `TEST_AGENT_PROMPT`：开工（3 步）、黑盒纪律（白名单 / 绝对禁令）、测试写作纪律、测试自身的可靠性（7 条）、发现协议、追溯矩阵、阶段完成协议、工作方式。
- `TEST_REVIEW_PROMPT`：审查输入、审查面（7 项）、根因判定（test-side / doc-side）、审查纪律、输出协议。
- `DOC_FIX_PROMPT`：开工（4 步）、文档纪律、工作方式。
- `DOC_AGENT_PROMPT`：开工（3 步）、纪律（9 条硬约束）、阶段流水（3 阶段）、交审前自检（5 条）、阶段完成协议、工作方式。
- `DOC_REVIEW_PROMPT`：审查输入、审查面（5 项）、输出协议。

### 数据结构

| 结构 | 行 | 字段（行） |
|---|---|---|
| `McpServer` | `src/config/manifest.rs:42` | `name: String`（44）、`command: String`（46）、`args: Vec<String>`（47）、`env: BTreeMap<String, String>`（50，带 `#[serde(default, skip_serializing_if = ...is_empty)]`，49） |
| `ModelEntry` | `src/config/manifest.rs:401` | `id: String`（403）、`provider: String`（405）、`model_id: String`（407）、`context_window: u64`（409）、`max_tokens: u64`（411） |
| `ProviderDef` | `src/config/manifest.rs:416` | `name: String`（418）、`api: String`（420）、`base_url: String`（422） |
| `AgentManifest` | `src/config/manifest.rs:427` | `name: String`（429）、`role_prompt: String`（431）、`extra_packages: Vec<String>`（433）、`model: String`（435）、`skills: Vec<String>`（437）、`mcp: Vec<McpServer>`（440，`skip_serializing_if = "Vec::is_empty"`）、`thinking_level: Option<String>`（445，`skip_serializing_if = "Option::is_none"`） |
| `ConfigManifest` | `src/config/manifest.rs:450` | `base_packages: Vec<String>`（452）、`providers: Vec<ProviderDef>`（454）、`model_pool: Vec<ModelEntry>`（456）、`agents: Vec<AgentManifest>`（458）、`api_key: Option<String>`（460） |

五个结构体均派生 `Debug, Clone, PartialEq, Serialize, Deserialize`（`McpServer` 见 `src/config/manifest.rs:41`，`ModelEntry` 见 `src/config/manifest.rs:400`，其余同理）。

### MCP 工厂函数

- `codegraph_mcp()`（`src/config/manifest.rs:54`）：`name = "codegraph"`、`command = "codegraph"`、`args = ["serve", "--mcp"]`、`env = {"CODEGRAPH_TELEMETRY": "0"}`。
- `context7_mcp()`（`src/config/manifest.rs:64`）：`name = "context7"`、`command = "npx"`、`args = ["-y", "@upstash/context7-mcp"]`、`env` 为空。

### `ConfigManifest` 方法

| 方法 | 行 | 行为 |
|---|---|---|
| `builtin` | `src/config/manifest.rs:466` | 构造内置清单：`base_packages` 取 `BASE_PACKAGES` 逐项转 `String`（`src/config/manifest.rs:502`）；`providers` 1 条 `newapi`（504—507，`api = "openai-completions"`，`base_url = "https://newapi.trtyr.top/v1"`）；`model_pool` 3 条（`grok-4.7` 513、`deepseek-flash` 520、`minimax-m3.1-flash` 527，均 `provider = "newapi"`，`context_window = 1_000_000`（516/523/530），`max_tokens = 65_536`（517/524/531））；`agents` 9 条（536/545/556/571/587/596/606/621/636）；`api_key = None`（646） |
| `resolve_model` | `src/config/manifest.rs:651` | 在 `model_pool` 中按 `agent.model` 找 `id` 相同的条目；找不到 → `DevforgeError::Config("Agent \`{}\` 引用的模型 \`{}\` 不在模型池中")`（656—659） |
| `resolve_provider` | `src/config/manifest.rs:664` | 先 `resolve_model`，再在 `providers` 中找 `name == model.provider`；找不到 → `DevforgeError::Config("模型 \`{}\` 引用的 provider \`{}\` 无定义")`（666—670） |
| `get_agent` | `src/config/manifest.rs:674` | 在 `agents` 中按名查找；找不到 → `DevforgeError::Config("清单中无 Agent \`{name}\`")`（678） |
| `save` | `src/config/manifest.rs:681` | `serde_json::to_string_pretty(self).ctx("序列化配置清单")?`；`path.parent()` 存在则 `create_dir_all`；`std::fs::write(path, json).ctx(format!("写入清单 {}", path.display()))` |
| `load` | `src/config/manifest.rs:689` | `std::fs::read_to_string(path).ctx(format!("读取清单 {}", path.display()))?`；`serde_json::from_str(&json).ctx("解析配置清单")` |

### `builtin()` 内 9 个 Agent 的取值

| Agent | name 行 | model（行） | extra_packages | skills | mcp | thinking_level |
|---|---|---|---|---|---|---|
| plan-agent | 536 | `deepseek-flash`（539） | `PLAN_AGENT_EXTRA` | `plan-tree` | 空 | `None` |
| plan-review | 545 | `grok-4.7`（548） | 空 | `plan-tree` | 空 | `Some("medium")` |
| coding-agent | 556 | `deepseek-flash`（559） | `CODING_AGENT_EXTRA` | doc-contract + 4 个语言 best-practices | `codegraph` + `context7` | `None` |
| coding-review | 571 | `grok-4.7`（574） | `CODING_REVIEW_EXTRA` | doc-contract + 4 个语言 best-practices | `codegraph` + `context7` | `Some("medium")` |
| test-agent | 587 | `deepseek-flash`（590） | `TEST_AGENT_EXTRA` | `python-best-practices` | `context7` | `None` |
| test-review | 596 | `grok-4.7`（599） | 空 | 空 | 空 | `Some("medium")` |
| doc-fix | 606 | `deepseek-flash`（609） | `CODING_AGENT_EXTRA` | doc-contract + 4 个语言 best-practices | `codegraph` + `context7` | `None` |
| doc-agent | 621 | `minimax-m3.1-flash`（624） | `CODING_AGENT_EXTRA` | doc-contract + 4 个语言 best-practices | `codegraph` + `context7` | `None` |
| doc-review | 636 | `grok-4.7`（639） | 空 | `doc-contract` | `codegraph` | `Some("medium")` |

`plan-agent` 与 `plan-review` 的 `role_prompt` 是 `builtin()` 内部局部变量 `plan_agent_prompt`（`src/config/manifest.rs:467`）与 `review_prompt`（`src/config/manifest.rs:484`）；其余 7 个 Agent 的 `role_prompt` 直接取对应提示词常量。

### 单测（`#[cfg(test)] mod tests`，`src/config/manifest.rs:695`）

| 单测名 | 行 | 断言要点 |
|---|---|---|
| `builtin_has_nine_agents_and_four_base_packages` | `src/config/manifest.rs:700` | `base_packages == BASE_PACKAGES` 映射结果；`agents.len() == 9`；plan-agent 有 `PLAN_AGENT_EXTRA`、plan-review 无差异化包；coding-agent 的 mcp 为 codegraph+context7 且 `CODEGRAPH_TELEMETRY == "0"`、extra 3 个、model `deepseek-flash`；coding-review extra 为 `npm:pi-lens`、model `grok-4.7`；规划双 Agent 无 mcp；test-agent 仅 context7、skills 为 `python-best-practices`、model `deepseek-flash`；test-review 三项皆空、model `grok-4.7`；doc-fix extra 等于 coding-agent、mcp 2 个、model `deepseek-flash`；doc-review extra 空、skills 仅 `doc-contract`、mcp 仅 codegraph、model `grok-4.7`；doc-agent model `minimax-m3.1-flash`、extra 同 coding-agent、mcp 2 个、skills 含 doc-contract、thinking_level 为 `None` |
| `doc_contract_skill_is_shared_by_four_profiles` | `src/config/manifest.rs:761` | `coding-agent`、`coding-review`、`doc-fix`、`doc-review` 四者的 skills 均含 `doc-contract` |
| `doc_agent_prompt_carries_protocol_and_guards` | `src/config/manifest.rs:774` | `DOC_AGENT_PROMPT` 含 16 个关键串：`只写 \`docs/contexts/\``、`codegraph-first`、`事实与推断分离`、`🟢`、`path:line`、`gaps.md`、`N/A`、`<doc-complete>unit-xxx</doc-complete>`、`devforge doc-lint`、`按 route 回去修`、`阶段流水（事实 / 推断分离）`、`_meta/facts/<unit>.md`、`严禁写“为什么”`、`不得静默省略` |
| `coding_agent_prompt_has_preflight_audit` | `src/config/manifest.rs:794` | `CODING_AGENT_PROMPT` 含 `提交前预审`、`绝对词`、`断言必须能失败`、`预审证据`、`isVisible()`、`doc-contract`、`devforge doc-lint`、`退出码必须 0`；`CODING_REVIEW_PROMPT` 含 `doc-lint 复核`、`自己跑一遍` |
| `testing_prompts_carry_protocol_and_discipline` | `src/config/manifest.rs:810` | `TEST_AGENT_PROMPT` 含黑盒禁令与可靠性 7 条锚点；`TEST_REVIEW_PROMPT` 含 reroute 与纪律锚点；`CODING_REVIEW_PROMPT` 含省时纪律锚点；`DOC_FIX_PROMPT` / `DOC_REVIEW_PROMPT` 含文档双 Agent 锚点 |
| `model_resolution_from_pool` | `src/config/manifest.rs:861` | plan-agent 解析出 `provider = "newapi"`、`model_id = "mokeywork/deepseek-flash"`；plan-review 解析出 `mokeywork/grok-4.7` |
| `unknown_model_rejected` | `src/config/manifest.rs:873` | 把 `agents[0].model` 改成 `ghost-model` 后 `resolve_model` 报错，错误串含 `不在模型池中` |
| `manifest_json_roundtrip` | `src/config/manifest.rs:881` | `builtin()` → `save` 到临时文件 → `load` → 与原对象相等 |

## 外部依赖（两个文件合计）

- 同仓：`crate::error`（`DevforgeError`、`Result`、`Context` 扩展 `ctx`）。
- 外部 crate：`serde`（derive）、`serde_json`（读写 JSON）、`tempfile`（仅单测，dev-dependency）。
- 外部程序：`pi`（装包）、`codegraph`（MCP server 的 command）、`npx`（context7 MCP server 的 command）。
