# 勘察事实 — unit-src-34（src/docgen/inventory/）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/docgen/inventory/*.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。

## 文件与位置

- 目录 `src/docgen/inventory/`：7 个文件、1613 行、7 条 module 清单项 + 31 个 symbol + 24 个 behavior（HEAD b8fc01e；本目录自 2026-10-24 18:47 起未变）。
  - `mod.rs` 25 行、`data.rs` 162 行、`closure.rs` 130 行、`imports.rs` 186 行、`scanner.rs` 429 行、`support.rs` 251 行、`tests.rs` 430 行。
- ARC-R6 从单文件 `inventory.rs` 垂直切分：每个文件首行注释为「（ARC-R6 自 inventory.rs 平移，纯移动。）」，`tests.rs` 为「自 inventory.rs mod tests 平移」。
- `mod.rs` 是唯一模块面：5 条 `pub use`（`:12`-`:16`）+ 5 个私有 `mod`（`:18`-`:22`）+ `#[cfg(test)] mod tests`（`:24`-`:25`）；共享导入 `:5`（BTreeMap/BTreeSet/VecDeque）、`:6`（Path/PathBuf）、`:8`（serde）、`:10`（crate::error）。
- 子模块统一 `use super::*`；额外跨子模块引用：`support.rs:4` `use super::imports::norm;`、`support.rs:5` `use super::scanner::is_skipped_dir;`、`tests.rs:4` `use super::scanner::MAX_UNIT_MODULES;`。

## 公开符号（31）

| 文件 | 符号 | 行 |
|---|---|---|
| data.rs | `SOURCE_EXTENSIONS`（21 条扩展名→Lang） | `:6` |
| data.rs | `Lang`（Python/TsJs/Go/Rust/Other） | `:32` |
| data.rs | `Lang::from_path` | `:42` |
| data.rs | `EntryKind`（Entry/Module/Symbol/Config/Data/Behavior） | `:53` |
| data.rs | `Coverage`（Uncovered/Covered） | `:64` |
| data.rs | `InventoryEntry`（8 字段，line/aliases/unit_id 有 serde 默认） | `:70` |
| data.rs | `InventoryMeta`（root/codegraph_indexed/unsupported_languages/unit_seq） | `:92` |
| data.rs | `Inventory`（meta/entries） | `:102` |
| data.rs | `Closure`（EntryReachability/FileCoverage/SymbolCoverage） | `:108` |
| data.rs | `Closure::as_str` | `:115` |
| data.rs | `ClosureFinding`（closure/path/line/message） | `:125` |
| data.rs | `Inventory::entry_paths`（过滤 Module） | `:133` |
| data.rs | `Inventory::symbol_names` | `:141` |
| data.rs | `Inventory::save`（建父目录 + pretty JSON 覆写） | `:149` |
| data.rs | `Inventory::load` | `:158` |
| closure.rs | `check_closures` | `:6` |
| closure.rs | `export_glossary` | `:98` |
| imports.rs | `resolve_imports` | `:6` |
| scanner.rs | `build_inventory` | `:6` |
| scanner.rs | `scan_source_files` | `:178` |
| scanner.rs | `is_entry_file` | `:226` |
| scanner.rs | `public_symbols` | `:248` |
| scanner.rs | `assign_units` | `:340` |
| support.rs | `config_keys` | `:8` |
| support.rs | `env_file_keys` | `:26` |
| support.rs | `sql_objects` | `:48` |
| support.rs | `make_targets` | `:73` |
| support.rs | `package_scripts` | `:97` |
| support.rs | `scan_support_files` | `:148` |
| support.rs | `is_test_file` | `:183` |
| support.rs | `test_specs` | `:202` |

## 私有条目

- data.rs：`one` `:87`（`line` 的 serde 默认值）。
- closure.rs：无私有 helper（两个函数内联逻辑）。
- imports.rs：`candidate` 闭包 `:13`、`add_relative` `:130`、`cargo_crate_name` `:142`、`resolve_by_suffix` `:159`、`norm`（`pub(super)`）`:183`。
- scanner.rs：`SKIP_DIRS` `:156`（11 项）、`is_skipped_dir`（`pub(super)`）`:173`、`module_name` `:217`、`MAX_UNIT_MODULES`（`pub(super)`）`:337`（=8）、`group_of` `:406`、`sanitize` `:413`、`parse_unit_seq` `:427`。
- support.rs：`keys_after` `:118`、`NAMES`（函数内 const）`:149`。
- tests.rs：`touch` `:5`。

## 行为细节（可重放）

