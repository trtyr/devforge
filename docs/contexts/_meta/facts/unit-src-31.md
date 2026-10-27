# 勘察事实 — unit-src-31（src/docgen/audit.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/docgen/audit.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。

## 文件与位置

- `src/docgen/audit.rs`，605 行（HEAD b8fc01e；该文件自 2026-10-19 22:20 起未变，行号对本轮仍成立）。
- 由 `src/docgen/mod.rs:7` `pub mod audit;` 声明；`src/docgen/mod.rs` **没有** re-export（无 `pub use`）。
- 文件头注释 `:1`-4：确定性审计（覆盖对账 + 报告渲染）；与 `docgen::lint` 分工——结构/实体/置信度/Mermaid 在 lint，本模块补「覆盖」。
- 导入：`std::collections::{BTreeMap, BTreeSet}`（`:6`）、`std::path::{Path, PathBuf}`（`:7`）、`crate::docgen::inventory::{EntryKind, Inventory, InventoryEntry}`（`:9`）、`crate::error::{Context as _, Result}`（`:10`）。

## 公开符号（12 个）

| 符号 | 行 | 形状 |
|---|---|---|
| `DocLevel` | `:14` | `pub enum`，变体 `Essential` / `Complete` / `Detailed`；derive `Debug, Clone, Copy, PartialEq, Eq` |
| `DocLevel::parse` | `:24` | `pub fn parse(raw: &str) -> Self`；`trim().to_ascii_lowercase()` 后匹配：`"essential"` → Essential、`"detailed"` → Detailed、**其余一律 Complete** |
| `CoverageMiss` | `:53` | `pub struct`，字段 `id: String`、`kind: EntryKind`、`canonical_name: String`、`path: String`、`line: usize`、`unit_id: String`；derive `Debug, Clone` |
| `CoverageReport` | `:63` | `pub struct`，字段 `required: usize`、`covered: usize`、`missing: Vec<CoverageMiss>`；derive `Debug, Clone, Default` |
| `CoverageReport::passed` | `:70` | `pub fn passed(&self) -> bool`，判据 `self.missing.is_empty()` |
| `CoverageReport::render` | `:75` | `pub fn render(&self) -> String`，1 行头（`:76`-81）+ 每条 miss 一行（`:82`-91）+ 结论一行（`:92`-96） |
| `coverage` | `:113` | `pub fn coverage(inv: &Inventory, docs_dir: &Path, level: DocLevel) -> Result<CoverageReport>` |
| `coverage_for_unit` | `:138` | `pub fn coverage_for_unit(inv: &Inventory, docs_dir: &Path, level: DocLevel, unit: &str) -> Result<CoverageReport>` |
| `runbook_commands` | `:260` | `pub fn runbook_commands(docs_dir: &Path) -> Result<Vec<String>>` |
| `duplicate_mentions` | `:298` | `pub fn duplicate_mentions(docs_dir: &Path, inv: &Inventory) -> Result<Vec<String>>` |
| `duplicated_content` | `:333` | `pub fn duplicated_content(docs_dir: &Path) -> Result<Vec<String>>` |
| `uncovered_units` | `:406` | `pub fn uncovered_units(report: &CoverageReport) -> BTreeSet<String>` |

## 私有条目

- `DocLevel::requires` `:32`-51：档位 → 条目类型集合。`Essential` 只要 `Entry`；`Complete` = Entry/Module/Symbol/Config/Data；`Detailed` = 上述 + Behavior。
- `kind_str` `:101`-110：`EntryKind` → 字面 `entry/module/symbol/config/data/behavior`。
- `DocFile` `:168`：私有结构（`text: String`、`ty: Option<String>`、`heading: Option<String>`）。
- `collect_docs` `:174`：逐篇读 `collect_markdown` 结果，抽 `frontmatter_type` 与首个 `#` 标题（去掉 `#` 并 trim）。
- `frontmatter_type` `:188`：首行必须是 `---`；其后逐行找 `type:`，值去引号并 `to_lowercase()`；遇第二个 `---` 停止。
- `entry_covered` `:211`：`Module` 分支要求「提到 path 或 canonical_name」且（frontmatter type 为 `module`/`模块` 或首个标题含名字/路径），**或**任意文档正文含 `]({path})` 形式的链接；其它 kind 只要求任意文档正文 `contains(canonical_name)`。
- `collect_markdown` `:231`：递归遍历（`stack`），跳过以 `.` 开头的文件/目录，只收 `.md`，末尾 `sort()`；目录不存在 → 空 `Vec`。
- `normalize_line` `:381`：trim → 去行首 `-`/`*`/`>` → 空白合并成单空格。
- `looks_like_index` `:387`：统计 `[` 到 `)` 之间的字符数，`link_chars * 2 >= 总字符数` 判为索引行（重复判定时跳过）。

## 行为细节（可重放）

