# 勘察事实 — unit-src-26（src/config/mod.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/config/mod.rs.md`。

## 文件与模块位置

- 文件：`src/config/mod.rs`（13 行）。
- 模块路径：`crate::config` / 外部 `devforge::config`（库 crate 名 `devforge`，`Cargo.toml:2`）。
- 文件头注释（`:1`-4）：配置管理（P002 · 双管理面：Agent 管理面 + 模型管理面）；配置即代码——清单为真源，profile 目录 = 渲染产物；模板层写死（提示词/必装四件套/skills/MCP/Key），模型层可变（模型池 + Agent 引用）。

## 文件内全部非注释行（逐行）

| 行 | 内容 |
|---|---|
| `:6` | `pub mod installer;` |
| `:7` | `pub mod manifest;` |
| `:8` | `pub mod render;` |
| `:9` | `pub mod skill_assets;` |
| `:11` | `pub use installer::{build_install_command, PackageInstaller, PiInstallBridge};` |
| `:12` | `pub use manifest::{AgentManifest, ConfigManifest, ModelEntry, ProviderDef};` |
| `:13` | `pub use render::Renderer;` |

- 转发面共 8 个名字：`build_install_command`、`PackageInstaller`、`PiInstallBridge`（来自 installer）、`AgentManifest`、`ConfigManifest`、`ModelEntry`、`ProviderDef`（来自 manifest）、`Renderer`（来自 render）。
- **不转发**：`installer` 的 `FakeInstaller`（测试桥，`src/config/installer.rs:71`）；manifest 的全部包常量/提示词常量/方法（`builtin`、`resolve_model`/`resolve_provider`、`get_agent`、`save`、`load`）；`skill_assets` 整模块的任何符号。
- 无函数、无类型定义、无 `#[cfg(test)]`（本文件零测试）。

## 子模块（各自独立单元，本文只建引用）

| 子模块 | 声明行 | 文件 | 覆盖单元 | 公开面（摘要） |
|---|---|---|---|---|
| `installer` | `:6` | `src/config/installer.rs`（198 行） | unit-src-24 | `build_install_command`、`PackageInstaller`（trait）、`PiInstallBridge`、`FakeInstaller` |
| `manifest` | `:7` | `src/config/manifest.rs`（1504 行） | unit-src-25 | 5 个包常量、`McpServer` + 2 个构造、13 份提示词常量、4 个类型、`ConfigManifest` 的 6 个方法/关联函数 |
| `render` | `:8` | `src/config/render.rs`（829 行） | unit-src-27 | `Renderer`（字段 `manifest`/`installer`/`skills_source`）、`render_agent`、`render_all`，以及私有 `recorded_packages`、`runtime_settings`、`MAX_RENDER_CONCURRENCY` |
| `skill_assets` | `:9` | `src/config/skill_assets.rs`（229 行） | unit-src-29 | `PLAN_TREE_SKILL_MD`（`:7`）、`DOC_CONTRACT_SKILL_MD`（`:10`）、`CODING_DISCIPLINE_SKILL_MD`（`:13`）+ `builtin_skill_files`（`:18`），均为 `include_str!`/内嵌资产 |

## crate 内调用点（消费者，只列经过 `crate::config::` 面的）

- `src/main.rs:158` `devforge::config::ConfigManifest::load`；`:160` `ConfigManifest::builtin`；`:161` `save`；`:201` `devforge::config::installer::PiInstallBridge`（走子模块路径，不走转发面）。
- `src/server/mod.rs:27` `use crate::config::ConfigManifest`；`:26` `use crate::config::installer::PackageInstaller`。
- `src/server/handlers.rs:14` `use crate::config::ConfigManifest`；`:675` `crate::config::Renderer { … }`；`:699` `crate::config::installer::FakeInstaller`（测试）。
- `src/server/tests/mod.rs:11` `use crate::config::ConfigManifest`；`:134` `crate::config::installer::FakeInstaller`。
- `tests/f6_observability/fixture.rs:27` `devforge::config::ConfigManifest::builtin()`；`:31` `devforge::config::installer::FakeInstaller`。
- `tests/change_ask_ring.rs` / `tests/change_review_protocol/main.rs:25` 直接 `use devforge::config::manifest::{…}`（走子模块路径）。

## 子模块互指（`crate::config::` 内部的依赖方向）

- `src/config/render.rs:8` `use crate::config::installer::PackageInstaller;`
- `src/config/render.rs:9` `use crate::config::manifest::ConfigManifest;`
- `src/config/render.rs:10` `use crate::config::skill_assets::builtin_skill_files;`
- `src/config/installer.rs` 不依赖其它 config 子模块；`src/config/manifest.rs` 不依赖其它 config 子模块；`src/config/skill_assets.rs` 不依赖其它 config 子模块。
- 因此依赖方向是单向的：`render → {installer, manifest, skill_assets}`，`mod.rs` 只做声明与转发。

## 可重放命令（本轮实跑）

- `TMPDIR=/var/tmp/dfz-src26c/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/var/tmp/dfz-src26c/target cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 0 measured; 550 filtered out; finished in 0.16s`，EXIT=0，从零编译 37.64s（机器相关值）。`1 ignored` 是 `config::manifest::tests::dump_change_agents_fixture`。
- 本文件自身无测试，因此用「整个 config 模块树的测试」作为编译与行为证据。
- 分模块测试计数（`grep -c '#\[test\]' src/config/*.rs`）：`config::installer` 5、`config::manifest` 15（含 1 个 `#[ignore]`）、`config::render` 22、`config::skill_assets` 6 → 合计 48 项 = 47 passed + 1 ignored。
- 复核：本文件全部 7 条非注释行（4 `pub mod` + 3 `pub use`）与转发面 8 个名字的定义行均已逐行对准源码（installer `:12`/`:47`/`:52`、manifest `:413`/`:428`/`:439`/`:462`、render `:29`）。

## 外部依赖

- 本文件不引入任何依赖（无 `use`、无外部 crate）；其子模块的外部依赖见各自单元文档。
