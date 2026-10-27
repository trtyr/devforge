# 勘察事实 — unit-src-25（src/config/manifest.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/config/manifest.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。
> 复核基线：HEAD `470b283`；`src/config/manifest.rs` 最后一次变更是 `19b3717`（ARC-R4 批 2+3 补齐），`git log -1 -- src/config/manifest.rs` 实查确认 470b283 只动 docs/plantree。`wc -l` = **948 行**。上一版按 1159 行（`c60641a`）、再上一版按 1504 行记录，行号已整体作废，本表逐行重核。
> inventory 46 条（module ×1 + symbol ×31 + behavior ×14）与本表逐条对齐（行号全同）。

## 文件与模块位置

- 文件：`src/config/manifest.rs`，948 行，无子模块。
- 声明/转发：`src/config/mod.rs:7` `pub mod manifest;`；`:12` `pub use manifest::{AgentManifest, ConfigManifest, ModelEntry, ProviderDef};`（**只有 4 个类型转发**；包常量与提示词常量不经 re-export）。
- 文件头注释（`:1`-:5`）：配置即代码真源，两层——模板层写死（role_prompt / 必装四件套 / skills / MCP+Key 内联 schema）、模型层可变（模型池 + Agent 引用）。
- `use`：`std::path::Path`（`:7`）、`serde::{Deserialize, Serialize}`（`:9`）、`crate::error::{Context as _, DevforgeError, Result}`（`:11`）。顶部**没有** `use std::collections::BTreeMap`——`McpServer::env` 写全路径（`:50`）。

## 常量（5 个包常量 + 13 个提示词常量）

包常量（行号即声明行）：

| 常量 | 行 | 类型 | 值 |
|---|---|---|---|
| `BASE_PACKAGES` | `:14` | `&[&str]` | `npm:@ff-labs/pi-fff`、`npm:@juicesharp/rpiv-web-tools`、`npm:pi-blackhole`、`git:github.com/NVlabs/SoL-Pi`（`:15`-:18`） |
| `PLAN_AGENT_EXTRA` | `:22` | `&str` | `npm:@juicesharp/rpiv-ask-user-question` |
| `CODING_AGENT_EXTRA` | `:25` | `&[&str]` | ask-user-question、rpiv-todo、pi-lens（`:26`-:28`） |
| `CODING_REVIEW_EXTRA` | `:32` | `&[&str]` | `npm:pi-lens`（单元素） |
| `TEST_AGENT_EXTRA` | `:35` | `&[&str]` | rpiv-todo、ask-user-question（`:36`-:37`） |

提示词常量（13 个，全部 `pub const NAME: &str = include_str!("../../assets/prompts/<name>.md")`，**无内联字面量**）：

| 常量 | 行 | 资产文件 |
|---|---|---|
| `CODING_AGENT_PROMPT` | `:74` | `assets/prompts/coding-agent.md` |
| `CODING_REVIEW_PROMPT` | `:77` | `coding-review.md` |
| `TEST_AGENT_PROMPT` | `:80` | `test-agent.md` |
| `TEST_REVIEW_PROMPT` | `:83` | `test-review.md` |
| `DOC_FIX_PROMPT` | `:86` | `doc-fix.md` |
| `DOC_AGENT_PROMPT` | `:89` | `doc-agent.md` |
| `DOC_REVIEW_PROMPT` | `:92` | `doc-review.md` |
| `CHANGE_PLAN_AGENT_PROMPT` | `:159` | `change-plan-agent.md` |
| `CHANGE_PLAN_REVIEW_PROMPT` | `:162` | `change-plan-review.md` |
| `CHANGE_CODING_AGENT_PROMPT` | `:165` | `change-coding-agent.md` |
| `CHANGE_CODING_REVIEW_PROMPT` | `:168` | `change-coding-review.md` |
| `CHANGE_TEST_AGENT_PROMPT` | `:171` | `change-test-agent.md` |
| `CHANGE_TEST_REVIEW_PROMPT` | `:174` | `change-test-review.md` |

