# 勘察事实 — unit-src-27（src/config/render.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/config/render.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。
> 复核时间：本轮；`wc -l` = **829 行**（实现 1-313，测试 315-829）。

## 文件与模块位置

- 文件：`src/config/render.rs`，829 行，无子模块。
- 声明：`src/config/mod.rs:8` `pub mod render;`；转发面 `src/config/mod.rs:13` `pub use render::Renderer;`（**只转发 `Renderer`**，方法随类型可见；`MAX_RENDER_CONCURRENCY`、`runtime_settings`、`recorded_packages`、`copy_dir` 均不可见）。
- `use`（`:3`-`:10`）：`std::fs`、`std::path::{Path, PathBuf}`、`std::sync::atomic::{AtomicUsize, Ordering}`、`std::sync::Mutex`、`crate::config::installer::PackageInstaller`、`crate::config::manifest::ConfigManifest`、`crate::config::skill_assets::builtin_skill_files`、`crate::error::{Context as _, DevforgeError, Result}`。

## 类型与函数

| 条目 | 行 | 形状 |
|---|---|---|
| `runtime_settings`（私有） | `:15` | `fn() -> serde_json::Value`；返回固定 JSON（`:16`-24）：`retry{maxRetries:3,baseDelayMs:5000}`、`httpIdleTimeoutMs:60000`、`defaultThinkingLevel:"high"`、`hideThinkingBlock:true`、`compaction{enabled:false,reserveTokens:16384,keepRecentTokens:20000}` |
| `MAX_RENDER_CONCURRENCY`（私有） | `:27` | `const usize = 4` |
| `Renderer<'a>` | `:29` | 三个 pub 字段：`manifest: &'a ConfigManifest`（`:30`）、`installer: &'a dyn PackageInstaller`（`:31`）、`skills_source: Option<&'a Path>`（`:33`） |
| `Renderer::render_agent` | `:38` | `fn(&self, profiles_root: &Path, agent_name: &str) -> Result<PathBuf>` |
| `Renderer::render_all` | `:239` | `fn(&self, profiles_root: &Path) -> Result<Vec<PathBuf>>` |
| `recorded_packages`（私有） | `:277` | `fn(&Path) -> Vec<String>`；读 `<profile>/settings.json` 的 `packages` 数组，读/解析失败或键缺失 → 空 `Vec` |
| `copy_dir`（私有） | `:296` | `fn(&Path, &Path) -> Result<()>`；递归 `create_dir_all` + `fs::copy` |

## `render_agent` 的步骤（顺序即代码顺序）

