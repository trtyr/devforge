---
type: module
module_path: src/docgen/audit.rs
unit: unit-src-31
description: 确定性覆盖审计——按文档档位对账 inventory 每一条的文档落点，另提供 Runbook 命令抽取与跨模块重复检测
---

# src/docgen/audit.rs

doc-from-zero 的**机械审计层**（P008 D013/阶段 4）：结构 / 实体 / 置信度 / Mermaid 由 docgen::lint 模块 管，本文件只补「覆盖」——清单（`_meta/inventory.json`）每一条在 `docs/contexts/` 下有没有落点，并按档位决定要求哪些条目类型。另有三件终审辅助：Runbook 命令抽取、同名跨文档出现、跨模块重复正文。

全部是纯函数 + 文件读取：无状态、不写盘、无网络、无进程。结果作为确定性证据注入审查输入（`src/main.rs:40`-94 的 CLI 与 `src/sop/doc/wiring.rs:230`/`:293` 的审查闭包）。

## 职责

**负责**：
- 档位语义：`DocLevel`（`:14`）与 `parse`（`:24`）——`essential` / `complete` / `detailed` 三档，未识别值回落 `complete`。
- 覆盖对账：`coverage`（`:113`，全清单）与 `coverage_for_unit`（`:138`，只算某单元）产出 `CoverageReport`。
- 覆盖判定：`entry_covered`（`:211`，私有）定义「算不算被文档覆盖」，含模块自报家门与链接两种放行路径。
- 报告渲染：`render`（`:75`）+ `passed`（`:70`）。
- 终审辅助：`runbook_commands`（`:260`）、`duplicate_mentions`（`:298`）、`duplicated_content`（`:333`）、`uncovered_units`（`:406`）。

