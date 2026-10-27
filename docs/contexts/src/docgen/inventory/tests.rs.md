---
type: module
module_path: src/docgen/inventory/tests.rs
unit: unit-src-34
description: 清单模块的全部单元测试（24 例）——建清单、排除规则、四语言符号、约定面抽取、闭包三件套、词表、JSON 往返、单元预分配
---

# src/docgen/inventory/tests.rs

清单模块的**唯一测试文件**（`#[cfg(test)] mod tests` 在 `src/docgen/inventory/mod.rs:24`-`:25` 声明）：24 个 `#[test]`，覆盖 `build_inventory` / `scan_source_files` / `public_symbols` / `is_entry_file` 的产出形状，`support` 的六种约定面抽取，`closure` 的三道闭包与词表，以及 `assign_units` 的单元切片与不漂移。文件头注释写明「自 inventory.rs mod tests 平移，纯移动」（`:1`）。

测试全部跑在 `tempfile` 的 `tempdir` 的临时仓里，**不碰真实仓库**、不联网、不起进程；唯一的外部依赖是 `tempfile`（`Cargo.toml:27`）。本文件只被 `cargo test` 编译，不进生产产物。

## 职责

**负责**：
- 夹具：`touch`（`src/docgen/inventory/tests.rs:5`）——在临时仓里按相对路径写文件并自动建父目录。
- 24 个用例：清单构建 4 例、闭包 5 例（含无入口退化）、词表与 JSON 往返 2 例、排除规则 1 例、四语言符号 3 例、约定面 5 例、测试规格 3 例、单元预分配 1 例（逐例见下「用例清单」）。

**不负责**：不测本模块以外的生产代码；不测 `imports` 的四条解析规则的 TS/JS、Go、Rust 分支（只经 Python 用例间接覆盖）；不测网络/进程/CLI 层。

## 公开成员

`N/A` —— 整个文件在 `#[cfg(test)]` 下编译，没有任何生产可用符号；`src/docgen/inventory/mod.rs` 也只在测试构建里声明它（`src/docgen/inventory/mod.rs:24`-`:25`）。

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `touch` | `src/docgen/inventory/tests.rs:5` | 夹具：建父目录 + 写文件（`create_dir_all` + `write`，都 `unwrap`） |

## 依赖

- **上游（谁调用）**：`cargo test`（`src/docgen/inventory/mod.rs:24` 的 `#[cfg(test)] mod tests;`）。
- **下游（它依赖谁）**：`use super::*`（`:3`，拿到 `src/docgen/inventory/mod.rs` 转发的全部清单 API 与 `Path` / `Lang` / `EntryKind` 等）；`use super::scanner::MAX_UNIT_MODULES`（`:4`，断言单元容量上限 8）；`tempfile`（每个用例自建临时仓）。
- **外部依赖**：无网络、无进程；只读写 `tempfile` 临时目录。

## 核心口径（解释层，逐条带置信度）