- `assets/prompts/` 下 13 个 `.md` 全部存在（`ls` 实跑）。相对路径以 `src/config/` 为基准上跳两级。

## 数据结构（`#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]` 各一）

| 类型 | derive / struct | 字段（行） |
|---|---|---|
| `McpServer` | `:41` / `:42` | `name: String`（`:44`）、`command: String`（`:46`）、`args: Vec<String>`（`:47`）、`env: std::collections::BTreeMap<String,String>`（`:50`，标注 `#[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]` `:49`） |
| `ModelEntry` | `:95` / `:96` | `id`（`:98`）、`provider`（`:100`）、`model_id`（`:102`）、`context_window: u64`（`:104`）、`max_tokens: u64`（`:106`） |
| `ProviderDef` | `:110` / `:111` | `name`（`:113`）、`api`（`:115`）、`base_url`（`:117`） |
| `AgentManifest` | `:121` / `:122` | `name`（`:124`）、`role_prompt`（`:126`）、`extra_packages: Vec<String>`（`:128`）、`model`（`:130`）、`skills: Vec<String>`（`:132`）、`mcp: Vec<McpServer>`（`:135`，`#[serde(default, skip_serializing_if = "Vec::is_empty")]` `:134`）、`thinking_level: Option<String>`（`:140`，`#[serde(default, skip_serializing_if = "Option::is_none")]` `:139`） |
| `ConfigManifest` | `:144` / `:145` | `base_packages: Vec<String>`（`:147`）、`providers: Vec<ProviderDef>`（`:149`）、`model_pool: Vec<ModelEntry>`（`:151`）、`agents: Vec<AgentManifest>`（`:153`）、`api_key: Option<String>`（`:155`） |

- serde 缺字段语义（本轮实测，HEAD 158b64f 的 `git archive` 副本里加临时探针测试）：`api_key` 无 `#[serde(default)]` 但类型是 `Option<String>` → 缺字段反序列化为 `None`（不报错）；`base_packages` / `providers` / `model_pool` / `agents` 缺字段 → `Err`，文案 `解析配置清单: JSON: missing field base_packages at line 1 column 44`。带 `#[serde(default)]` 的只有 `env`（`:49`）、`mcp`（`:134`）、`thinking_level`（`:139`）。
- `save` 落盘形状：`api_key: None` 会写成 `null`（该字段没有 `skip_serializing_if`）；空 `mcp` / `None` 的 `thinking_level` / 空 `env` 不落盘。

## 函数与方法