**不负责**：不生成清单（清单模块 docgen::inventory）；不做结构/实体/链接/Mermaid 检查（docgen::lint 模块，正随 ARC-R6 拆分）；不写任何文件（只读 `docs_dir`）；不做影响面分析（`src/docgen/impact.rs`）；不跑 Runbook 命令——只把它们抽出来交给审查官执行。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `DocLevel` | 枚举（3 变体） | [DocLevel](#doclevel) |
| `parse` | 关联函数 | [parse](#parse) |
| `CoverageMiss` | 结构体 | [CoverageMiss](#coveragemiss) |
| `CoverageReport` | 结构体 | [CoverageReport](#coveragereport) |
| `passed` | 方法 | [passed](#passed) |
| `render` | 方法 | [render](#render) |
| `coverage` | 函数 | [coverage](#coverage) |
| `coverage_for_unit` | 函数 | [coverage_for_unit](#coverage_for_unit) |
| `runbook_commands` | 函数 | [runbook_commands](#runbook_commands) |
| `duplicate_mentions` | 函数 | [duplicate_mentions](#duplicate_mentions) |
| `duplicated_content` | 函数 | [duplicated_content](#duplicated_content) |
| `uncovered_units` | 函数 | [uncovered_units](#uncovered_units) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `requires` | `src/docgen/audit.rs:32` | 档位 → 要求集合（见 [档位与要求集合](#档位与要求集合)），私有 |
| `kind_str` | `src/docgen/audit.rs:101` | `EntryKind` → 报告里的字面 kind，私有 |
| `DocFile` | `src/docgen/audit.rs:168` | 一篇文档的 `text` / `ty`（frontmatter type）/ `heading`（首个标题），私有 |
| `collect_docs` / `frontmatter_type` | `src/docgen/audit.rs:174` / `:188` | 读文档并抽 frontmatter type 与首个标题，私有 |
| `entry_covered` | `src/docgen/audit.rs:211` | 单条覆盖判定（本文件的规则核心），私有 |
| `collect_markdown` | `src/docgen/audit.rs:231` | 递归收集 md 文件（跳过以 `.` 开头的文件与目录，结果排序），私有 |
| `normalize_line` / `looks_like_index` | `src/docgen/audit.rs:381` / `:387` | 重复正文的行归一化与索引行豁免，私有 |

## 依赖

- **上游（谁调用）**：
  - `src/main.rs:40`-94：CLI `devforge doc-audit <docs_dir> --repo <root> [--level essential|complete|detailed]`——`:58` 解析档位、`:63` 全量覆盖、`:66` 同名、`:68` 重复正文；`:94` 退出码 = 覆盖与 lint 都通过才 0。
  - `src/sop/doc/wiring.rs:230`（`coverage_for_unit`，单元级审计闭包，档位固定 `Complete` `:233`）、`:248`（`runbook_commands` 注入审查输入）、`:293`（终审 `coverage`）、`:311`（`duplicate_mentions`）、`:321`（`duplicated_content`）。
- **下游（它依赖谁）**：`crate::docgen::inventory::{EntryKind, Inventory, InventoryEntry}`（清单模块，ARC-R6 已把单文件拆成 src/docgen/inventory/ 目录，行号不稳定，故只给符号）；`crate::error::{Context, Result}`。标准库：`BTreeMap` / `BTreeSet` / `Path` / `PathBuf` / 标准库 fs。
- **外部依赖**：无网络、无进程；测试用 `tempfile`（`Cargo.toml:27`）。只读 `docs_dir`，不写。

## 核心口径（解释层，逐条带置信度）

- 🟢 **「缺一条即零覆盖」**：判据在 `passed()`——只看 missing.is_empty()（`src/docgen/audit.rs:70`-72），没有百分比阈值；对应文案在 `render` 的不通过分支（`:95`）。🟢 CONFIRMED `src/docgen/audit.rs:70`、`:95`
- 🟢 **档位只缩分母，不降判据**：`coverage` 对 `requires` 为假的条目直接跳过（`:117`-`:119`），因此 `essential` 档的 `required` 只含入口点；同一份文档在低档位可能 100%、在高档位缺 behavior。🟢 CONFIRMED `src/docgen/audit.rs:32`-51、`:117`
- 🟢 **模块覆盖需要「自报家门」或「被链接」**：`Module` 条目要求「正文提到 path 或 canonical_name」且（frontmatter `type: module`/`模块` 或首个标题含名字/路径），或者任意文档里出现指向该模块路径的 Markdown 链接（`:211`-`:229`）。只在 Runbook 正文提一句不算——这正是单测 `module_mentioned_only_in_prose_is_not_covered`（`:565`）钉住的语义。🟢 CONFIRMED `src/docgen/audit.rs:211`-229
- 🟢 **非模块条目只做子串匹配**：其余 kind 用 `text.contains(canonical_name)`（`:227`），既不要求「本模块文档」，也不做词边界——兄弟模块文档里提到一次就算覆盖。🟢 CONFIRMED `src/docgen/audit.rs:227`
- 🟢 **重复正文按「行」判，不按段**：`duplicated_content` 逐行归一化（`:344`）、字符数 ≥40 才参与（`:357`），跨 ≥2 篇非 `_meta` 文档才报（`:333`-`:380`）；围栏代码块内整段跳过（`:350`-`:352`），索引行（链接字符占多数）跳过（`:353`、`:387`）。函数注释里叫「同一段落」，实现单位是物理行。🟢 CONFIRMED `src/docgen/audit.rs:344`、`:352`、`:357`、`:367`
- 🟢 **同名出现是提示不是判决**：`duplicate_mentions` 只扫 `Symbol` 且名字长度 ≥4（`:302`），`_meta` 目录整篇跳过（`:307`-`:310`）；CLI 与终审都把它标成「提示级，不单独判 FAIL」（`src/main.rs:73`、`src/sop/doc/wiring.rs:315`）。🟢 CONFIRMED `src/docgen/audit.rs:298`-332
- 🟢 **Runbook 命令只从「文件名含 runbook」的文档抽**：`runbook_commands` 用 `file_stem` 小写含 `runbook` 过滤（`:263`-`:270`），只认 `bash`/`sh`/`console`/`shell`/`zsh` 围栏（`:284`-`:285`），块内空行与 `#` 注释行丢弃（`:289`-`:290`）。🟢 CONFIRMED `src/docgen/audit.rs:260`-296
- 🟡 **`DocLevel` 与 `src/change/gate0.rs` 里的同名枚举 是两个同名枚举**：门 0 的版本有 `None` 变体且 `parse` 返回 `Result`（unknown 即错，`src/change/gate0.rs:24`-39）；本文件的版本没有 `None`、unknown 静默回落 `Complete`。调用方混用时语义不同——落差登记在 `_meta/gaps.md`。🟡 INFERRED（依据：两处枚举定义与 `parse` 签名对照）

## 档位与要求集合

| 档位 | `parse` 的输入 | 要求哪些 `EntryKind` | 典型用途 |
|---|---|---|---|
| `Essential` | `essential` | `Entry` | 只要入口点能跑能接 |
| `Complete`（默认） | 其它任何值（含 `complete`、拼错、空串） | `Entry` / `Module` / `Symbol` / `Config` / `Data` | 常规逐单元审查与终审（`src/sop/doc/wiring.rs:233`、`:296`） |
| `Detailed` | `detailed` | 上述 + `Behavior` | 需要把 behavior 条目也钉住时 |

`EntryKind` 有 6 个变体（`Entry` / `Module` / `Symbol` / `Config` / `Data` / `Behavior`，见清单模块），三档分别是它的前 1 / 5 / 6 个。

## 数据模型（`CoverageReport` 与输出形状）

`CoverageReport`（`src/docgen/audit.rs:63`）是纯内存值，不落盘；生命周期仅限一次审计调用。

| 字段 | 类型 | 约束 / 可空 | 语义 |
|---|---|---|---|
| `required` | `usize` | 恒 ≥ `covered` | 本档位要求集合的条目数（被 `requires` 跳过的条目不计） |
| `covered` | `usize` | ≤ `required` | 命中文档的条目数 |
| `missing` | `Vec<CoverageMiss>` | 可空（空 = 通过） | 未命中条目，顺序 = inv.entries 顺序（`coverage`）或该单元条目顺序（`coverage_for_unit`） |

`CoverageMiss`（`:53`）字段：`id`、`kind`、`canonical_name`、`path`、`line`、`unit_id`——与 `InventoryEntry` 对应字段一一拷贝。

`render()` 的三种行形状（`:76`-`:99`）：

```text
覆盖审计（引擎）：要求 46 条 / 已覆盖 46 条 / 缺失 0 条
- [symbol] foo 未在文档中出现（src/a.rs:12，单元 unit-src-01）
结论：覆盖 100%（该档位要求集合内）。
```

- **索引 / 唯一约束**：`N/A`（内存结构，无存储层）。
- **迁移注意**：无版本字段；字段增删只影响 `render()` 文本与调用方（CLI 打印、审查输入字符串），没有序列化契约。

---

### DocLevel

- **职责**：文档档位枚举，决定 `coverage` 的分母（要求哪些 `EntryKind`）。**不做**校验——非法字符串不报错，回落 `Complete`（`:24`-`:30`）。
- **构造 / 初始化**：`DocLevel::parse(raw)`（[parse](#parse)）是唯一入口；也可在 Rust 里直接写变体字面量（`Copy` 类型）。无 `Default` 实现。
- **公开成员**：三个变体 `Essential` / `Complete` / `Detailed`；唯一的公开方法是 `parse`。私有方法 `requires(kind)`（`:32`）承载档位→类型集合的映射。
- **生命周期**：`Copy` 值对象，随调用栈传递；无状态、无生命周期参数。
- **典型用法**：CLI 从 `--level` 读字符串（`src/main.rs:55`-59）；SOP 审查闭包固定用 `Complete`（`src/sop/doc/wiring.rs:233`、`:296`）。

### parse

- **摘要**：把档位字符串解析成 `DocLevel`，未识别值一律 `Complete`。（🟢 CONFIRMED `src/docgen/audit.rs:24`）
- **行为语义**：`raw.trim().to_ascii_lowercase()` 后精确匹配 `essential` / `detailed`；**其它所有输入**（含 `complete`、`full`、`""`、拼错）→ `Complete`。幂等、无副作用。
- **参数**：`raw: &str`。
- **返回**：`DocLevel`（不是 `Result`——没有失败路径）。
- **异常**：`N/A`（不返回 `Result`）。
- **示例**：`DocLevel::parse("whatever") == DocLevel::Complete`；`DocLevel::parse("essential") == DocLevel::Essential`（用例 `src/docgen/audit.rs:601`）。

### CoverageMiss

- **职责**：一条未覆盖记录的不可变快照（把 `InventoryEntry` 的相关字段拷出来，避免报告持有清单引用）。
- **构造 / 初始化**：只在 `coverage` / `coverage_for_unit` 内部构造（`:124`-`:131`、`:154`-`:161`）；外部只能读字段。
- **公开成员**：`id` / `kind` / `canonical_name` / `path` / `line` / `unit_id`，全部 `pub` 且不可变（无 `&mut` 方法）。
- **生命周期**：随 `CoverageReport` 一起活到调用结束；不落盘。
- **典型用法**：`report.missing.iter().map(|m| m.unit_id.clone())`（`uncovered_units` 的做法，`:406`-`:413`）。

### CoverageReport

- **职责**：一次覆盖对账的结果聚合：分母、命中数、缺失明细。**不做**渲染之外的加工（无过滤/排序逻辑，miss 顺序即遍历顺序）。
- **构造 / 初始化**：由 derive 的 Default 在 `coverage` / `coverage_for_unit` 起点创建（`:115`、`:145`），随后累加。
- **公开成员**：`required` / `covered` / `missing`（均 `pub`）；行为方法 `passed` / `render`。
- **生命周期**：值对象，随调用返回给 CLI / 审查闭包；`render()` 产出字符串进审查输入。
- **典型用法**：`let rep = coverage(&inv, &docs, DocLevel::Complete)?; print!("{}", rep.render());`（`src/main.rs:63`-64）。

### passed

- **摘要**：覆盖是否全部命中。（🟢 CONFIRMED `src/docgen/audit.rs:70`）
- **行为语义**：self.missing.is_empty()——没有阈值、不看 covered 与 required 的比例。纯读、无副作用。
- **参数**：`&self`（无显式参数）。
- **返回**：`bool`。
- **异常**：`N/A`。
- **示例**：`assert!(!rep.passed())`（用例 `:446`-`:455`）；CLI 用它决定退出码（`src/main.rs:94`）。

### render

- **摘要**：把报告渲染成人读文本（确定性事实，不是建议）。（🟢 CONFIRMED `src/docgen/audit.rs:75`）
- **行为语义**：输出 1 行头（`:76`-`:81`）+ 每条 miss 1 行（`:82`-`:91`）+ 1 行结论（分支在 `:92`-`:96`：通过写 `:93`，不通过写 `:95`）。不含时间戳/随机量，同输入同输出。
- **参数**：`&self`。
- **返回**：`String`（以 `\n` 结尾）。
- **异常**：`N/A`。
- **示例**：`assert!(rep.render().contains("覆盖 100%"))`（用例 `:457`-`:469`）；`assert!(rep.render().contains("orphan_fn"))`（用例 `:446`）。

### coverage

- **摘要**：对整个清单做覆盖对账。（🟢 CONFIRMED `src/docgen/audit.rs:113`）
- **行为语义**：先 `collect_docs`（读全部 md 文件，含 frontmatter type 与首个标题）；遍历 inv.entries，`level.requires(kind)` 为假则跳过，否则 `required += 1` 并判 `entry_covered`；未命中 push 一条 `CoverageMiss`。只读，不写盘。
- **参数**：`inv: &Inventory`、`docs_dir: &Path`（如 `<仓>/docs/contexts`）、`level: DocLevel`。
- **返回**：`Result<CoverageReport>`。
- **异常**：`Result` 的 `Err` 只在读文档失败时出现（`collect_docs` 对每个文件 `read_to_string` 加 `Context`，`:174`-`:186`）；目录不存在不算错——`collect_markdown` 返回空集（`:233`-`:235`），于是全清单都缺。
- **示例**：`devforge doc-audit docs/contexts --repo .`（`src/main.rs:40`-94）；单元测试 `:446`。

### coverage_for_unit

- **摘要**：只对某个单元的条目做覆盖对账（逐单元审查用）。（🟢 CONFIRMED `src/docgen/audit.rs:138`）
- **行为语义**：与 `coverage` 同构，差别只在遍历前 `filter(|e| e.unit_id == unit)`（`:146`）；`required` 只计该单元且满足档位的条目。
- **参数**：`inv: &Inventory`、`docs_dir: &Path`、`level: DocLevel`、`unit: &str`（单元号，如 `unit-src-30`）。
- **返回**：`Result<CoverageReport>`。
- **异常**：同 `coverage`（读文档失败）；未知单元号返回 `required = 0` 的报告（不报错）。
- **示例**：`src/sop/doc/wiring.rs:230`-237 把 cov.render() 拼进单元审查输入；用例 `:480`。

### runbook_commands

- **摘要**：抽取 Runbook 文档里的可执行命令块，供审查官「从零 boot」时逐条跑。（🟢 CONFIRMED `src/docgen/audit.rs:260`）
- **行为语义**：只扫文件名（`file_stem`）小写含 `runbook` 的 md 文件（`:263`-`:270`）；围栏行切换块状态，开启时只认 info string ∈ {bash, sh, console, shell, zsh}（`:284`-`:285`）；块内空行与 `#` 开头行丢弃（`:289`-`:290`）；一个块拼成一条字符串（行间 `\n`）。只读。
- **参数**：`docs_dir: &Path`。
- **返回**：`Result<Vec<String>>`——每个非空命令块一项。
- **异常**：读文件失败 → `Err`（带文件名语境，`:271`）。
- **示例**：10-runbook.md 里一个 bash 块 + 一个 text 块 → `cmds.len() == 1` 且包含 `pip install`（用例 `:508`-`:520`）；`src/sop/doc/wiring.rs:248`-255 把每条包成 ``` 围栏写进审查输入。

### duplicate_mentions

- **摘要**：同一 `Symbol` 名出现在 ≥2 篇（非 `_meta`）文档里 → 报「重复提及」。（🟢 CONFIRMED `src/docgen/audit.rs:298`）
- **行为语义**：只看 `kind == Symbol` 且 `canonical_name.len() >= 4`（`:302`-`:304`）；逐文件 `read_to_string` + `contains`，路径含 `_meta` 组件的文件整体跳过（`:307`-`:312`）；命中文件数 ≥2 才输出 `` `name` 在 N 个文档中重复出现：a, b ``；结果排序（`:326`）。读失败的文件被静默跳过。
- **参数**：`docs_dir: &Path`、`inv: &Inventory`。
- **返回**：`Result<Vec<String>>`（每条一个符号）。
- **异常**：目录层面的错误不会出现（`collect_markdown` 对不存在的目录返回空集，`:233`-`:235`），单个文件读失败也被 `if let Ok(..)` 静默跳过（`:311`）——因此当前实现不会产出 `Err`，签名保留 `Result` 仅供上层统一 `?`（见 `_meta/gaps.md`）。
- **示例**：两篇文档都写 `create_note` → 命中（用例 `:522`）；`_meta/glossary.md` 写同一名字 → 不命中（用例 `:532`）。

### duplicated_content

- **摘要**：归一化后 ≥40 字且逐字相同的行出现在 ≥2 篇非 `_meta` 文档 → 报「同一段落重复」。（🟢 CONFIRMED `src/docgen/audit.rs:333`）
- **行为语义**：逐文件逐行（`:344`）；围栏代码块内整段跳过（`:350`-`:352`）；索引行（链接字符占多数，`looks_like_index` `:387`）跳过（`:353`）；`normalize_line`（去 `-`/`*`/`>` 前缀、空白合并，`:381`）后字符数 ≥40 才参与（`:357`），篇内用 `BTreeSet` 去重（`:342`、`:358`）；跨篇聚合后只保留出现在 ≥2 篇的（`:367`），预览截前 60 字（`:369`）。只读。
- **参数**：`docs_dir: &Path`。
- **返回**：`Result<Vec<String>>`。
- **异常**：读失败的文件静默跳过（`:339`-`:341`）；签名 `Result` 但当前实现不会产出 `Err`。
- **示例**：同一段 60+ 字出现在两篇 → 1 条且含「2 篇文档」（用例 `:542`）；短行与 `_meta` 不计（用例 `:553`）。

### uncovered_units

- **摘要**：从报告里汇总「有缺失条目的单元号」。（🟢 CONFIRMED `src/docgen/audit.rs:406`）
- **行为语义**：取 report.missing 的 `unit_id`，过滤空串（未分组条目，`:412`），收进 `BTreeSet`（去重 + 字典序）。纯函数。
- **参数**：`report: &CoverageReport`。
- **返回**：`BTreeSet<String>`——空集表示所有单元都覆盖。
- **异常**：`N/A`。
- **示例**：空文档目录下 `uncovered_units(&rep)` 非空且每个都以 `unit-` 开头（用例 `:498`）。

## 测试覆盖（`#[cfg(test)]` `:415`，12 例，全绿）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `coverage_marks_missing_entries` | 缺条目进 `missing` 且出现在报告文本 | `src/docgen/audit.rs:446` |
| `full_coverage_requires_modules_and_symbols` | `type: module` 文档 + 符号名 → 100% | `src/docgen/audit.rs:457` |
| `essential_level_only_requires_entry_points` | Essential 只要求入口点 | `src/docgen/audit.rs:471` |
| `unit_scope_only_counts_own_entries` | 单元级分母只含本单元 | `src/docgen/audit.rs:480` |
| `uncovered_units_groups_misses` | 缺失单元号去重汇总 | `src/docgen/audit.rs:498` |
| `runbook_commands_extracted_from_bash_blocks` | 只抽 bash 围栏、不抽 text | `src/docgen/audit.rs:508` |
| `duplicate_mentions_flags_repeated_symbol` | 两篇同符号 → 命中 | `src/docgen/audit.rs:522` |
| `duplicate_mentions_ignores_meta_dir` | `_meta` 不参与 | `src/docgen/audit.rs:532` |
| `duplicated_content_flags_same_paragraph` | 同段两篇 → 1 条 | `src/docgen/audit.rs:542` |
| `duplicated_content_ignores_short_lines_and_meta` | 短行与 `_meta` 豁免 | `src/docgen/audit.rs:553` |
| `module_mentioned_only_in_prose_is_not_covered` | 正文提及不算 / 标题算 / 链接算 | `src/docgen/audit.rs:565` |
| `level_parse_defaults_to_complete` | 未知档位回落 Complete | `src/docgen/audit.rs:601` |

测试夹具：`fixture`（`:420`）在临时目录建两个 Python 源文件（一个调 `create_note`、一个定义 `create_note` 与无人调用的 `orphan_fn`），`write_doc`（`:436`）在 `docs/contexts/` 下写文档并自动建父目录——所有用例都跑在临时目录里，不碰真实仓。

## 测试运行与环境（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；测试用 `tempfile`（`Cargo.toml:27`）。离线用 CARGO_NET_OFFLINE=true。
- **配置项**：`N/A` — 无环境变量/CLI 开关读进本文件；档位由调用方以参数传入（CLI 的 `--level` 在 `src/main.rs:55`-59 解析）。
- **启停 + 健康检查**：`N/A` — 库内纯函数模块，无进程/端口/健康端点；进程级入口是 CLI 子命令（见下）。
- **种子数据**：`N/A` — 输入是 `Inventory`（由清单模块 docgen::inventory 生成）与 `docs_dir` 下的 Markdown，无内置种子。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::audit
  ```

  预期（机器相关）：`12 passed; 0 failed`。
- **公开接口清单**：
  - CLI：`devforge doc-audit <docs_dir> --repo <root> [--level essential|complete|detailed]`（`src/main.rs:40`-94）；缺 `docs_dir` → 退出码 2（`:45`）；覆盖或 lint 任一不通过 → 退出码 1（`:94`）。
  - Rust 面：上表 12 个名字，均从 crate::docgen::audit 全路径调用（`src/docgen/mod.rs` 无 re-export）。

## 证据与缺口

- 结构事实逐行复核：`DocLevel` `:14`、`parse` `:24`、`requires` `:32`、`CoverageMiss` `:53`、`CoverageReport` `:63`、`passed` `:70`、`render` `:75`、`kind_str` `:101`、`coverage` `:113`、`coverage_for_unit` `:138`、`DocFile` `:168`、`collect_docs` `:174`、`frontmatter_type` `:188`、`entry_covered` `:211`、`collect_markdown` `:231`、`runbook_commands` `:260`、`duplicate_mentions` `:298`、`duplicated_content` `:333`、`normalize_line` `:381`、`looks_like_index` `:387`、`uncovered_units` `:406`、测试模块 `:415`。
- 开放问题记在 `_meta/gaps.md`（本单元：同名 `DocLevel` 双实现、非模块条目子串判定过宽、Runbook 只认文件名、重复正文按行不按段、三个函数 `Result` 签名下实际无 `Err` 路径）。
- 第 27 轮审查（当时编号 unit-src-30）的裁定是「本单元文档四面均过」：覆盖 13/13、必填节/N-A 合规、置信度诚实、Runbook 从零 12/12（EXIT=0）；当轮 FAIL 仅因 docs corpus 级 doc-lint 红（错在 change 目录与 `config/render.rs.md` 等其它单元的行号漂移/未完成标记），本文件 0 命中。
- 本轮（unit-src-31）实跑：仓树干净（`git status` 只有未跟踪的 `.pi/` 与产出目录 `docs/contexts/`），`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外空目录> TMPDIR=<仓外临时目录> CARGO_NET_OFFLINE=true cargo test --lib docgen::audit` → `12 passed; 0 failed; 0 ignored; 607 filtered out`，EXIT=0（`filtered out` 随全库用例数变，只认前两项）。
- 覆盖自检：`devforge doc-audit docs/contexts --repo . --level detailed` 中 `src/docgen/audit.rs` 的条目 0 条落在「未在文档中出现」（本单元清单 25 条：module×1 + symbol×12 + behavior×12 全命中）；`devforge doc-lint docs/contexts --repo .` → `doc-lint: 通过（0 违规）`，EXIT=0。