- 🟢 **24 个用例，全部在临时仓**：`tempfile` 的 `tempdir` 出现在每个用例开头（如 `:13`、`:28`、`:40`、`:49`…），`touch` 负责铺文件（`:5`-`:9`）；没有任何用例写仓内路径。🟢 CONFIRMED `src/docgen/inventory/tests.rs:5`、`:13`
- 🟢 **文件里有 25 处 `#[test]` 字样，但只有 24 个真用例**：`test_specs_extracted_per_language` 把 `#[test]` 当作**夹具字符串**写进临时源文件（`:349`），它不是测试属性。数用例请以 `cargo test -- --list` 的 24 为准。🟢 CONFIRMED `src/docgen/inventory/tests.rs:349`
- 🟢 **断言方向是「产出里有/没有」，不是精确全量**：多数用例用 `assert!(entries.iter().any(...))` / `!…any(...)`（如 `:17`-`:24`、`:30`-`:35`、`:43`-`:45`），只有少数做 `assert_eq!` 精确比对（约定面抽取的键序列、`sql_objects` 名字序列、单元数 2）。🟢 CONFIRMED `src/docgen/inventory/tests.rs:17`、`:251`、`:259`、`:401`
- 🟢 **闭包用例靠「手工删条目」造错**：`file_coverage_closure_catches_missing_module` 用 `entries` 的 `retain` 删掉删除目标文件的 Module 条目（`:79`），`symbol_coverage_closure_catches_missing_symbol` 同理删符号（`:93`-`:95`）——不依赖磁盘变化，因此与文件系统时序无关。🟢 CONFIRMED `src/docgen/inventory/tests.rs:79`、`:93`
- 🟢 **单元预分配有两条硬断言**：① 同一份清单再调一次 `assign_units`，`(id, unit_id)` 序列逐项相同（`:54`-`:66`）；② 12 个模块必须切成 2 个单元且每个单元 ≤ `MAX_UNIT_MODULES`（`:392`-`:429`）。🟢 CONFIRMED `src/docgen/inventory/tests.rs:54`、`:401`、`:424`
- 🟢 **「条目必须挂单元」是全局不变量**：`convention_entries_land_in_inventory_with_units` 断言**所有**条目 `unit_id` 非空（`:297`-`:310`），`test_files_get_behavior_entries` 断言 spec: 条目也非空（`:366`-`:373`）——这是「约定面条目不能永远无人覆盖」的守卫。🟢 CONFIRMED `src/docgen/inventory/tests.rs:297`、`:366`
- 🟡 **没有任何用例覆盖失败路径**：`build_inventory` 的「读源文件失败 → `Err`」、`check_closures` 的 canonicalize 失败，都需要构造不可读文件/非法根，仓内没有对应用例；这类行为只能靠读代码确认。🟡 INFERRED（依据：24 例全部只断言 `Ok` 分支，全文无 `unwrap_err`——除 `plan_impact` 那个用例在 `src/docgen/impact.rs`）
- 🟡 **`imports` 的 TS/JS、Go、Rust 三条分支无用例**：可达性三例全部用 Python 源码（`from core import a` 形态，`:104`-`:107`、`:117`-`:119`），因此 `resolve_imports` 的其它三分支只被「编译通过」验证。🟡 INFERRED（依据：`:103`-`:137` 的夹具只有 .py 文件）

## 用例清单（24 例）

