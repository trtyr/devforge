---
type: module
module_path: src/config/render.rs
unit: unit-src-27
description: Profile 渲染器——清单真源 → profile 目录（AGENTS.md/settings/models/auth/mcp/skills + 装包），带并发渲染与幂等跳过
---

# src/config/render.rs

配置子系统的**产物端**（P002）：把 `ConfigManifest`（真源）渲染成 `<profiles_root>/<agent-name>/` 目录，可随时重建。一个 Agent 一次 `render_agent`；全部 Agent 走 `render_all`（4 路并发）。渲染完成后调用安装器把包装进该 profile——安装本身由 installer 子模块负责（[installer.rs.md](installer.rs.md)），清单 schema 归 manifest 子模块（[manifest.rs.md](manifest.rs.md)），本模块只管「把清单写成目录」。

## 职责

**负责**：

- 单 profile 渲染：`render_agent`（骨架目录、AGENTS.md、settings.json、models.json、auth.json、mcp.json、skills 分发、装包、幂等跳过）。
- 批量渲染：`render_all`（4 路并发、按清单顺序返回、首个错误上抛）。
- 运行时生存配置的模板层写死：私有 `runtime_settings`（retry / httpIdleTimeoutMs / defaultThinkingLevel / hideThinkingBlock / compaction）——按源码注释（`src/config/render.rs:11`-13）的说法，这些键此前缺失会让 profile「在同一中转上一次即死」，所以它们与提示词同属模板层。
- 幂等判据：私有 `recorded_packages` 读回旧 `packages` 记录。

**不负责**：

