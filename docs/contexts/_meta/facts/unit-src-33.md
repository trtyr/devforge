# 勘察事实 — unit-src-33（src/docgen/impact.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/docgen/impact.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。

## 文件与位置

- `src/docgen/impact.rs`，512 行（HEAD b8fc01e；该文件自 2026-10-20 15:21 起未变，行号对本轮仍成立）。
- 由 `src/docgen/mod.rs:6` `pub mod impact;` 声明；`src/docgen/mod.rs` 无 `pub use`。
- 文件头注释 `:1`-4：增量模式的影响传播（008 T023 / D015 / D021）；只做机械可判定的事；「宁可多算，不可漏算」。
- 导入：`std::collections::{BTreeMap, BTreeSet}`（`:6`）、`std::path::{Path, PathBuf}`（`:7`）、`anyhow::{Context, Result}`（`:9`）、`crate::docgen::inventory::{Inventory, build_inventory}`（`:11`）。

## 公开符号（11 个）

| 符号 | 行 | 形状 |
|---|---|---|
| `ChangedFile` | `:15` | `pub struct`：`kind: char`、`path: String`、`old_path: Option<String>`；derive `Debug, Clone, PartialEq, Eq` |
| `Impact` | `:26` | `pub struct`：`units: Vec<String>`、`assembly_review: Vec<String>`、`notes: Vec<String>`；derive `Debug, Clone, Default, PartialEq, Eq` |
| `ASSEMBLY_PAGES` | `:36` | `pub const [&str; 4]`：`README.md`、`01-architecture.md`、`02-dataflow.md`、`10-runbook.md` |
| `parse_name_status` | `:58` | `pub fn(&str) -> Vec<ChangedFile>` |
| `parse_porcelain` | `:94` | `pub fn(&str) -> Vec<ChangedFile>` |
| `changed_since` | `:133` | `pub fn(repo: &Path, base: &str) -> anyhow::Result<Vec<ChangedFile>>` |
| `units_touching` | `:160` | `pub fn(&Inventory, &[ChangedFile]) -> BTreeSet<String>` |
| `vanished_names` | `:173` | `pub fn(&Inventory, &Inventory, &[ChangedFile]) -> BTreeSet<String>` |
| `reference_files` | `:198` | `pub fn(&Path, &BTreeSet<String>) -> Result<Vec<String>>` |
| `impacted` | `:294` | `pub fn(&Path, &Inventory, &Inventory, &[ChangedFile], &Path) -> Result<Impact>` |
| `plan_impact` | `:360` | `pub fn(&Path, &Path, &[ChangedFile]) -> Result<Impact>` |

## 私有条目

- `SKIP_DIRS` `:44`-`:53`：`[&str; 8]` = `docs`、`.git`、`target`、`.codegraph`、`node_modules`、`dist`、`build`、`.venv`。
- `run_git` `:143`：`std::process::Command::new("git")`，`current_dir(repo)`，非 0 退出 → `anyhow::bail!`（含 stderr）。
- `contains_word` `:213`：词边界匹配（前后字符都不是 `is_alphanumeric()` 或 `_`），循环 `find` 推进 `start = abs + needle.len().max(1)`。
- `is_word_char` `:239`：`c.is_alphanumeric() || c == '_'`。
- `walk` `:244`：递归（`read_dir` + 排序），跳过 `.` 开头名与 `SKIP_DIRS`，只收 `is_source_ext` 扩展名的文件，`read_to_string` 失败静默跳过；回调收到（相对 root、`/` 分隔的路径，全文）。
- `is_source_ext` `:285`-`:291`：23 个扩展名——`rs py ts tsx js jsx go java kt c h cc cpp hpp cs rb php swift m mm scala sh lua`。
- `assembly_pages_mentioning` `:334`：把 unit id 做 `trim_start_matches("unit-").replace('-', "/")` 得 token；对 4 个汇编页逐个读文件（读不到跳过），`text.contains(token)` 或 `contains_word(text, name)` 命中即列入。

## 行为细节（可重放）

