---
type: module
module_path: src/docgen/impact.rs
unit: unit-src-33
description: 增量影响传播——从 git 变更集算出受影响单元与需复核的汇编页，宁可多算不可漏算
---

# src/docgen/impact.rs

doc-from-zero 增量模式的**影响面计算**（008 T023 / D015 / D021）：给定「这次改了什么文件」，回答两件事——哪些单元要重写、哪几篇汇编页要复核。文件头注释写明取舍：只做机械可判定的事，语义精确度不承诺，**宁可多算，不可漏算**（漏算 = 文档说假话，`src/docgen/impact.rs:1`-4）。

全部是纯函数 + 一次 git 调用：解析两种 git 输出（`--name-status`、`--porcelain`）、按路径把变更映射到清单单元、用「消失的符号名」反向找引用方、最后检查 4 篇汇编页。除 `changed_since` 起 `git` 子进程外，不做 IO 以外的副作用，不写盘。

## 职责

**负责**：
- 变更集解析：`parse_name_status`（`:58`）、`parse_porcelain`（`:94`）、`changed_since`（`:133`，跑 git 并合并未提交改动）。
- 变更 → 单元映射：`units_touching`（`:160`）。
- 删除/改名的传播：`vanished_names`（`:173`，旧清单有、新清单没有的名字）+ `reference_files`（`:198`，全仓源码里还引用这些名字的文件）。
- 汇编页复核清单：`ASSEMBLY_PAGES`（`:36`）与私有 `assembly_pages_mentioning`（`:334`）。
- 汇总入口：`impacted`（`:294`，纯汇总）与 `plan_impact`（`:360`，读旧清单 + 重建新清单 + 汇总）。