| 符号 | 行 | 形状 |
|---|---|---|
| `codegraph_mcp` | `:54`-:61 | `pub fn() -> McpServer`；`name=codegraph`（`:56`）、`command=codegraph`、`args=[serve,--mcp]`、`env={CODEGRAPH_TELEMETRY:0}` |
| `context7_mcp` | `:64`-:71 | `pub fn() -> McpServer`；`name=context7`（`:66`）、`command=npx`、`args=[-y,@upstash/context7-mcp]`、`env` 空 |
| `ConfigManifest::builtin` | `:179`-:436 | `pub fn() -> Self`；局部变量 `plan_agent_prompt`（`:180`-:196`）、`review_prompt`（`:197`-:213`）为**内联字符串**；`Self { … }` 起 `:214` |
| `builtin` 内部字段装配 | — | `base_packages` `:215`；`providers` `:217`-:221`（1 项 newapi）；`model_pool` `:224`-:246`（3 项）；`agents` `:247`-:432`（15 项）；`api_key: None` `:433` |
| `ConfigManifest::resolve_model` | `:438`-:449 | `pub fn(&self, agent: &AgentManifest) -> Result<&ModelEntry>`；线性 find `m.id == agent.model`；未命中 `DevforgeError::Config("Agent `{name}` 引用的模型 `{model}` 不在模型池中")` |
| `ConfigManifest::resolve_provider` | `:451`-:459 | `pub fn(&self, agent: &AgentManifest) -> Result<&ProviderDef>`；先 `resolve_model`，再按 `p.name == model.provider` 查；未命中 `Config("模型 `{id}` 引用的 provider `{name}` 无定义")` |
| `ConfigManifest::get_agent` | `:461`-:466 | `pub fn(&self, name: &str) -> Result<&AgentManifest>`；未命中 `Config("清单中无 Agent `{name}`")` |
| `ConfigManifest::save` | `:468`-:474 | `pub fn(&self, path: &Path) -> Result<()>`；`to_string_pretty` → 有父目录则 `create_dir_all` → `fs::write`；每步 `.ctx(...)` |
| `ConfigManifest::load` | `:476`-:479 | `pub fn(path: &Path) -> Result<Self>`；`read_to_string` + `from_str`；无版本校验、无引用校验 |

## `builtin()` 的 15 个 Agent（逐字段，行号为块内 `name:` 行）

| # | agent | 行 | extra_packages | model | skills | mcp | thinking_level |
|---|---|---|---|---|---|---|---|
| 1 | `plan-agent` | `:249` | `[PLAN_AGENT_EXTRA]`（`:251`） | `deepseek-flash`（`:252`） | `plan-tree`（`:253`） | `[]`（`:254`） | `None`（`:255`） |
| 2 | `plan-review` | `:258` | `[]`（`:260`） | `grok-4.7`（`:261`） | `plan-tree`（`:262`） | `[]`（`:263`） | `Some("medium")`（`:264`） |
| 3 | `coding-agent` | `:269` | `CODING_AGENT_EXTRA`（`:271`） | `deepseek-flash`（`:272`） | doc-contract+coding-discipline+4 语言（`:273`-:280`） | `[codegraph, context7]`（`:281`） | `None`（`:282`） |
| 4 | `coding-review` | `:285` | `CODING_REVIEW_EXTRA`（`:287`） | `grok-4.7`（`:288`） | 同 coding-agent（`:289`-:296`） | `[codegraph, context7]`（`:297`） | `medium`（`:298`） |
| 5 | `test-agent` | `:302` | `TEST_AGENT_EXTRA`（`:304`） | `deepseek-flash`（`:305`） | `python-best-practices`（`:306`） | `[context7]`（`:307`） | `None`（`:308`） |
| 6 | `test-review` | `:311` | `[]`（`:313`） | `grok-4.7`（`:314`） | `[]`（`:315`） | `[]`（`:316`） | `medium`（`:317`） |
| 7 | `doc-fix` | `:321` | `CODING_AGENT_EXTRA`（`:323`） | `deepseek-flash`（`:324`） | doc-contract + 4 语言（`:325`-:331`） | `[codegraph, context7]`（`:332`） | `None`（`:333`） |
| 8 | `doc-agent` | `:336` | `CODING_AGENT_EXTRA`（`:338`） | `minimax-m3.1-flash`（`:339`） | doc-contract + 4 语言（`:340`-:346`） | `[codegraph, context7]`（`:347`） | `None`（`:348`） |
| 9 | `doc-review` | `:351` | `[]`（`:353`） | `grok-4.7`（`:354`） | `doc-contract`（`:355`） | `[]`（`:359`，注释 `:356`-:358` 记 DR-12 拍板） | `medium`（`:360`） |
| 10 | `change-plan-agent` | `:365` | `[PLAN_AGENT_EXTRA]`（`:369`） | `deepseek-flash`（`:370`） | `plan-tree`（`:371`） | `[codegraph]`（`:373`） | `high`（`:374`） |
| 11 | `change-plan-review` | `:377` | `[]`（`:379`） | `grok-4.7`（`:380`） | `plan-tree`（`:381`） | `[codegraph]`（`:383`） | `medium`（`:384`） |
| 12 | `change-coding-agent` | `:388` | `CODING_AGENT_EXTRA`（`:390`） | `deepseek-flash`（`:391`） | coding-discipline + 4 语言（`:392`-:399`） | `[codegraph, context7]`（`:400`） | `high`（`:401`） |
| 13 | `change-coding-review` | `:404` | `[]`（`:406`） | `grok-4.7`（`:407`） | `coding-discipline`（`:408`） | `[codegraph]`（`:410`） | `medium`（`:411`） |
| 14 | `change-test-agent` | `:415` | `CODING_AGENT_EXTRA`（`:417`） | `deepseek-flash`（`:418`） | `python-best-practices`（`:419`） | `[]`（`:420`） | `high`（`:421`） |
| 15 | `change-test-review` | `:424` | `[]`（`:426`） | `grok-4.7`（`:427`） | `[]`（`:428`） | `[]`（`:429`） | `medium`（`:430`） |

- 三个模型池条目：`grok-4.7`（`:226`，model_id `mokeywork/grok-4.7`，窗口 1_000_000、输出 65_536）、`deepseek-flash`（`:233`，`mokeywork/deepseek-flash`，同参数）、`minimax-m3.1-flash`（`:240`，`MiniMax-M3.1-Flash`，同参数）。
- 唯一 provider：`newapi` / `openai-completions` / `https://newapi.trtyr.top/v1`（`:217`-:221`）。
- 各 agent 的 `role_prompt` 行：`:250` plan-agent、`:259` plan-review、`:270`/`:286`/`:303`/`:312`/`:322`/`:337`/`:352`/`:366`/`:378`/`:389`/`:405`/`:416`/`:425` 依次对应其余 13 个。