| 步骤 | 行 | 事实 |
|---|---|---|
| 取 Agent | `:39` | `manifest.get_agent(agent_name)?`（未知名字 → `Config` 错误） |
| 解析模型 | `:40` | `manifest.resolve_model(agent)?`（引用不在池 → `Config` 错误） |
| 建骨架 | `:42`-`:43` | `dir = profiles_root.join(&agent.name)`；`create_dir_all(dir/skills)` |
| 读旧记录 | `:47` | `recorded_packages(&dir)`（**在写 settings 之前**取，因为后面要重写 settings） |
| AGENTS.md | `:50`-`:51` | 内容 = `agent.role_prompt` 原文 |
| settings.json | `:54`-`:70` | 基底 = `runtime_settings()`；覆盖 `defaultProvider` = `model.provider`（`:55`）、`defaultModel` = `model.model_id`（`:56`）；若 `agent.thinking_level` 为 `Some` 覆盖 `defaultThinkingLevel`（`:62`-64`）；pretty 写盘（`:66`-70`） |
| models.json | `:73`-`:99` | `manifest.resolve_provider(agent)?`；只取 `model_pool` 中 `m.provider == provider.name` 的条目（`:76`-78`），每条映射 `{id, name, contextWindow, maxTokens}`（`:79`-86`）；外壳 `{"providers": {<provider>: {api, baseUrl, models}}}`（`:87`-94`）；pretty 写盘 |
| skills 分发 | `:104`-`:131` | 对 `agent.skills` 逐个：`skills_source = Some(src)` → `copy_dir(src/<skill>, dir/skills/<skill>)`（失败上抛，**不降级**）；`None` → `builtin_skill_files(skill)` 命中则逐文件写（`Some((name, body))`）；未命中 → 建目录 + 写占位 `README.md`（`"# {skill}\n\n占位目录——无内置资产且未提供 skills 源。\n"`） |
| auth.json | `:136`-`:141` | `api_key = Some` → 写 `{"<provider>": {"type":"api_key","key": <明文>}}`（`.to_string()` 非 pretty）；`None` → **不写也不删**，只 `tracing::warn!`（`event="profile.render_without_api_key"`，`:143`-`:151`） |
| mcp.json | `:155`-`:193` | `agent.mcp` 空 → 若旧文件存在则 `remove_file`（`:157`-`:161`）；非空 → 拼 `{"mcpServers": {<name>: {command, args, env?}}}`（`env` 为空则不带该键，`:174`-`:183`），pretty 写盘 |
| 组包 | `:196`-`:198` | `packages = base_packages.clone()` + `agent.extra_packages`（顺序：四件套在前） |
| 幂等判定 | `:202`-`:203` | `installed = packages 全部在 previous 中`；`dual_record = <dir>/npm/package.json 存在` |
| 跳过分支 | `:204`-`:220` | `installed && dual_record && !previous.is_empty()` → 把 `packages` 回写进 settings.json（`:205`-`:212`）、`tracing::info!`（「profile 复用」）、`return Ok(dir)`（**不再调 installer**） |
| 安装 | `:222` | `self.installer.install(&dir, &packages)?`（失败整次渲染失败，已写文件留在盘上） |
| 完成 | `:224`-`:230` | `tracing::info!`「profile 渲染完成」；`Ok(dir)` |

## `render_all` 的步骤

- `:241`-`:243` 空清单 → `Ok(Vec::new())`。
- `:244` `workers = MAX_RENDER_CONCURRENCY(4).min(agents.len()).max(1)`。
- `:245`-`:247` `next: AtomicUsize`（工作分发）+ `slots: Vec<Mutex<Option<Result<PathBuf>>>>`（按索引收结果）。
- `:248`-`:259` `std::thread::scope`，起 `workers` 个线程，每个循环 `fetch_add` 取索引 → `render_agent(profiles_root, &agents[i].name)` → 写入 `slots[i]`。
- `:260`-`:272` 按索引顺序收：`Some(res) => rendered.push(res?)`（**首个错误（按清单序）即返回**），`None` → `DevforgeError::System("渲染槽位未填充（并发调度异常）")`。
- 无超时、无取消：任一 worker 卡住则 scope join 不返回。

## 落盘产物（每次 `render_agent` 可能写/删的文件）

| 路径（相对 `<dir>`） | 写/删 | 内容要点 |
|---|---|---|
| `AGENTS.md` | 必写 | `agent.role_prompt` 原文 |
| `settings.json` | 必写（跳过分支再回写一次） | 运行时配置 + `defaultProvider`/`defaultModel`/可选 `defaultThinkingLevel`；跳过分支追加 `packages` 数组 |
| `models.json` | 必写 | 当前 provider 与其全部同 provider 模型（`{id,name,contextWindow,maxTokens}`） |
| `auth.json` | 仅在 `api_key` 为 `Some` 时写 | `{"<provider>": {"type":"api_key","key": <明文>}}` |
| `mcp.json` | `mcp` 非空则写，空则删旧文件 | `{"mcpServers": {...}}` |
| `skills/<name>/<file>` | 按 `agent.skills` | 源目录复制 / 内置资产 / 占位 `README.md` |
| `npm/package.json` | 不直接写 | 由 `installer`（真桥 `pi install` / `FakeInstaller`）双记录写 |

## 测试（`#[cfg(test)]` `:315`，`mod tests` `:316`，22 个 `#[test]`，全部同步、无网络）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `render_creates_directory_structure` | `:338` | 返回路径 = `root/plan-agent`；AGENTS.md == role_prompt；settings 的 provider/model；内置 skill 真落盘（`skills/plan-tree/SKILL.md` 以 `---` 开头且含「目录约定」） |
| `render_empty_mcp_removes_stale_mcp_json` | `:361` | 预置旧 mcp.json + 清单 `mcp` 空 → 渲染后文件不存在 |
| `doc_contract_skill_renders_into_coding_profile` | `:384` | `coding-agent` 的 `skills/doc-contract/SKILL.md` 落盘且含「必填」 |
| `dual_records_carry_base_plus_extra` | `:400` | `plan-agent` 的 settings `packages` 5 条 + `npm/package.json` `dependencies` 5 条 |
| `render_all_parallel_preserves_manifest_order` | `:450` | 返回顺序 == 清单顺序；`CountingInstaller` 调用次数 == agent 数；每个 profile 有 AGENTS.md |
| `render_all_propagates_install_failure` | `:481` | `FailingInstaller` → `render_all` 返回 `Config` 错误 |
| `second_render_skips_install_when_packages_already_present` | `:497` | 二次渲染计数不变；跳过分支回写 `packages` |
| `manifest_package_change_forces_reinstall` | `:525` | 清单加一个包 → 计数 +1 |
| `missing_npm_record_forces_reinstall` | `:559` | 删 `<profile>/npm/package.json` → 计数 +1 |
| `review_agent_gets_no_extra_packages` | `:580` | `plan-review` 的 settings `packages` 恰 4 条 |
| `review_profiles_render_reduced_thinking_level` | `:592` | 四个 `*-review` → `medium`；`coding-agent`/`test-agent` → `high` |
| `api_key_written_provider_keyed` | `:616` | `auth.json` 为 `{"newapi":{"type":"api_key","key":"sk-test-123"}}`；旧形状键 `api_key` 不存在 |
| `models_json_carries_provider_and_budget` | `:630` | `api`/`baseUrl` 正确；同 provider 3 个模型；`MiniMax-M3.1-Flash` 与 `mokeywork/deepseek-flash` 的窗口/预算 |
| `settings_carry_runtime_keys` | `:655` | `retry.maxRetries`=3、`httpIdleTimeoutMs`=60000、`defaultThinkingLevel`=high、`compaction.enabled`=false，且模型层键共存 |
| `prompts_carry_protocol_and_workflow` | `:674` | `plan-review` 提示词含 `<verdict>PASS</verdict>`/`<verdict>FAIL</verdict>`/「快速审查」；`plan-agent` 含「小步快跑」 |
| `skills_copied_from_source_when_given` | `:685` | 给了 `skills_source` → 复制源内容（逐字相等），不写内置资产 |
| `render_all_covers_every_agent` | `:705` | 渲染 15 个；九个 MVP profile 目录都在 |
| `coding_agent_profile_has_mcp_json` | `:730` | codegraph（`serve --mcp`、`CODEGRAPH_TELEMETRY=0`）+ context7；`plan-agent` 不生成 mcp.json |
| `coding_packages_carry_todo_and_lens` | `:755` | `coding-agent` 7 条包；`coding-review` 5 条且不含 todo |
| `coding_prompts_carry_protocol_and_discipline` | `:795` | coding 提示词协议锚点与纪律短语 |
| `unknown_agent_rejected` | `:815` | `render_agent("ghost")` → `Config` 错误 |
| `base_packages_exactly_four_verbatim` | `:825` | 四件套 4 条且含 `git:github.com/NVlabs/SoL-Pi` |