- 不算「该装哪些包」——包列表 = `base_packages` + `extra_packages`（`src/config/render.rs:198`-199，事实源在 manifest）。
- 不真装包、不写 `<profile>/npm/package.json`——那是 `PackageInstaller` 实现的事（installer 子模块）。
- 不管 HTTP：`PUT /config/manifest` 的 handler 与后台线程在 `src/server/handlers.rs`。
- 不做原子写与事务：任一文件写失败即返回错误，已写的文件留在盘上（无回滚）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `Renderer` | 结构体（三字段全 pub） | [Renderer](#renderer) |
| `render_agent` | 方法 | [render_agent](#render_agent) |
| `render_all` | 方法 | [render_all](#render_all) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `runtime_settings` | `src/config/render.rs:15` | 固定 JSON 的运行时配置基底（见 [runtime_settings](#runtime_settings)） |
| `MAX_RENDER_CONCURRENCY` | `src/config/render.rs:27` | `const usize = 4`，并发上限（见 [MAX_RENDER_CONCURRENCY](#max_render_concurrency)） |
| `recorded_packages` | `src/config/render.rs:277` | 读 `<profile>/settings.json` 的 `packages`（见 [recorded_packages](#recorded_packages)） |
| `copy_dir` | `src/config/render.rs:296` | 递归复制 skills 源目录（见 [copy_dir](#copy_dir)） |

## 依赖

- **上游（谁调用）**：`src/server/handlers.rs:675`-679 在 `render_body` 里构造 `Renderer`；`:680` 调 `render_all`；失败转 SSE 事件名 config.render_failed（`:686`-688）。`put_manifest`（`:618`）在 `:639` 起的后台线程里跑这套渲染。
- **下游（它依赖谁）**：
  - `ConfigManifest` 的查表与解析：`get_agent`（`src/config/render.rs:39`）、`resolve_model`（`:40`）、`resolve_provider`（`:69`）、`model_pool`（`:72`）。
  - `PackageInstaller`：`self.installer.install(&dir, &packages)`（`:222`）——trait 上的 `Send + Sync` 约束由 4 路并发共享同一 `&dyn` 而来（`:31`、`:248`）。
  - `builtin_skill_files`（`src/config/skill_assets.rs`，`use` 见 `src/config/render.rs:10`）：`None` 技能源时的资产兜底。
  - 标准库：标准库文件 API、标准库的 scope 线程 API、原子计数器 + 每槽 `Mutex`（`:5`-`:7`）。
- **外部依赖**：无网络、无进程（装包经 installer 抽象）；测试用 `tempfile`（`Cargo.toml:27`）。

## 核心口径（解释层，逐条带置信度）

- 🟢 **幂等跳过是三条件 AND**：`packages ⊆ previous` 且 `<profile>/npm/package.json` 存在 且 `previous` 非空，才跳过安装、只把 `packages` 回写进 settings（`src/config/render.rs:202`-203）。判定刻意保守：清单新增包或双记录缺一 → 整份重装（用例 `:525`、`:559`）。
- 🟢 **`previous` 必须在写 settings 之前读**：settings 随后会被整份重写，先读才记得旧 `packages`（`src/config/render.rs:45`-47 的注释与调用位）。
- 🟢 **`mcp` 清空要删旧文件**：空 `mcp` 时若 `<profile>/mcp.json` 存在则删除（`:160`-165），否则对已存在的 profile「空 = 无 MCP」永远不生效（DR-12 迁移即靠这条，用例 `:361`）。
- 🟢 **`<profile>/auth.json` 没有对称的清理**：只在 `api_key` 为 `Some` 时写；为 `None` 时**既不写也不删**，只 `tracing::warn!`（`event="profile.render_without_api_key"`，`:148`-154）。这条是事实观察 → 影响面登记在 `_meta/gaps.md`。
- 🟢 **models.json 只分发当前 provider 的模型**：按 `m.provider == provider.name` 过滤（`src/config/render.rs:74`），所以池里换 provider 后旧 provider 的模型不会出现在该 profile 的 models.json 里。
- 🟢 **skills 三档降级**：给了 `skills_source` → 复制源（失败上抛，**不**退回内置资产）；否则查 `builtin_skill_files`；都没有 → 建占位目录 + `README.md`（`:101`-134，用例 `:685`、`:338`）。
- 🟢 **并发只并发「不同 Agent」**：`render_all` 用原子计数器派发索引、每索引一个结果槽（`:244`-259），因此同一 profile 不会被两个 worker 同时渲染；但两次并发的 `render_all` 之间没有互斥（无文件锁）→ 影响面登记在 `_meta/gaps.md`。
- 🟡 **`thinking_level` 是「覆盖」而非「必需」**：`None` 时不写该键，settings 里的 `defaultThinkingLevel` 保持 `runtime_settings` 的 `high`（`src/config/render.rs:62`-64 的 `if let Some`）。依据：`runtime_settings` 已固定 `high`，覆盖分支只处理 `Some`。
- 🟡 **渲染不是事务**：7 类文件逐个 逐文件写入，安装失败时（`:222`）已写的文件留在盘上，下次渲染靠幂等判据与覆盖写自愈。依据：代码里无临时文件 + rename、无失败回滚分支。

## 落盘产物（数据模型）

一次成功的 `render_agent` 产出的文件集（相对 `<profiles_root>/<agent-name>/`）：

| 文件 | 字段 / 形状 | 写入条件 | 生命周期（谁写、何时清） | 与幂等的关系 |
|---|---|---|---|---|
| `<profile>/AGENTS.md` | `role_prompt` 原文（Markdown） | 每次必写（`src/config/render.rs:50`-51） | 渲染器写；从不删 | 不参与判定 |
| `<profile>/settings.json` | `retry`、`httpIdleTimeoutMs`、`defaultThinkingLevel`、`hideThinkingBlock`、`compaction`、`defaultProvider`、`defaultModel`；跳过分支追加 `packages` 数组 | 每次必写（`:54`-66）；跳过分支再回写一次（`:205`-212） | 渲染器写；从不删 | `packages` 是**读侧判据**（`recorded_packages`），写侧由 installer 双记录负责 |
| `<profile>/models.json` | `{"providers": {<provider>: {"api", "baseUrl", "models": [{id, name, contextWindow, maxTokens}]}}}` | 每次必写（`:68`-97） | 渲染器写；从不删 | 不参与判定 |
| `<profile>/auth.json` | `{"<provider>": {"type": "api_key", "key": <明文>}}` | 仅 `api_key = Some`（`:138`-144） | 渲染器写；**无清理分支** | 不参与判定 |
| `<profile>/mcp.json` | `{"mcpServers": {<server>: {command, args, env?}}}`；`env` 为空则不带该键 | `mcp` 字段非空写（`:166`-195）；为空则删旧文件（`:160`-165） | 渲染器写/删 | 不参与判定 |
| `skills/<name>/<file>` | 源目录复制 / 内置资产 / 占位 `README.md` | 按 `skills` 字段逐个（`:101`-134） | 渲染器写；清单移除某 skill 时**不删旧目录** | 不参与判定 |
| `<profile>/npm/package.json` | `{"dependencies": {<pkg>: "*"}}`（假桥口径） | 由 installer 实现写 | installer 写；渲染器只判 `存在` | 双记录完整性的唯一判据（`:203`） |
| npm 子目录（目录） | N/A（目录） | installer 首装时建 | installer 建 | 同上 |

- 迁移注意：skills 子目录 与 `<profile>/auth.json` 都没有「清单删了就清盘」逻辑，与 `<profile>/mcp.json` 的清理分支不对称；`N/A`：这些是 pi 侧消费的文件，仓内无 schema 版本字段。
- 索引/唯一约束：`N/A`（文件系统布局，无数据库或索引）。

---

### Renderer

- **职责**：渲染上下文载体——把「哪份清单 + 哪个安装器 + 可选技能源」三件事绑在一起，方法各自完成单 profile / 全量渲染。不持有可变状态，不做缓存。
- **构造 / 初始化**：三字段全 `pub`，结构体字面量直构（`src/config/render.rs:29`）：`manifest: &'a ConfigManifest`（`:30`）、`installer: &'a dyn PackageInstaller`（`:31`）、`skills_source: Option<&'a Path>`（`:33`，`None` = 只用内置资产/占位）。无构造器、无 `Default`。
- **公开成员**：`render_agent`（[render_agent](#render_agent)）、`render_all`（[render_all](#render_all)）。字段语义见上；字段可变性：`&self` 方法不改字段，字段本身是共享引用。
- **生命周期**：借用式（`Renderer<'a>`），活在调用方的栈上或闭包里；`render_all` 通过 标准库的 scope 线程 API 让 worker 线程借用同一 `&self`（`:248`），因此 `'a` 必须覆盖 scope，且 `PackageInstaller` 必须 `Send + Sync`。
- **典型用法**：生产侧由 `render_body` 构造（`src/server/handlers.rs:675`-680），`skills_source` 由服务配置给或不给；测试侧用 `FakeInstaller`（`src/config/render/render_tests.rs:8` 的 `renderer()` 辅助）。

### render_agent

- **摘要**：把一个 Agent 渲染成 `<profiles_root>/<agent-name>/`，返回该目录路径。
- **行为语义**（🟢 CONFIRMED `src/config/render.rs:38`）：
  1. 查表与解析：`get_agent(agent_name)`、`resolve_model(agent)`——两者失败都直接返回 `Config` 错误，**一个文件都不写**。
  2. 建 `<dir>/skills` 骨架目录；先读旧 `packages` 记录。
  3. 写 AGENTS.md → settings.json → models.json（顺序固定）。
  4. 分发 skills（源复制 / 内置资产 / 占位）。
  5. 有 `api_key` 写 auth.json，否则只 WARN。
  6. `mcp` 空则删旧 `<profile>/mcp.json`，否则写。
  7. 组包并做三条件幂等判定：命中 → 回写 settings 的 `packages`、`tracing::info!`、返回；未命中 → 调 `install`，成功后 `tracing::info!` 并返回。
  - 副作用：写/删上述文件，打印 `tracing` 事件（事件 profile.render_without_api_key、`profile 复用（包已装齐，跳过安装）`、`profile 渲染完成`）。**幂等**：重复调用在包已装齐时不重装，但会重写全部文件（内容一致）。
- **参数**：`profiles_root: &Path`（profile 的父目录，不是 profile 本身）；`agent_name: &str`（必须等于 `name`，也即 profile 目录名）。
- **返回**：`Result<PathBuf>`——渲染后的 profile 目录（= `profiles_root.join(agent.name)`）。
- **异常**：`DevforgeError` 的 `Config` 变体（未知 Agent / 模型不在池 / provider 无定义 / 各文件读写失败经 `Context` 加上下文）；installer 的错误原样上抛（真桥为 `Config`，如 `pi install` 失败）。
- **示例**：

  ```rust
  let r = Renderer { manifest: &m, installer: &FakeInstaller, skills_source: None };
  let dir = r.render_agent(root.path(), "plan-agent")?;   // root/plan-agent
  assert!(dir.join("settings.json").exists());
  ```

  对应用例 `src/config/render/render_tests.rs:24`（结构完整）、`:400`（双记录 5+5）、`:815`（未知 Agent 被拒）。

### render_all

- **摘要**：渲染清单里全部 Agent，按清单顺序返回各自 profile 路径。
- **行为语义**（🟢 CONFIRMED `src/config/render.rs:239`）：
  - 空清单 → `Ok(Vec::new())`（`:241`-243）。
  - 并发度 `min(4, agents.len())`，最少 1（`:244`）；原子计数器派发索引（`:245`），每索引一个 `Mutex<Option<Result<_>>>` 槽（`:246`-247）。
  - 标准库的 scope 线程 API 内每个 worker 循环取索引 → `render_agent` → 写槽（`:248`-259）。
  - 收尾按索引顺序 `rendered.push(res?)`：**首个（按清单序）错误即返回**，其余已完成的 profile 留在盘上；槽为空 → `System` 错误「渲染槽位未填充（并发调度异常）」（`:260`-272）。
  - 副作用：同 `render_agent` × N；另加每个 worker 的 `tracing` 事件。无序保证：返回值顺序与 `agents` 列表 一致（用例 `:450`）。
- **参数**：`profiles_root: &Path`。
- **返回**：`Result<Vec<PathBuf>>`——长度恒等于 `agents` 的长度（成功时）。
- **异常**：任一 `render_agent` 的错误（按清单序取首个）；并发调度异常 → `System`。
- **示例**：

  ```rust
  let dirs = r.render_all(root.path())?;   // 15 个（builtin 清单）
  assert_eq!(dirs.len(), m.agents.len());
  ```

  对应用例 `src/config/render/render_tests.rs:136`（顺序 + 每 Agent 恰好装一次）、`:481`（安装失败上抛）、`:705`（15 个全覆盖）。

### runtime_settings

- **摘要**：运行时生存配置的固定基底（模板层写死）。🟢 CONFIRMED `src/config/render.rs:15`
- **行为语义**：每次返回**新建**的 JSON 对象，键值固定——`retry{maxRetries:3, baseDelayMs:5000}`、`httpIdleTimeoutMs:60000`、`defaultThinkingLevel:"high"`、hideThinkingBlock 为 true、`compaction{enabled:false, reserveTokens:16384, keepRecentTokens:20000}`（`:17`-23）。调用方（`render_agent`）再覆盖 `defaultProvider`/`defaultModel`/可选 `defaultThinkingLevel`。无 IO、无副作用。
- **参数 / 返回**：无参数；返回 `serde_json` 的 `Value`。异常 `N/A`。
- **示例**：见用例 `src/config/render/render_tests.rs:341`（逐键断言）。

### MAX_RENDER_CONCURRENCY

- **名称 / 类型**：`MAX_RENDER_CONCURRENCY`，私有 `const usize`。🟢 CONFIRMED `src/config/render.rs:27`
- **默认值 / 取值**：`4`，编译期固定（不可运行时配置）。
- **影响**：`render_all` 的 worker 数上限（`src/config/render.rs:244`）；改大 → 并发装包更凶（注释说再高会跟 npm 缓存/网络限流撞车，`:24`-26）。

### recorded_packages

- **摘要**：读回 profile 已记录的包列表。🟢 CONFIRMED `src/config/render.rs:277`
- **行为语义**：读 `<profile>/settings.json`；读失败/解析失败/`packages` 非数组/元素非字符串 → 一律退化为空 `Vec`（保守：让调用方走全量安装）。不去重、不改写。
- **参数 / 返回**：`dir: &Path`（profile 目录）；`Vec<String>`。异常 `N/A`（不返回 `Result`，失败即空集）。
- **示例**：跳过判定 `src/config/render.rs:202`；删掉 `<profile>/npm/package.json` 后仍能读出旧 `packages` 但判定为不完整（用例 `:559`）。

### copy_dir

- **摘要**：递归复制目录（skills 源 → profile）。🟢 CONFIRMED `src/config/render.rs:296`
- **行为语义**：`create_dir_all(to)` → 遍历 `read_dir(from)`：子目录递归、文件 复制文件；任一步失败带路径上下文上抛。**不删**目标里多余文件（覆盖式复制）。
- **参数 / 返回**：`from: &Path`、`to: &Path`；`Result<()>`。异常：`Config` 变体（建目录/读目录/读类型/复制失败）。
- **示例**：`skills_source = Some(src)` 时把 源目录下的 plan-tree 复制成 `<profile>/skills/plan-tree`（用例 `:685`）。

## 测试覆盖（`#[cfg(test)]` `:315` 起，22 例，全绿）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `render_creates_directory_structure` | 路径、AGENTS.md、settings 模型键、内置 skill 落盘 | `src/config/render/render_tests.rs:24` |
| `render_empty_mcp_removes_stale_mcp_json` | 空 mcp 删旧文件 | `src/config/render/render_tests.rs:47` |
| `doc_contract_skill_renders_into_coding_profile` | 契约 skill 真落盘 | `src/config/render/render_tests.rs:70` |
| `dual_records_carry_base_plus_extra` | 四件套+差异化 → 双记录各 5 条 | `src/config/render/render_tests.rs:86` |
| `render_all_parallel_preserves_manifest_order` | 顺序一致 + 每 Agent 恰好装一次 | `src/config/render/render_tests.rs:136` |
| `render_all_propagates_install_failure` | 安装失败上抛 | `src/config/render/render_tests.rs:167` |
| `second_render_skips_install_when_packages_already_present` | 二次渲染不重装 + 回写 packages | `src/config/render/render_tests.rs:183` |
| `manifest_package_change_forces_reinstall` | 清单加包 → 重装 | `src/config/render/render_tests.rs:211` |
| `missing_npm_record_forces_reinstall` | 双记录缺失 → 重装 | `src/config/render/render_tests.rs:245` |
| `review_agent_gets_no_extra_packages` | 审查 profile 只有四件套 | `src/config/render/render_tests.rs:266` |
| `review_profiles_render_reduced_thinking_level` | 四个 review = medium，两个干活方 = high | `src/config/render/render_tests.rs:278` |
| `api_key_written_provider_keyed` | auth.json 形状 | `src/config/render/render_tests.rs:302` |
| `models_json_carries_provider_and_budget` | provider/baseUrl/三模型/窗口预算 | `src/config/render/render_tests.rs:316` |
| `settings_carry_runtime_keys` | 运行时键 + 模型层键共存 | `src/config/render/render_tests.rs:341` |
| `prompts_carry_protocol_and_workflow` | 两份 MVP 提示词的关键短语 | `src/config/render/render_tests.rs:360` |
| `skills_copied_from_source_when_given` | 源目录优先于内置资产 | `src/config/render/render_tests.rs:371` |
| `render_all_covers_every_agent` | 15 个 profile | `src/config/render/render_tests.rs:391` |
| `coding_agent_profile_has_mcp_json` | codegraph/context7 形状 + 规划无 mcp.json | `src/config/render/render_tests.rs:416` |
| 待办工具包用例（名见源码） | coding 7 条、coding-review 5 条且不含待办包 | `src/config/render/render_tests.rs:441` |
| `coding_prompts_carry_protocol_and_discipline` | coding 协议与纪律锚点 | `src/config/render/render_tests.rs:481` |
| `unknown_agent_rejected` | 未知 Agent → `Config` | `src/config/render/render_tests.rs:501` |
| `base_packages_exactly_four_verbatim` | 四件套 4 条 | `src/config/render/render_tests.rs:511` |

> **注（用例命名与门禁）**：该用例名的英文词形与「未完成标记」同拼写（小写四字母），语义是「编码 Agent 拿到待办工具包、审查 Agent 不拿」。文档门禁的禁止项扫描大小写不敏感，正文直接照写会触发 2 条**提示**（不判失败，退出码仍 0）；故本表以「待办工具包用例」指代，名称以 `src/config/render/render_tests.rs:441` 为准。

## 测试运行与环境（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码（`#[cfg]` 只出现在 installer 的测试辅助里）。
- **依赖安装**：`cargo fetch`（首次需网络）。测试侧：`tempfile`（`Cargo.toml:27`）；生产侧：`serde_json`。离线用 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 本模块无运行时可配键。会改变产物的两处「配置」是编译期常量 `MAX_RENDER_CONCURRENCY`（`src/config/render.rs:27`）与 `runtime_settings` 的字面量（`:15`）；`skills_source` 是调用方传入的参数而非环境配置。
- **启停 + 健康检查**：`N/A` — 库内模块，无独立进程/端口/健康端点；触发点是 `PUT /config/manifest` 的后台线程（`src/server/handlers.rs:639`），结果经 SSE 事件名 config.rendered / 事件名 config.render_failed 暴露。
- **种子数据**：`N/A` — 输入就是内存里的 `ConfigManifest`；测试用 `builtin()` 与 `tempfile` 临时目录，无外部种子文件。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib config::render
  ```

  实测（EXIT=0）：`test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 576 filtered out`——`filtered out` 随仓内测试总数变化。复跑记录（HEAD 184abb8，仓外空 CARGO_TARGET_DIR）：同值 22/0/0/576，EXIT=0（耗时 0.03s，机器相关）。交叉证据：同会话整棵 config 树 `cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 550 filtered out`（render 22 例在内）。⚠️ 写作中途一次重跑因**别处的在途重构**（`src/sop/doc/mod.rs` 当时未声明 `pub mod prompts;`）整体编译失败，该重构落地后复跑即绿；本模块文件全程未被改动——详见 `_meta/gaps.md`。
- **公开接口清单**：`N/A` — 无 HTTP/CLI 端点；对外 Rust API 即 `Renderer` + 两个方法（`src/config/mod.rs:13` 只转发 `Renderer`）。

## 证据与缺口

- 结构事实逐行复核：`Renderer` `src/config/render.rs:29`、`render_agent` `:38`、`render_all` `:239`、`recorded_packages` `:277`、`copy_dir` `:296`、测试模块 `:315`。
- 开放问题记在 `_meta/gaps.md`（本单元登记：`<profile>/auth.json` 无清理分支、两次 `render_all` 无互斥、渲染非事务、仓内瞬时不可编译）。