## 测试（`#[cfg(test)]` `:482`，`mod tests` `:483`，`use super::*` `:484`；15 个 `#[test]`，1 个 `#[ignore]`）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `change_plan_prompts_carry_six_blocks_and_forbidden_items` | `:487` | `PLAN_BLOCKS` 逐块（`:489`）；4 个纪律锚点；review 侧 3 锚点；`VERDICT_MUST_ASK`/`VERDICT_SELF`/`VERDICT_ANSWER_FIELD`（`:510`-:512`、`:528`-:529`）与 `ask-user-question` |
| `change_six_agents_have_pinned_supplies` | `:542` | 6 个 change agent 的 `extra_packages` 逐项相等 |
| `other_agents_supplies_are_unchanged` | `:569` | 其余 9 个 agent 的 `extra_packages` 逐项相等 |
| `builtin_has_eleven_agents_and_four_base_packages` | `:594` | `base_packages == BASE_PACKAGES`（`:596`）、`agents.len()==15`（`:597`-:599`）、各 agent 的 mcp/包/模型/skills 逐项断言（含 `doc-agent` 的 minimax、`doc-review` 的空 mcp） |
| `doc_contract_skill_is_shared_by_four_profiles` | `:705` | 只断言 4 个 profile（coding-agent/coding-review/doc-fix/doc-review）持有 `doc-contract` |
| `coding_discipline_skill_goes_to_exactly_the_four_coding_agents` | `:718` | 4 正 + 11 负；`agents.len()==15`（`:752`） |
| `coding_prompts_carry_discipline_anchors` | `:756` | 两份 coding 提示词 3 锚点 + 「只做内部重构」；两份 review 的「先证后报」 |
| `dump_change_agents_fixture`（`#[ignore]` `:789`） | `:790` | 写 `tests/e2e/fixtures/change_agents.json`；`agents.len()==6`（`:797`） |
| `change_agents_fixture_matches_builtin` | `:808` | 夹具与 builtin 的 change-* 序列化逐字相等 |
| `doc_agent_prompt_carries_protocol_and_guards` | `:829` | 17 个锚点（`只写 docs/contexts/`、codegraph-first、🟢、path:line、`<doc-complete>`、doc-lint、严禁写“为什么” …） |
| `coding_agent_prompt_has_preflight_audit` | `:853` | 预审 5 锚点 + doc-lint 硬门禁 4 锚点 |
| `testing_prompts_carry_protocol_and_discipline` | `:869` | test/test-review 黑盒与 reroute 锚点 + coding-review 省时纪律 + doc 双 Agent 锚点 |
| `model_resolution_from_pool` | `:920` | `plan-agent`→`mokeywork/deepseek-flash`；`plan-review`→`mokeywork/grok-4.7` |
| `unknown_model_rejected` | `:932` | 改成 `ghost-model` → 错误串含「不在模型池中」 |
| `manifest_json_roundtrip` | `:940` | `save`→`load` 后 `assert_eq!(m, loaded)`（tempdir） |