| # | 用例 | 钉住的行为 | 位置 |
|---|---|---|---|
| 1 | `build_inventory_collects_modules_symbols_and_entries` | 入口条目 / 符号条目 / 私有符号不入清单 | `src/docgen/inventory/tests.rs:12` |
| 2 | `private_symbols_are_excluded` | 下划线开头符号全部排除 | `:27` |
| 3 | `other_languages_are_recorded_as_unsupported_gap` | `Other` 语言进 `unsupported_languages` 且仍有模块条目 | `:39` |
| 4 | `unit_numbering_is_stable_across_runs` | 重复 `assign_units` 不漂移；符号继承模块单元 | `:48` |
| 5 | `file_coverage_closure_catches_missing_module` | 文件覆盖闭包抓缺失模块 | `:76` |
| 6 | `symbol_coverage_closure_catches_missing_symbol` | 符号覆盖闭包抓缺失符号（消息含名字） | `:90` |
| 7 | `entry_reachability_follows_python_imports` | 沿 import 可达时不报可达性 | `:103` |
| 8 | `unreachable_file_is_reported` | 孤立文件被报不可达 | `:116` |
| 9 | `missing_entry_points_reports_every_file` | 无入口时退化报「没有入口点」 | `:128` |
| 10 | `glossary_is_derived_from_inventory` | 词表标题 / 名字 / path:line 证据指针 | `:139` |
| 11 | `inventory_roundtrip_json` | `save` → `load` 往返，条目数与水位不变 | `:150` |
| 12 | `skips_vendor_and_hidden_dirs` | 跳过 `node_modules` 与点目录 | `:162` |
| 13 | `ts_export_symbols_detected` | `export function` / `export class` 抽得到，非导出抽不到 | `:175` |
| 14 | `rust_pub_symbols_detected` | `pub fn` / `pub struct` 抽得到，私有抽不到 | `:194` |
| 15 | `go_exported_symbols_detected` | 首字母大写函数（含方法接收者）抽得到 | `:213` |
| 16 | `config_keys_cover_four_languages` | 四语言环境变量 marker 各抽一个键 | `:234` |
| 17 | `env_example_keys_ignore_comments_and_blank_lines` | 注释/空行/无等号行跳过，行号是原文件行号 | `:251` |
| 18 | `sql_ddl_yields_tables_and_views` | table（含 `if not exists`）与 view 都抽，名字小写 | `:259` |
| 19 | `make_targets_and_package_scripts_extracted` | Makefile 目标只取 `all`/`run`（`VAR = 1` 排除）；package.json scripts 取 `dev`/`build` | `:268` |
| 20 | `convention_entries_land_in_inventory_with_units` | 配置/数据/行为条目落库 + 全部条目有单元 + 同文件同单元 | `:280` |
| 21 | `test_specs_extracted_per_language` | Python / TS-JS / Go / Rust 四种测试名抽取 | `:329` |
| 22 | `test_files_get_behavior_entries` | 测试文件生成 spec: 条目且挂到单元 | `:356` |
| 23 | `test_file_detection_covers_common_layouts` | 六种布局的测试文件判定（正 4 / 负 2） | `:382` |
| 24 | `oversized_group_is_split_into_multiple_units` | 12 模块切 2 单元、每单元 ≤ 8、重复调用不漂移 | `:392` |

## 测试覆盖

`N/A`（本文件就是用例本体）。它覆盖的生产代码见各模块文档的「测试覆盖」表：[scanner.rs.md](scanner.rs.md)、[support.rs.md](support.rs.md)、[closure.rs.md](closure.rs.md)、[imports.rs.md](imports.rs.md)、[data.rs.md](data.rs.md)。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；显式 `RUSTUP_TOOLCHAIN=stable` 可避开「无默认 toolchain」。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；`tempfile`（`Cargo.toml:27`）；离线 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 无用例读环境变量；`TMPDIR` 仅决定临时仓落点。
- **启停 + 健康检查**：`N/A` — 无进程/端口；「健康」= 24 例全绿。
- **种子数据**：`N/A` — 每例自建临时仓（`touch` 铺文件），无共享种子。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::inventory
  ```

  预期（机器相关）：`24 passed; 0 failed; 0 ignored`。单例运行：`cargo test --lib docgen::inventory::tests::oversized_group_is_split_into_multiple_units`。列全部用例：加 `-- --list`（应得 24 条）。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点；本文件不导出任何生产符号）。

## 证据与缺口

- 结构事实逐行复核：头注释 `:1`、`use` `:3`-`:4`、`touch` `:5`、24 个 `#[test]` 分别起于 `:12`/`:27`/`:39`/`:48`/`:76`/`:90`/`:103`/`:116`/`:128`/`:139`/`:150`/`:162`/`:175`/`:194`/`:213`/`:234`/`:251`/`:259`/`:268`/`:280`/`:329`/`:356`/`:382`/`:392`；夹具字符串里的 `#[test]` 在 `:349`。
- 本轮（unit-src-34）实跑：`cargo test --lib docgen::inventory` → `24 passed; 0 failed; 0 ignored; 595 filtered out`，EXIT=0；`-- --list` 显示 `24 tests, 0 benchmarks`。
- 开放问题登记在 `_meta/gaps.md` 的 unit-src-34 段（失败路径无用例、`imports` 三条分支无用例）。