- `parse_name_status`：按 `\t` 切；kind 取首字符。`A`/`M`/`D`/`T` → `ChangedFile{kind, path, old_path: None}`（`T` 原样保留）；`R`/`C` → `kind: 'R'`、`path = 新路径`、`old_path = 旧路径`（`C` 被归一成 `'R'`）；其它 kind 丢弃；空行跳过。
- `parse_porcelain`：行长 `< 4` 跳过；`status = &line[..2]`、`path = line[3..].trim()`；含 `R` 或 `C` 且 path 含 `" -> "` → 拆成 old/new，`kind: 'R'`；否则 `status` 含 `A` 或 `?` → `'A'`，含 `D` → `'D'`，其余 `'M'`。
- `changed_since`：跑 `git diff --name-status <base> HEAD` 与 `git status --porcelain`，两组解析结果 `chain` 后按 `path` 放进 `BTreeMap<String, ChangedFile>`（后来者覆盖同 path），返回 `into_values()`（按 path 字典序）。
- `units_touching`：把每个变更文件的 `path` 与 `old_path` 收进集合，取 `inv.entries` 中 `path` 命中者的 `unit_id`。
- `vanished_names`：对变更路径集合，分别取旧/新清单里这些路径的 `canonical_name` 集合，求差集，**过滤 `len() < 2`**。
- `reference_files`：`names` 为空 → 空；否则 `walk(repo, repo, …)` 对每个源码文件做 `names.iter().any(|n| contains_word(text, n))`，命中的相对路径集返回（BTreeSet → 排序 Vec）。
- `impacted`：
  1. `units = units_touching(new_inv, files)`；为空才回退 `units_touching(old_inv, files)`（`:302`-`:305`）。
  2. `names = vanished_names(old, new, files)`；对 `reference_files` 的每个引用方文件，取 `new_inv` 中 `path == rel` 的条目 unit 并入 `units`；查不到 → `notes` 追加「引用方 `{rel}` 不在清单里（可能是新文件或未支持语言）」（`:307`-`:319`）。
  3. `assembly_review = assembly_pages_mentioning(docs_root, &units, &names)`。
  4. `names` 非空 → `notes` 追加「删除/改名的符号：…」。
  5. 返回 `Impact { units: units.into_iter().collect(), assembly_review, notes }`（units 来自 BTreeSet，已排序去重）。
- `plan_impact`：读 `docs_root/_meta/inventory.json` → `Inventory::load`，失败加语境「读旧清单 … 失败（增量需要上次的清单做对照）」；`build_inventory(repo)` 重建新清单；调 `impacted`；若 `units` 为空且 `changes` 非空 → notes 追加「变更文件没有命中任何单元（可能是纯文档/配置改动），不派发单元」。

## 测试（`#[cfg(test)]` `:374`，6 个 `#[test]`，无 `#[ignore]`）

- `:395` `parses_name_status_and_renames`：4 行输入 → 4 条；`R100` 的 kind/old_path/新 path 正确。
- `:405` `parses_porcelain_including_untracked_and_renames`：` M`/`??`/`R  old -> new2` → 3 条，`??` 记 `'A'`，重命名带 old_path。
- `:413` `changed_file_maps_only_to_its_own_unit`：只改 `core/a.py` → 集合恰 1 个单元且不含 `other` 目录的单元。
- `:438` `deleted_symbol_pulls_in_referrers`：旧清单有 `alpha`、新清单变 `alpha_v2`，改 `core/a.py` → notes 含 `alpha`，引用方 `core/b.py` 的单元进集合。
- `:475` `assembly_page_mentioning_changed_unit_is_flagged`：README 提 token → `assembly_review == ["README.md"]`。
- `:500` `plan_impact_without_old_inventory_errors`：无 `_meta/inventory.json` → `Err` 且错误串含 `inventory.json`。

测试夹具：`touch` `:380`（写文件并建父目录）、`repo` `:386`（`core/a.py` 定义 alpha、`core/b.py` 引用 alpha、`other/c.py` 定义 gamma）。

## 消费方

- `src/sop/doc/wiring.rs:170` `changed_since(&workspace, base)`：变更集为空 → 不派发单元（`:171`-176`）；否则 `:177` `plan_impact(&workspace, &workspace.join(DOCS_DIR), &changes)`，成功后 `:184` `incremental::merge_plan(..., &impact, ...)`，并把「受影响单元 N 个 / 汇编复核」写进 resume 说明（`:194`-`:204`）。
- `src/sop/doc/incremental.rs:12` `use crate::docgen::impact::{ASSEMBLY_PAGES, Impact}`：`:109` 起的 `merge_plan`，读 `impact.units`（`:116`）；`:150`-`:151` 把 `units` / `assembly_review` 落进状态；`:193` 用 `ASSEMBLY_PAGES` 做一致性检查。
- 依赖类型：`Inventory` / `build_inventory` / `InventoryEntry`（`src/docgen/inventory.rs`）。
- 外部：唯一有进程的地方是 `changed_since` 调 `git`（`run_git` `:143`）。

## 可重放命令

- `grep -n '^pub fn\|^pub struct\|^pub const' src/docgen/impact.rs`
- `grep -rn 'docgen::impact' --include=*.rs src/` → wiring.rs:170/:177、incremental.rs:12/:244。