**不负责**：不生成清单（清单模块 docgen::inventory）；不判断文档内容是否真的需要改（只给「可能受影响」集合）；不改增量状态、不派发单元（`src/sop/doc/incremental.rs` 的 `merge_plan` 负责）；不做 git 写操作（只读 `diff` / `status`）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `ChangedFile` | 结构体 | [ChangedFile](#changedfile) |
| `Impact` | 结构体 | [Impact](#impact) |
| `ASSEMBLY_PAGES` | 常量 | [ASSEMBLY_PAGES](#assembly_pages) |
| `parse_name_status` | 函数 | [parse_name_status](#parse_name_status) |
| `parse_porcelain` | 函数 | [parse_porcelain](#parse_porcelain) |
| `changed_since` | 函数 | [changed_since](#changed_since) |
| `units_touching` | 函数 | [units_touching](#units_touching) |
| `vanished_names` | 函数 | [vanished_names](#vanished_names) |
| `reference_files` | 函数 | [reference_files](#reference_files) |
| `impacted` | 函数 | [impacted](#impacted) |
| `plan_impact` | 函数 | [plan_impact](#plan_impact) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `SKIP_DIRS` | `src/docgen/impact.rs:44` | 8 个扫描时跳过的目录：docs、.git、target、.codegraph、node_modules、dist、build、.venv |
| `run_git` | `src/docgen/impact.rs:143` | 唯一子进程点：git 非 0 退出即 `anyhow::bail!`，附 stderr |
| `contains_word` / `is_word_char` | `src/docgen/impact.rs:213` / `:239` | 词边界匹配（前后非字母数字下划线），避免子串误命中 |
| `walk` | `src/docgen/impact.rs:244` | 递归遍历仓库源码（排序、跳 `SKIP_DIRS` 与点文件、按扩展名过滤、读失败静默跳过） |
| `is_source_ext` | `src/docgen/impact.rs:285` | 23 个源码扩展名白名单 |
| `assembly_pages_mentioning` | `src/docgen/impact.rs:334` | 汇编页是否提到受影响单元 token 或消失符号 |

## 依赖

- **上游（谁调用）**：`src/sop/doc/wiring.rs:170` `changed_since`（增量为空则不派发，`:171` 至 `:176`）；`:177` `plan_impact`；`:184` 把 `Impact` 交给 `merge_plan`，`:191` 至 `:204` 把「受影响单元 / 汇编复核」写进续跑说明。`src/sop/doc/incremental.rs:12` 引入 `{ASSEMBLY_PAGES, Impact}`：`:109` `merge_plan` 读 impact.units（`:116`），`:150`-`:151` 把 `units` / `assembly_review` 落进状态，`:193` 用 `ASSEMBLY_PAGES` 做一致性检查。
- **下游（它依赖谁）**：`crate::docgen::inventory::{Inventory, build_inventory}`（清单模块 docgen::inventory）；`anyhow`（`Context` / `Result`）；标准库 `BTreeMap` / `BTreeSet` / `Path` / `PathBuf`，以及 `std` 的 Command 与 fs。
- **外部依赖**：`git` 可执行文件（仅 `changed_since` → `run_git`）；无网络、无端口。测试用 `tempfile`（`Cargo.toml:27`）。

## 核心口径（解释层，逐条带置信度）

- 🟢 **变更集 = 已提交 diff ∪ 未提交 status**：`changed_since` 跑 `git diff --name-status <base> HEAD` 与 `git status --porcelain`，两组结果按**新路径**放进同一个 `BTreeMap` 后返回（`:134`-`:140`）——未提交改动也算「这次变更」。🟢 CONFIRMED `src/docgen/impact.rs:133`-`:140`
- 🟢 **改名按「新路径 + old_path」双记**：`R`/`C` 都归一成 `kind: 'R'`，`path` 是新路径、`old_path` 是旧路径（`:78`-`:87`、`:106`-`:115`）；`units_touching` 与 `vanished_names` 都同时看两个路径（`:161`-`:167`、`:174`-`:177`）。🟢 CONFIRMED `src/docgen/impact.rs:78`-`:87`、`:161`-`:167`
- 🟢 **删除符号靠「名字差集 + 词边界引用扫描」传播**：`vanished_names` = 旧清单在变更路径上的名字集合 − 新清单同路径集合，并**过滤长度 < 2 的名字**（`:192`）；`reference_files` 用词边界匹配扫全仓源码（`:203`-`:209`）。🟢 CONFIRMED（`src/docgen/impact.rs:173`-`:210`）
- 🟢 **单元集合是「并集」而非「取最相关」**：变更文件命中的单元 ∪ 引用方所在单元（`:302`-`:319`）；返回前经 `BTreeSet` 收集，因此已排序去重（`:326`-`:330`）。🟢 CONFIRMED `src/docgen/impact.rs:294`-`:331`
- 🟢 **旧清单是新清单的兜底，但只在「一个都没命中」时整体兜底**：`units` 先按 `new_inv` 求，units.is_empty() 才改用 `old_inv`（`:302`-`:305`）——**不是逐文件兜底**。混合变更（既改活文件又删文件）时，删掉文件的单元只能靠 `reference_files` 或 `old_inv` 整体回退进入集合。🟢 CONFIRMED（`src/docgen/impact.rs:302`-`:305`）
- 🟢 **4 篇汇编页是硬编码约定**：`ASSEMBLY_PAGES`（`:36` 至 `:41`）就是 README、01-architecture、02-dataflow、10-runbook；页面不存在（或读失败）时静默跳过（`:345`-`:348`）。🟢 CONFIRMED `src/docgen/impact.rs:36`、`:345`
- 🟡 **单元 token 的形状在真实仓里可能对不上**：`assembly_pages_mentioning` 把 `unit-src-30` 之类 id 变成 `trim_start_matches("unit-")` + 连字符转斜杠，即 src/30（`:341`-`:344`）；而汇编页里通常写的是模块路径（`src/config/render.rs`）或单元号原文（`unit-src-30`），两种都不含 src/30 这个串。因此「按单元命中汇编页」这条分支在真实 id 格式下几乎恒假，汇编复核主要靠「消失符号名」分支。🟡 INFERRED（依据：本仓 64 个 unit id 全为 `unit-src-NN` 形态；用例 `:475` 用的是自造的 core/auth 形态）
- 🟡 **`reference_files` 只扫 23 种源码扩展名**：`is_source_ext`（`:285`-`:291`）不含 md/toml/yaml/sql 等扩展名，因此「文档或配置里还引用旧名字」不会被算作引用方；这是「漏算」风险点，登记在 `_meta/gaps.md`。🟡 INFERRED（依据：白名单字面量）
- 🟡 **`notes` 字段是过程日志而非判据**：调用方只把它拼进提示文本（`src/sop/doc/wiring.rs:203`），没有任何下游逻辑读它做分支。🟡 INFERRED（依据：grep impact.notes 无消费点）

## 数据模型（`ChangedFile` / `Impact`）

两者都是纯内存值，不落盘；`Impact` 的部分字段会被写进增量状态（`src/sop/doc/incremental.rs:150`-`:151`）。

| 结构 | 字段 | 类型 | 约束 / 可空 | 语义 |
|---|---|---|---|---|
| `ChangedFile` | `kind` | `char` | 取值见下 | 变更类型 |
| | `path` | `String` | 非空 | 相对仓库根、`/` 分隔；重命名记**新**路径 |
| | `old_path` | `Option<String>` | 仅 `R`/`C` 有值 | 重命名前的路径 |
| `Impact` | `units` | `Vec<String>` | 可空；有序去重 | 需要重写的单元 id |
| | `assembly_review` | `Vec<String>` | 可空；顺序 = `ASSEMBLY_PAGES` 顺序 | 需要复核的汇编页文件名 |
| | `notes` | `Vec<String>` | 可空 | 过程说明（引用方不在清单、删除符号清单、无命中提示） |

`kind` 的实际取值范围（`parse_name_status` 保留 `A`/`M`/`D`/`T`，`C` 归一成 `R`；`parse_porcelain` 只产出 `A`/`M`/`D`/`R`）：

| 来源 | 可能值 | 说明 |
|---|---|---|
| `git diff --name-status` | `A` `M` `D` `T` | 新增 / 修改 / 删除 / 类型变化（`T` 原样保留，尽管字段注释只列了 4 种） |
| 同上（重命名/复制） | `R` | `C`（复制）也记成 `R`，旧路径进 `old_path` |
| `git status --porcelain` | `A` `M` `D` `R` | `??` 视为 `A`；`R`/`C` 行按 ` -> ` 拆分 |

- **索引 / 唯一约束**：`N/A`（内存集合，无存储层；`changed_since` 用新路径做去重键）。
- **迁移注意**：`Impact` 的 `units` / `assembly_review` 会以 JSON 数组写进增量状态（`src/sop/doc/incremental.rs:150`-`:151`），字段名即序列化键；改名会影响旧状态可读性。

---

### ChangedFile

- **职责**：一次变更里的单个文件——只承载「类型 + 新旧路径」三件事，**不做**归一化之外的加工（不解析内容、不判断语言）。
- **构造 / 初始化**：结构体字面量直构；生产路径只有两个解析函数（`parse_name_status` / `parse_porcelain`），测试里也直接字面量构造。
- **公开成员**：`kind` / `path` / `old_path` 均 `pub`、均不可变（无 `&mut` 方法）；`derive(Debug, Clone, PartialEq, Eq)`。
- **生命周期**：值对象，从解析函数产出后传进 `units_touching` / `vanished_names` / `impacted`；不落盘。
- **典型用法**：`ChangedFile { kind: 'M', path: "core/a.py".into(), old_path: None }`（用例 `:413` 至 `:436`）。

### Impact

- **职责**：影响面聚合结果——「哪些单元要重写 + 哪几篇汇编页要复核 + 过程说明」。**不做**优先级排序或去重之外的加工（`units` 顺序来自 `BTreeSet`，即字典序，不是紧急度）。
- **构造 / 初始化**：`Default` 派生（空结构）；生产路径由 `impacted` 组装（`:326`-`:330`）。
- **公开成员**：`units` / `assembly_review` / `notes` 均 `pub`；无方法（纯数据）。
- **生命周期**：值对象；在增量模式下被 `merge_plan` 读取并转写进状态（`src/sop/doc/incremental.rs:116`、`:150`-`:151`）。
- **典型用法**：`assert_eq!(impact.assembly_review, vec!["README.md".to_string()])`（用例 `:475`）。

### ASSEMBLY_PAGES

- 名称 / 类型：`pub const ASSEMBLY_PAGES: [&str; 4]`（🟢 CONFIRMED `src/docgen/impact.rs:36`）
- 默认值：`README.md`、01-architecture.md、02-dataflow.md、10-runbook.md（顺序即 `assembly_review` 的输出顺序）。
- 是否必填：常量无「必填」语义；`impacted` 每次都会检查这 4 个名字，**文件不存在（或读失败）则跳过**（`:345`-`:348`）。
- 取值范围：编译期固定 4 项；改数组长度/内容会同时影响汇编复核范围与 `consistency_notes`（`src/sop/doc/incremental.rs:193`）。
- 影响：漏掉某篇汇编页 = 该页可能陈旧却没人复核；多加一篇 = 每次变更都多一次复核。
- 示例：`for page in ASSEMBLY_PAGES { … }`（`src/docgen/impact.rs:345`）。

### parse_name_status

- **摘要**：解析 `git diff --name-status` 输出为 `ChangedFile` 列表。（🟢 CONFIRMED `src/docgen/impact.rs:58`）
- **行为语义**：按行、按 `\t` 切分；`kind` 取首字符：`A`/`M`/`D`/`T` → 原样保留、`old_path = None`；`R`/`C` → `kind: 'R'`、`path` = 新路径、`old_path` = 旧路径；其它 kind 静默丢弃；空行跳过；缺字段的行跳过。无 IO。
- **参数**：`text: &str`（git 的原始 stdout）。
- **返回**：`Vec<ChangedFile>`（输入顺序）。
- **异常**：`N/A`（不返回 `Result`，坏行只是被跳过）。
- **示例**：输入 4 行制表符分隔记录（`M`、`A`、`D` 各带一个路径，`R100` 带新旧两个路径）→ 4 条；第 4 条 `kind == 'R'`、`path == "new.rs"`、`old_path == Some("old.rs")`（用例 `:395`-`:403`）。

### parse_porcelain

- **摘要**：解析 `git status --porcelain` 输出（含未跟踪、重命名）。（🟢 CONFIRMED `src/docgen/impact.rs:94`）
- **行为语义**：行长 < 4 跳过；取前 2 字节为状态、第 4 字节起为路径；含 `R`/`C` 且路径含 ` -> ` → 拆 old/new 记成 `kind: 'R'`；否则状态含 `A` 或 `?` → `'A'`、含 `D` → `'D'`、其余 → `'M'`。无 IO。
- **参数**：`text: &str`。
- **返回**：`Vec<ChangedFile>`（输入顺序）。
- **异常**：`N/A`。
- **示例**：输入「空格+M+空格+路径」「??+空格+路径」「R+两空格+旧路径+` -> `+新路径」三行 → 3 条，第二条 `kind == 'A'`，第三条 `kind == 'R'` 且 `old_path == Some("old.rs")`（用例 `:405`-`:411`）。

### changed_since

- **摘要**：取 base..HEAD 的变更集，并并入未提交改动。（🟢 CONFIRMED `src/docgen/impact.rs:133`）
- **行为语义**：先跑 `git diff --name-status <base> HEAD`，再跑 `git status --porcelain`；两组解析结果按新路径入 `BTreeMap`（后写入者覆盖同路径条目）；返回按路径字典序的值。**副作用**：起 2 个 git 子进程（只读）。
- **参数**：`repo: &Path`（工作仓库根）、`base: &str`（基线 SHA/ref）。
- **返回**：`anyhow::Result<Vec<ChangedFile>>`。
- **异常**：git 可执行缺失 → `anyhow` 上下文「跑 git … 失败」；git 退出码非 0 → `bail!` 附 stderr（如未知 base ref）。
- **示例**：`src/sop/doc/wiring.rs:170` 调用；空结果直接不派发单元（`:171` 至 `:176`）。

### units_touching

- **摘要**：变更文件命中的单元集合。（🟢 CONFIRMED `src/docgen/impact.rs:160`）
- **行为语义**：把每个文件的 `path` 与 `old_path` 收进集合，再取清单中 `path` 命中者的 `unit_id`；纯函数、无 IO。
- **参数**：`inv: &Inventory`、`files: &[ChangedFile]`。
- **返回**：`BTreeSet<String>`（排序去重）。
- **异常**：`N/A`。
- **示例**：只改 core/a.py 时集合恰 1 个单元且不含无关目录的单元（用例 `:413` 至 `:436`）。

### vanished_names

- **摘要**：变更路径上「旧清单有、新清单没有」的符号名。（🟢 CONFIRMED `src/docgen/impact.rs:173`）
- **行为语义**：对变更路径（含 `old_path`）分别取旧/新清单的 `canonical_name` 集合，求差集，过滤长度 < 2 的名字。纯函数。
- **参数**：`old: &Inventory`、`new: &Inventory`、`files: &[ChangedFile]`。
- **返回**：`BTreeSet<String>`。
- **异常**：`N/A`。
- **示例**：`alpha` 改成 `alpha_v2` 后差集含 `alpha`（用例 `:438` 起的 notes 断言）。

### reference_files

- **摘要**：全仓源码里以词边界引用了这些名字的文件。（🟢 CONFIRMED `src/docgen/impact.rs:198`）
- **行为语义**：`names` 为空直接返回空；否则 `walk` 全仓，只读 `is_source_ext` 白名单内的文件（23 种扩展名），跳过 `SKIP_DIRS` 与点文件；`contains_word` 命中即记相对路径。读取失败的文件静默跳过。
- **参数**：`repo: &Path`、`names: &BTreeSet<String>`。
- **返回**：`anyhow::Result<Vec<String>>`（排序）。
- **异常**：遍历目录失败（`read_dir`）→ `Err` 带路径上下文。
- **示例**：core/b.py 引用 `alpha` → 返回其相对路径，`impacted` 据此把该文件所在单元并入（用例 `:438` 至 `:473`）。

### impacted

- **摘要**：汇总影响面（纯汇总入口，不读旧清单文件）。（🟢 CONFIRMED `src/docgen/impact.rs:294`）
- **行为语义**：①`units_touching(new_inv)`，整体为空才回退 `units_touching(old_inv)`；②算 `vanished_names`，对每个引用方文件取 `new_inv` 中同路径条目的 unit 并入；找不到 → notes 记「引用方 … 不在清单里」（`:315`-`:316`）；③`assembly_pages_mentioning` 得汇编复核清单；④`names` 非空 → notes 记删除/改名符号清单；⑤返回 `Impact`。副作用：读源码文件（经 `reference_files`）与 4 篇汇编页。
- **参数**：`repo: &Path`、`old_inv: &Inventory`、`new_inv: &Inventory`、`files: &[ChangedFile]`、`docs_root: &Path`。
- **返回**：`anyhow::Result<Impact>`。
- **异常**：`reference_files` 的目录读取错误上抛。
- **示例**：删 `alpha` 后引用方单元必须进集合（用例 `:438` 至 `:473`）；README 提到受影响单元 → `assembly_review == ["README.md"]`（用例 `:475` 至 `:498`）。

### plan_impact

- **摘要**：便捷入口——读旧清单、重建新清单、算影响面。（🟢 CONFIRMED `src/docgen/impact.rs:360`）
- **行为语义**：读 docs_root 下的 _meta/inventory.json（`Inventory` 的 `load`）→ `build_inventory(repo)` → `impacted`；若 `units` 为空且变更非空 → notes 追加「变更文件没有命中任何单元（可能是纯文档/配置改动），不派发单元」。副作用：读旧清单 + 全仓扫描建新清单 + `impacted` 的全部读盘。
- **参数**：`repo: &Path`、`docs_root: &Path`、`changes: &[ChangedFile]`。
- **返回**：`anyhow::Result<Impact>`。
- **异常**：旧清单缺失/损坏 → `Err`，上下文含「读旧清单 … 失败（增量需要上次的清单做对照）」（`:361`-`:363`）。
- **示例**：无 `_meta/inventory.json` 时错误串含 inventory.json（用例 `:500` 至 `:510`）；生产调用见 `src/sop/doc/wiring.rs:177`。

## 测试覆盖（`#[cfg(test)]` `:374`，6 例，全绿）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `parses_name_status_and_renames` | diff 四类 + 重命名新旧路径 | `src/docgen/impact.rs:395` |
| `parses_porcelain_including_untracked_and_renames` | 修改/未跟踪/重命名 | `src/docgen/impact.rs:405` |
| `changed_file_maps_only_to_its_own_unit` | 路径 → 单元，且不含无关单元 | `src/docgen/impact.rs:413` |
| `deleted_symbol_pulls_in_referrers` | 删符号 → notes + 引用方单元入集合 | `src/docgen/impact_tests.rs` |
| `assembly_page_mentioning_changed_unit_is_flagged` | 汇编页命中单元 → 进复核清单 | `src/docgen/impact_tests.rs` |
| `plan_impact_without_old_inventory_errors` | 缺旧清单 → 明确报错 | `src/docgen/impact_tests.rs` |

测试夹具：`touch`（`:380`，写文件并建父目录）与 `repo`（`:386`，三文件两目录：core/a.py 定义 alpha、core/b.py 引用 alpha、other/c.py 定义 gamma）——全部跑在 `tempfile` 临时目录。

## 测试运行与环境（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`。**运行期还需要 `git` 在 PATH**——但 6 个用例都不调用 `changed_since`，测试本身不依赖 git。
- **依赖安装**：`cargo fetch`（首次需网络）；测试用 `tempfile`（`Cargo.toml:27`）。离线用 CARGO_NET_OFFLINE=true。
- **配置项**：`N/A` — 无环境变量或 CLI 开关读进本文件；扫描范围由编译期常量 `SKIP_DIRS` / `is_source_ext` / `ASSEMBLY_PAGES` 决定。
- **启停 + 健康检查**：`N/A` — 库内模块，无进程/端口/健康端点；进程级触发点是增量模式（`src/sop/doc/wiring.rs:170`）。
- **种子数据**：`N/A` — 输入是 git 变更集 + 旧清单（`docs/contexts/_meta/inventory.json`）+ 现仓扫描结果，无内置种子。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::impact
  ```

  预期（机器相关）：6 passed / 0 failed。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即上表 11 个名字，均从 crate::docgen::impact 全路径调用（`src/docgen/mod.rs` 无 re-export）。

## 证据与缺口

- 结构事实逐行复核：`ChangedFile` `:15`、`Impact` `:26`、`ASSEMBLY_PAGES` `:36`、`SKIP_DIRS` `:44`、`parse_name_status` `:58`、`parse_porcelain` `:94`、`changed_since` `:133`、`run_git` `:143`、`units_touching` `:160`、`vanished_names` `:173`、`reference_files` `:198`、`contains_word` `:213`、`is_word_char` `:239`、`walk` `:244`、`is_source_ext` `:285`、`impacted` `:294`、`assembly_pages_mentioning` `:334`、`plan_impact` `:360`、测试模块 `:374`、`touch` `:380`、`repo` `:386`、6 个用例 `:395`/`:405`/`:413`/`:438`/`:475`/`:500`。
- 开放问题记在 `_meta/gaps.md`（本单元：`kind` 取值与字段注释不一致、`changed_since` 按新路径去重可能丢 `old_path`、删除文件只在整体为空时回退旧清单、单字符符号名被过滤、汇编页 token 与真实 unit id 形态不匹配、引用扫描白名单漏文档/配置）。
- 本轮（unit-src-33）实跑：仓树干净（`git status` 只有未跟踪的本地状态目录 .pi 与产出目录 `docs/contexts/`），`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外空目录> TMPDIR=<仓外临时目录> CARGO_NET_OFFLINE=true cargo test --lib docgen::impact` → `6 passed; 0 failed; 0 ignored; 613 filtered out`，EXIT=0（`filtered out` 随全库用例数变，只认前两项）。
- 覆盖自检：`devforge doc-audit docs/contexts --repo . --level detailed` 输出中 `src/docgen/impact.rs` 的条目在「未在文档中出现」列表里 0 条（本单元清单 18 条：module×1 + symbol×11 + behavior×6 全命中）；`devforge doc-lint docs/contexts --repo .` → `doc-lint: 通过（0 违规）`，EXIT=0。
- 本轮按源码（512 行，mtime 2026-10-20）重校后修正的锚点：`run_git` `:151`→`:143`、`contains_word` `:219`→`:213`、`is_word_char` `:245`→`:239`、`walk` `:250`→`:244`、`SKIP_DIRS` `:43`→`:44`、`assembly_pages_mentioning` `:333`→`:334`；`changed_since` 并集 `:134`-`:140`、`parse_porcelain` 改名分支 `:106`-`:115`、`vanished_names` 过滤 `:192`、`reference_files` 扫描 `:203`-`:209`、`impacted` 回退分支 `:302`-`:305`、token `:341`-`:344` 与页跳过 `:345`-`:348`、`Impact` 组装 `:326`-`:330`、`plan_impact` 读旧清单 `:361`-`:363`（上列均已按源码逐行复核）。另：`is_source_ext` 白名单实为 **23** 种扩展名（此前误记 22，gaps 已同步）。