- 测试辅助：`renderer(manifest)`（`:322`，`installer: &FakeInstaller`、`skills_source: None`）、`base_packages()`（`:330`）、`CountingInstaller(AtomicUsize)`（`:429`，计数后转调 `FakeInstaller`）、`FailingInstaller`（`:438`，返回 `Config` 错误）。

## 仓内消费者

- `src/server/handlers.rs:669` `render_body`；`:675`-680 `crate::config::Renderer { manifest, installer: st.installer.as_ref(), skills_source }`；`:682` 附近调 `render_all`；`:687`-688 失败 → SSE `config.render_failed`；由 `put_manifest`（`:618`）在 `:639` 起的后台线程执行。
- `src/config/mod.rs:8`/`:13` 声明与转发。
- 测试注入方：`src/server/tests/mod.rs:134`、`tests/f6_observability/fixture.rs:31`（`FakeInstaller`）。

## 可重放命令与证据

```
TMPDIR=<宽裕临时目录> RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable \
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
cargo test --lib config::render
```

- 本轮实跑（`CARGO_TARGET_DIR=/var/tmp/dfz-src27b/target`，`cargo test --lib config::render`）：`test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 576 filtered out; finished in 0.16s`，EXIT=0。
- 交叉证据：同会话早前整棵 config 树 `cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 550 filtered out`，其中 render 22 例（`--list` 计数：installer 5 / manifest 15 / render 22 / skill_assets 6）。
- ⚠️ 写作中途一次重跑**编译失败**，原因在别处的在途重构：`src/sop/doc/mod.rs` 当时尚未声明 `pub mod prompts;`（12 个 E0432/E0433/E0308 错误，全在 `src/sop/doc/*` 与 `src/engine/doc_wiring.rs`）。该重构随后落地，复跑即绿；`src/config/render.rs` 全程不在改动集中（git 状态可见）。此现象记进 `_meta/gaps.md`。
- edition 2024（`Cargo.toml:4`）；测试依赖 `tempfile`（`Cargo.toml:27`）与 `serde_json`（生产依赖）。