- `build_inventory`：`canonicalize` → `scan_source_files` → 每文件 `mod:` 条目（line 1，名字=文件名）→ `Other` 只登记扩展名 / 其余读文本抽 `sym:`（`public_symbols`）与 `cfg:`（`config_keys`）→ `is_test_file||Rust` 时抽 `spec:`（`test_specs`）→ `is_entry_file` 时 `entry:` 条目 → 支持文件（`.env.example`/`.env.sample`→`cfg:`、`.sql`→`data:`、Makefile→`cmd:`、package.json→`cmd:`）→ `meta{root, codegraph_indexed=.codegraph 目录存在, unsupported_languages, unit_seq=0}` → `assign_units`。
- `assign_units`：分组键 `group_of(path)`（顶层目录，无则 `root`），只分 Module/Config/Data/Behavior；组内按 path 排序、`chunks(8)`；每 chunk 取首个非空 `unit_id` 复用，否则 `next += 1` 生成 `unit-{sanitize(group)}-{next:02}`；最后把 Symbol/Entry 挂到同 path 的 Module 单元；回写 `meta.unit_seq`。
- `check_closures`：① 磁盘文件 ∉ `entry_paths()`；② 非 Other 文件 `public_symbols` ∉ `symbol_names()`；③ 以 `Entry` 条目为种子 BFS（边 = `resolve_imports`），无入口则每文件报一条；`sort_by(closure.as_str, path, line)`。
- `export_glossary`：首行 `# 词表（由 inventory 自动导出，勿手改）`；只取 `Symbol`；按 `unit_id` 分组（BTreeMap）、组内按名字排序；条目 `- \`name\` — \`path:line\`（别名：…）`；空 unit → `## 未分组`；末尾按需 `## 未支持符号抽取的语言（缺口）`。
- `resolve_imports`：Python `from/import`（相对点号上跳）；TS/JS `from '…'`/`require(…)` 且只收 `./`、`../`；Go `import "…"` 与裸字符串行，按末段文件名全仓匹配；Rust `use crate::…`/`use <crate>::…`/`mod x;`；候选消歧顺序：原文件 → 8 种扩展名 → index.ts/index.js/index.tsx/mod.rs → `__init__.py`；结果 BTreeSet 排序。
- `support` 六函数：见 `docs/contexts/src/docgen/inventory/support.rs.md` 的速查表（marker 表、SQL 前缀、Makefile 目标约束、scripts 键、支持文件白名单、测试文件判据、四语言测试名）。
- `save`/`load`：pretty JSON；`save` 先 `create_dir_all(parent)` 再整文件覆写；`load` 只做反序列化，无校验。

## 测试（`tests.rs`，24 个真用例）

`:12` build_inventory_collects_modules_symbols_and_entries、`:27` private_symbols_are_excluded、`:39` other_languages_are_recorded_as_unsupported_gap、`:48` unit_numbering_is_stable_across_runs、`:76` file_coverage_closure_catches_missing_module、`:90` symbol_coverage_closure_catches_missing_symbol、`:103` entry_reachability_follows_python_imports、`:116` unreachable_file_is_reported、`:128` missing_entry_points_reports_every_file、`:139` glossary_is_derived_from_inventory、`:150` inventory_roundtrip_json、`:162` skips_vendor_and_hidden_dirs、`:175` ts_export_symbols_detected、`:194` rust_pub_symbols_detected、`:213` go_exported_symbols_detected、`:234` config_keys_cover_four_languages、`:251` env_example_keys_ignore_comments_and_blank_lines、`:259` sql_ddl_yields_tables_and_views、`:268` make_targets_and_package_scripts_extracted、`:280` convention_entries_land_in_inventory_with_units、`:329` test_specs_extracted_per_language、`:356` test_files_get_behavior_entries、`:382` test_file_detection_covers_common_layouts、`:392` oversized_group_is_split_into_multiple_units。

- `#[test]` 字样共 25 处：`:349` 是 `test_specs_extracted_per_language` 内 Rust 夹具字符串，不是测试属性 → 实际 24 例（`--list` 佐证）。
- 用例夹具：`touch` `:5`；每个用例自建 `tempfile` 临时仓。

## 消费方

- `src/main.rs`：`doc-inventory` / `doc-audit` 子命令（建清单、落盘、闭包校验、词表落盘）。
- `src/docgen/audit.rs`：`Inventory` / `EntryKind` / `InventoryEntry`（覆盖对账）。
- `src/docgen/impact.rs:360`：`build_inventory` 重建新清单；`:361`-`:363` 读旧清单。
- `src/docgen/lint/entities.rs`：`scan_source_files` 建实体索引。
- `src/sop/doc/wiring.rs`：门 0 闭包与词表落盘；`src/sop/doc/incremental.rs`：增量派发读 `Inventory`。
- `src/change/gate0.rs`：单元号与清单。

## 可重放命令

- `wc -l src/docgen/inventory/*.rs`
- `grep -n '^pub fn \|^pub struct \|^pub enum \|^pub const \|^pub(super)' src/docgen/inventory/*.rs`
- `devforge doc-inventory <repo>`（建清单 + 三道闭包）
- `RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外空目录> cargo test --lib docgen::inventory` → 24 passed
