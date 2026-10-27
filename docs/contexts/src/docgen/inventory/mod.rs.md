---
type: module
module_path: src/docgen/inventory/mod.rs
unit: unit-src-34
description: 清单模块的模块面（pub use 层 + 共享导入）——6 个子模块的唯一对外出口，ARC-R6 垂直切分后外部路径不变
---

# src/docgen/inventory/mod.rs

清单（inventory）是反向写文档的**分母**：把仓库机械抽成「一条一条可对账的条目」（D002/D013）。本文件只有 25 行，干两件事：**声明 6 个子模块**并对它们做**统一的 `pub use` 转发**，以及给子模块提供一组**共享导入**（子模块一律 `use super::*;`）。

ARC-R6 之前清单是一个大文件 inventory.rs，按职责垂直切成 `data` / `support` / `scanner` / `closure` / `imports` / `tests` 六个文件（`:2`-`:3` 注释写明「纯移动」「外部路径不变」）。调用方（`src/main.rs`、`src/docgen/audit.rs`、src/sop/doc/…）始终只写 `crate::docgen::inventory::…`，因此**切分对调用方透明**。

## 职责

**负责**：
- 模块面：`pub use` 把 25 个公开名字从 5 个实现子模块重新导出（`:12`-`:16`），使 `crate` 的 `docgen` 模块的 `X` 对全部子模块符号成立。
- 子模块声明：`mod closure / data / imports / scanner / support`（`:18`-`:22`）与测试模块 `mod tests`（`:24`-`:25`）。
- 共享导入：`BTreeMap` / `BTreeSet` / `VecDeque`（`:5`）、`Path` / `PathBuf`（`:6`）、`serde::{Deserialize, Serialize}`（`:8`）、`crate::error::{Context as _, Result}`（`:10`）。子模块用 `use super::*;` 一次性拿全。

**不负责**：不含任何逻辑（无函数、无类型定义、无常量值）；不做路径白名单或 IO；不决定清单内容——那是子模块的事（见下「公开成员」的归属列）。

## 公开成员

`pub use` 逐条转发（名字 → 定义处）：

| 名字 | 来自 | 条目文档 |
|---|---|---|
| `Closure` / `ClosureFinding` / `Coverage` / `EntryKind` / `Inventory` / `InventoryEntry` / `InventoryMeta` / `Lang` / `SOURCE_EXTENSIONS` | `data`（`:12`） | [data.rs.md](data.rs.md) |
| `config_keys` / `env_file_keys` / `is_test_file` / `make_targets` / `package_scripts` / `scan_support_files` / `sql_objects` / `test_specs` | `support`（`:13`） | [support.rs.md](support.rs.md) |
| `assign_units` / `build_inventory` / `is_entry_file` / `public_symbols` / `scan_source_files` | `scanner`（`:14`） | [scanner.rs.md](scanner.rs.md) |
| `check_closures` / `export_glossary` | `closure`（`:15`） | [closure.rs.md](closure.rs.md) |
| `resolve_imports` | `imports`（`:16`） | [imports.rs.md](imports.rs.md) |

子模块可见性：`closure` / `data` / `imports` / `scanner` / `support` 均为**私有** `mod`（`:18`-`:22`），外部只能经上表 25 个名字访问；`tests` 只在 `#[cfg(test)]` 下编译（`:24`），见 [tests.rs.md](tests.rs.md)。

## 依赖

- **上游（谁调用）**：`src/main.rs`（`doc-inventory` / `doc-audit` 子命令）、`src/docgen/audit.rs`（覆盖对账读 `Inventory` / `EntryKind` / `InventoryEntry`）、`src/docgen/impact.rs`（`Inventory` / `build_inventory`）、`src/sop/doc/wiring.rs` 与 `src/sop/doc/incremental.rs`、`src/change/gate0.rs`。全部经 `crate::docgen::inventory::…` 路径，且都指向本文件的转发。
- **下游（它依赖谁）**：6 个子模块 + 标准库 + `serde` + `serde_json`（子模块用）+ `crate` 的 `error`。子模块之间有 3 处直接调用（不经 `pub use`）：`support` → `imports` 的 `norm`（`src/docgen/inventory/support.rs:4`）、`support` → `scanner` 的 `is_skipped_dir`（`:5`）、`tests` → `scanner` 的 `MAX_UNIT_MODULES`（`src/docgen/inventory/tests.rs:4`）。
- **外部依赖**：无网络、无进程；IO 全在子模块（读源文件、读写清单 JSON）。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧用 `tempfile`（`Cargo.toml:27`）。离线用 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 本文件不读任何环境变量或配置。
- **启停 + 健康检查**：`N/A` — 纯库内模块，无进程/端口/健康端点。
- **种子数据**：`N/A` — 输入是仓库磁盘内容与被调方传入的 `Inventory`。
- **测试运行**：`cargo test --lib docgen::inventory`（`TMPDIR`、CARGO_TARGET_DIR 指到仓外即可，见各子模块文档）。预期 `24 passed; 0 failed`——**全部 24 例都在 `src/docgen/inventory/tests.rs`**（`closure` / `data` / `imports` / `scanner` / `support` 五个文件没有内联 `#[test]`）。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即上表 25 个名字。

## 证据与缺口

- 结构事实逐行复核：`mod` 声明 `:18`-`:22`、`tests` `:24`-`:25`、5 条 `pub use` `:12`/`:13`/`:14`/`:15`/`:16`、共享导入 `:5`/`:6`/`:8`/`:10`。
- 无本文件特有的开放问题；实现层的缺口登记在 `_meta/gaps.md` 的 unit-src-34 段（按子模块分条）。