- `coverage` / `coverage_for_unit`：各自先 `collect_docs`；遍历 `inv.entries`（后者先按 `e.unit_id == unit` 过滤），`level.requires(kind)` 为假则跳过（不计 required）；命中 `entry_covered` 计 covered，否则 push `CoverageMiss`。
- `CoverageReport::render` 输出模板（`:76`-99）：首行 `覆盖审计（引擎）：要求 N 条 / 已覆盖 M 条 / 缺失 K 条`；每条 `- [kind] name 未在文档中出现（path:line，单元 unit）`；结论 `结论：覆盖 100%（该档位要求集合内）。` 或 `结论：覆盖不完整——缺一条即零覆盖，必须补文档或登记缺口（D013）。`
- `runbook_commands`：只扫**文件名**（`file_stem`）小写含 `runbook` 的 `.md`；代码块围栏行 ` ``` ` 切换状态，开启时取 info string，仅 `bash`/`sh`/`console`/`shell`/`zsh` 视为命令块；块内非空且不以 `#` 开头的行按原样收进当前块；块结束时以 `\n` 连接成一个字符串 push。
- `duplicate_mentions`：只处理 `kind == Symbol` 且 `canonical_name.len() >= 4`；逐文件读文本（跳过路径含 `_meta` 组件的文件），`contains` 命中即记；命中文件数 ≥2 才输出 `` `name` 在 N 个文档中重复出现：a, b ``；输出排序。
- `duplicated_content`：跳过 `_meta`；逐行维护 `in_fence`（围栏内整段跳过）；`looks_like_index` 跳过；`normalize_line` 后字符数 ≥40 才收进本篇集合（`BTreeSet`，篇内去重）；跨篇聚合，出现在 ≥2 篇才输出 `同一段落在 N 篇文档中重复：<前 60 字>…（a, b）`。
- `uncovered_units`：取 `report.missing` 的 `unit_id`，过滤空串，收进 `BTreeSet`。

## 测试（`#[cfg(test)]` `:415`，12 个 `#[test]`，无 `#[ignore]`）

- `:446` `coverage_marks_missing_entries`：缺 `orphan_fn` → `!passed()` 且 `render()` 含该名。
- `:457` `full_coverage_requires_modules_and_symbols`：`type: module` 文档覆盖两文件 → passed 且 render 含「覆盖 100%」。
- `:471` `essential_level_only_requires_entry_points`：只写 `src/main.py` 即 passed（Essential）。
- `:480` `unit_scope_only_counts_own_entries`：`coverage_for_unit` 的 required>0 且所有 miss 属该 unit。
- `:498` `uncovered_units_groups_misses`：结果非空且每个都以 `unit-` 开头。
- `:508` `runbook_commands_extracted_from_bash_blocks`：`10-runbook.md` 的 bash 块被抽出 1 条、text 块不抽。
- `:522` `duplicate_mentions_flags_repeated_symbol`：两篇文档写同一符号 → 命中。
- `:532` `duplicate_mentions_ignores_meta_dir`：`_meta/glossary.md` 不参与 → 空。
- `:542` `duplicated_content_flags_same_paragraph`：同一段 60+ 字出现在两篇 → 1 条且含「2 篇文档」。
- `:553` `duplicated_content_ignores_short_lines_and_meta`：短行与 `_meta` 均不计 → 空。
- `:565` `module_mentioned_only_in_prose_is_not_covered`：Runbook 正文提一句不算；标题是模块名算；`[core](src/core.py)` 链接算。
- `:601` `level_parse_defaults_to_complete`：`whatever`→Complete、`essential`→Essential、`detailed`→Detailed。

测试夹具（`fixture` `:420`）：临时目录建 `src/main.py` 与 `src/core.py`（含 `create_note` 与 `orphan_fn`）；`write_doc` `:436` 在 `docs/contexts/` 下按名写文档（自动建父目录）。

## 消费方

- `src/main.rs:40`-94 CLI `devforge doc-audit <docs_dir> --repo <root> [--level essential|complete|detailed]`：`:58` 用 `DocLevel::parse`；`:63` `coverage`；`:66` `duplicate_mentions`；`:68` `duplicated_content`；`:94` 退出码 = `cov.passed() && lint.passed()` ? 0 : 1（缺 docs 参数 → `:45` exit 2）。
- `src/sop/doc/wiring.rs`：`:230` `coverage_for_unit`（单元级审计闭包，level 固定 `Complete` `:233`）；`:248` `runbook_commands`（把命令块写进审查输入）；`:293` `coverage`（终审全量）；`:311` `duplicate_mentions`；`:321` `duplicated_content`。
- 依赖的类型：`Inventory` / `InventoryEntry` / `EntryKind` 定义在 `src/docgen/inventory.rs`（`:67` EntryKind、`:84` InventoryEntry、`Inventory` 同文件）。
- 错误类型：`crate::error::DevforgeError`（经 `Context` trait 加语境）。

## 可重放命令

- `grep -n '^pub fn\|^pub enum\|^pub struct' src/docgen/audit.rs`
- `devforge doc-audit docs/contexts --repo <仓根>`（本仓实跑：覆盖审计 + 同名 + 重复正文 + lint 摘要）