## 仓内消费者（grep 全仓 `--include=*.rs`，排除本文件）

- `src/config/mod.rs:7` 声明、`:12` re-export 4 个类型。
- `src/config/render.rs:9` 引入 `ConfigManifest`；`:30` `Renderer.manifest: &'a ConfigManifest`；`:39` `get_agent`；`:40` `resolve_model`；`:50` 写 AGENTS.md 用 `role_prompt`；`:69` `resolve_provider`；`:70`-:72` 用 `model_pool` 拼 models.json；`:198` `base_packages.clone()`；`:331` 测试里引用 `crate::config::manifest::BASE_PACKAGES`；测试另用 `builtin()`（`:339` 等 19 处）。
- `src/server/mod.rs:27` 引入；`:125` `pub manifest: Mutex<ConfigManifest>`；`:127` `manifest_path: Mutex<Option<PathBuf>>`。
- `src/server/handlers.rs:14` 引入；`:611` `get_manifest`（GET）；`:618`-:620 `put_manifest`（PUT）；`:624`-:625` `resolve_model`/`resolve_provider` fail-fast；`:727`/`:762` 测试用 `builtin()`；`:675`-:680` 借给 `Renderer`。
- `src/main.rs:155`-:162 启动装配：`<root>/manifest.json` 存在则 `ConfigManifest::load`（`:158`），否则 `builtin()`（`:160`）+ `save`（`:161`）。
- `src/server/tests/mod.rs:11`（import）/`:130`、`src/server/tests/http.rs:36`/`:53`/`:71` 用 `ConfigManifest::builtin()` 造测试清单。

## 可重放命令（本轮实跑）

```
TMPDIR=/var/tmp/dfz-src24c/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable \
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/var/tmp/dfz-src24c/target \
cargo test --lib config::manifest
```

- 结果（同一套命令，两棵树）：①仓库工作区（在岸改动可编译时）→ `14 passed; 0 failed; 1 ignored; 583 filtered out; 0.01s`，EXIT=0；②`git archive HEAD`（HEAD 158b64f）导出到 `/var/tmp/dfz-src25-clean` → 同值，EXIT=0。从零编译 36.95s（机器相关）。`1 ignored` 即 `dump_change_agents_fixture`。
- 前置条件：整包 lib 必须可编译（本模块是 lib 级用例）——审查轮在 ARC-R5 在途改动下实测 `cargo test --lib config::manifest` EXIT=101（13 处兄弟模块编译错误，未进入测试）；本轮复跑时工作区已可编译。
- `rustc 1.99.0` / `cargo 1.99.0`；`edition = "2024"`（`Cargo.toml:4`）。

## 外部依赖与环境

- 无进程、无网络、无端口。
- 编译期依赖：`assets/prompts/*.md`（13 个 `include_str!`；文件缺失 = 编译错误）。
- 测试依赖：`tempfile`（`Cargo.toml:27`，`manifest_json_roundtrip` 用）；`env!("CARGO_MANIFEST_DIR")` 定位 e2e 夹具（`:792`、`:811`-`:812`）。
- 测试读取的仓内文件：`tests/e2e/fixtures/change_agents.json`（存在，17532 字节）；唯一写仓内文件的用例是 `#[ignore]` 的 `dump_change_agents_fixture`。
