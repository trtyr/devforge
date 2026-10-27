---
type: module
module_path: src/docgen/inventory/scanner.rs
unit: unit-src-34
description: 清单主扫描与单元预分配——磁盘源文件 → Module/Symbol/Config/Data/Behavior/Entry 条目，再按顶层目录切单元并编号
---

# src/docgen/inventory/scanner.rs

清单的**主扫描器**：从磁盘走一遍仓库，产出整份 `Inventory`——每个源文件一条 `Module` 条目、抽出的公开符号一条 `Symbol`、环境变量读取点一条 `Config`、SQL 表视图一条 `Data`、测试名与构建目标一条 `Behavior`、入口文件一条 `Entry`；最后给条目**预分配单元号**（D020），让逐单元文档任务有稳定的切分。

启发式 v1（`:5` 注释）：只用「行首特征 + 文件名/目录约定」，不建语法树、不跑编译器。除 `build_inventory` / `scan_source_files` / `scan_support_files` 读盘外无副作用：不写盘、不联网、不起进程。

## 职责

**负责**：
- 清单构建：`build_inventory`（`src/docgen/inventory/scanner.rs:6`）——条目抽取、支持文件合并、`meta` 填充，末尾调 `assign_units`。
- 源文件枚举：`scan_source_files`（`:178`）——递归遍历 + 排除规则 + 语言识别 + 排序。
- 排除规则：`SKIP_DIRS`（`:156`）与 `is_skipped_dir`（`:173`，`pub(super)`，被 `support` 复用）。
- 条目判定：`is_entry_file`（`:226`，入口文件启发式）、`public_symbols`（`:248`，四语言公开符号）。
- 单元预分配：`assign_units`（`:340`）——分组、切片、编号、回填；辅助 `group_of`（`:406`）、`sanitize`（`:413`）、`parse_unit_seq`（`:427`）、常量 `MAX_UNIT_MODULES`（`:337`）。

**不负责**：不定义数据结构（`data` 子模块）；不做闭包校验（`closure`）；不解析 import（`imports`）；不抽取约定面细节（`support`——本文件只调用它的函数并包装成条目）；不写清单文件（调用方用 `Inventory` 的 `save`）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `build_inventory` | 函数 | [build_inventory](#build_inventory) |
| `scan_source_files` | 函数 | [scan_source_files](#scan_source_files) |
| `is_entry_file` | 函数 | [is_entry_file](#is_entry_file) |
| `public_symbols` | 函数 | [public_symbols](#public_symbols) |
| `assign_units` | 函数 | [assign_units](#assign_units) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `SKIP_DIRS` | `src/docgen/inventory/scanner.rs:156` | 11 个跳过的目录名：node_modules、target、dist、build、out、__pycache__、venv、env、vendor、docs、assets |
| `is_skipped_dir` | `src/docgen/inventory/scanner.rs:173` | 判据 = 以 `.` 开头 或 命中 `SKIP_DIRS` 或 以 .venv 结尾；`pub(super)`，`support` 也用它 |
| `module_name` | `src/docgen/inventory/scanner.rs:217` | 取路径最后一段非空片段当模块名（即文件名，含扩展名） |
| `MAX_UNIT_MODULES` | `src/docgen/inventory/scanner.rs:337` | 单元粒度上限 8：大组按路径排序后每 8 个模块切一个单元 |
| `group_of` | `src/docgen/inventory/scanner.rs:406` | 分组键 = 路径第一个 `/` 之前的部分；无 `/` 视为 `root` |
| `sanitize` | `src/docgen/inventory/scanner.rs:413` | 分组名 → 单元号 slug：非字母数字/连字符换成 `-`，转小写，去掉首尾 `-` |
| `parse_unit_seq` | `src/docgen/inventory/scanner.rs:427` | 从单元号尾部取数字（`unit-xxx-07` → 7），用于水位计算 |

## 依赖

- **上游（谁调用）**：`src/main.rs` 的 `doc-inventory` 子命令（建清单 + 落盘）；`src/docgen/impact.rs:360` 起的 `plan_impact`（重建新清单做对照）；`src/sop/doc/wiring.rs` 与 `src/sop/doc/incremental.rs`（增量与首次全量派发）；`src/docgen/audit.rs` 与 `src/docgen/lint/entities.rs` 经 `scan_source_files` 拿源文件全集；`src/docgen/inventory/closure.rs:10` 用 `scan_source_files` 当分母。
- **下游（它依赖谁）**：`data`（`Inventory` / `InventoryEntry` / `InventoryMeta` / `EntryKind` / `Coverage` / `Lang` / `SOURCE_EXTENSIONS`）、`support`（`config_keys` / `env_file_keys` / `sql_objects` / `make_targets` / `package_scripts` / `is_test_file` / `test_specs` / `scan_support_files`）、以及经 `use super::*` 拿到的 `Path` / `BTreeMap` / `BTreeSet` / `Result` / `Context`。
- **外部依赖**：无网络、无进程；只读文件系统（`read_dir` / `read_to_string`，`canonicalize`）。测试用 `tempfile`（`Cargo.toml:27`）。

## 核心口径（解释层，逐条带置信度）

- 🟢 **每个源文件都有一条 Module 条目**：`scan_source_files` 的每个 `(rel, lang)` 先 push 一条 `id = mod:{rel}`、`kind = Module`、`line = 1`、`canonical_name = module_name(rel)` 的条目（`:14`-`:24`）——这是「文件覆盖」闭包的分母基线。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:14`、`:21`
- 🟢 **未支持语言只登记扩展名**：`Lang` 的 `Other` 分支把扩展名塞进 `unsupported` 集合（取不到扩展名时用 `?`），**不做符号抽取、不读文件**（`:27`-`:35`）；结果进 `meta` 的 `unsupported_languages`（`:147`）。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:27`、`:147`
- 🟢 **读源文件失败会让整次建清单失败**：非 `Other` 分支的 `read_to_string` 用 `Context` 直接 `?`（`:38`-`:39`）；而支持文件与各处目录读取失败是静默跳过（`:101`-`:103`、`:182`-`:185`）——两者容错口径相反。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:38`、`:101`、`:182`
- 🟢 **五种条目各有 id 前缀**：mod:（模块）、sym:（符号）、cfg:（配置）、spec:（行为/测试名）、entry:（入口点），支持文件侧还有 data:（SQL）与 cmd:（构建命令）（`:16`、`:42`、`:54`、`:71`、`:87`、`:128`、`:133`）。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:16`、`:42`、`:128`、`:133`
- 🟢 **测试规格的入口条件是「测试文件 或 Rust」**：`is_test_file(rel) || *lang == Lang::Rust` 才调 `test_specs`（`:64`-`:81`）——因为 Rust 的测试写在源码文件内的 `#[test]` 块里，不能只按文件名判。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:66`、`:69`
- 🟢 **支持文件的条目类型由文件名决定**：.env.example / .env.sample → `Config`（前缀 `cfg`）；.sql → `Data`（前缀 `data`）；`Makefile` / `makefile` / package.json → `Behavior`（前缀 `cmd`）（`:121`-`:140`）。支持文件先读文本，读失败跳过（`:101`-`:103`）。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:121`、`:126`、`:131`、`:136`
- 🟢 **`codegraph_indexed` 是「目录探测」而非真实索引状态**：`root.join(".codegraph").is_dir()`（`:146`）——只要目录在就记 `true`，不校验索引内容/新鲜度。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:146`
- 🟢 **单元分组只覆盖 4 类条目**：`Module` / `Config` / `Data` / `Behavior` 参与分组（`:342`-`:351`）；`Symbol` 与 `Entry` 不参与，最后统一「挂到同路径 Module 条目的单元上」（`:389`-`:402`）。因此同一个源文件里的符号永远和它的模块同单元。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:342`、`:389`
- 🟢 **大组按路径排序后每 8 个切一刀**：成员按 `path` 排序（`:369`）再 `chunks(MAX_UNIT_MODULES)`（`:370`），切分确定性来自排序，与 `read_dir` 顺序无关。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:369`、`:370`、`:337`
- 🟢 **单元号不漂移的三条规则**：① chunk 里只要有一个条目已有 `unit_id` 就用它（`:372`-`:376`）；② 否则从水位 `next` 递增（`:376`-`:383`）；③ 水位 = `max(meta.unit_seq, 已用最大编号)`（`:354`-`:364`），末尾回写 `meta` 的 `unit_seq`（`:403`）。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:354`、`:372`、`:403`
- 🟢 **单元号形状 `unit-{slug}-{NN}`**：slug 由顶层目录 `sanitize` 得到（`:380`-`:381`），`NN` 是两位补水位的十进制（`{next:02}`）；顶层目录缺失时分组键为 `root`（`:406`-`:411`）。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:380`、`:381`、`:407`
- 🟢 **扫描排除三类**：名字以 `.` 开头（`:189`-`:191`）、`is_skipped_dir` 命中的目录（`:192`-`:195`）、以 .min.js 结尾的文件（`:202`-`:204`）；非 `SOURCE_EXTENSIONS` 里的扩展名也不会成为源文件（`:199`-`:201`）。结果 `sort()`（`:213`），因此同一仓两次扫描顺序一致。🟢 CONFIRMED `src/docgen/inventory/scanner.rs:189`、`:192`、`:202`、`:213`
- 🟡 **`assets` 进入 SKIP_DIRS 是仓库特定取舍**：`:167`-`:168` 注释说明 assets/ 是随技能发布的 vendored 模板，「算进分母会把文档任务拖成写别人代码的说明书」；对别的仓（assets/ 是产品代码）这条排除会**漏项**，而文件覆盖闭包也只按同一份 `scan_source_files` 判定，发现不了这个漏。🟡 INFERRED（依据：`:156`-`:170` 的字面表与 `src/docgen/inventory/closure.rs:10` 复用同一函数）
- 🟡 **`public_symbols` 漏 `pub(crate)` / `pub(super)` 与多行声明**：Rust 分支只认行首 `pub ` + 紧跟的 fn / struct / enum / trait / const / mod（`:309`-`:326`），`pub(crate) fn` 因为 `rest` 以 `(crate)` 开头而不匹配；跨行的 `pub fn foo<\n T>()` 仍能命中（名字在首行），但 `pub\n fn` 不行。🟡 INFERRED（依据：`:311`-`:326` 的前缀链）
- 🟡 **`is_entry_file` 是文件名白名单**：11 个常见入口文件名 + cmd/、bin/、/bin/ 前缀（`:226`-`:245`）；`src/main.rs` 与 `src/lib.rs` 都算入口，因此一个库可能有多入口；反过来 examples/、benches/、wsgi.py 之类不算。🟡 INFERRED（依据：`:229`-`:244` 的 `matches!` 字面表）

## 条目生成规则速查

| 来源 | 判据 | 条目 | id 前缀 | 行号 | 落点 |
|---|---|---|---|---|---|
| 每个源文件 | `scan_source_files` 命中 | `Module` | mod: | 1 | `:14`-`:24` |
| 非 `Other` 源文件 | `public_symbols` 抽出 | `Symbol` | sym: | 符号行 | `:40`-`:51` |
| 非 `Other` 源文件 | `config_keys` 抽出 | `Config` | cfg: | 键所在行 | `:52`-`:63` |
| 测试文件或 Rust 源文件 | `test_specs` 抽出 | `Behavior` | spec: | 测试名行 | `:64`-`:81` |
| 入口文件 | `is_entry_file` | `Entry` | entry: | 1 | `:85`-`:96` |
| `Other` 语言文件 | 扩展名 | 只登记 `unsupported_languages` | — | — | `:27`-`:35` |
| .env.example / .env.sample | `env_file_keys` | `Config` | cfg: | 键所在行 | `:121`-`:125` |
| .sql | `sql_objects` | `Data` | data: | DDL 行 | `:126`-`:130` |
| `Makefile` / `makefile` | `make_targets` | `Behavior` | cmd: | 目标行 | `:131`-`:135` |
| package.json | `package_scripts` | `Behavior` | cmd: | 脚本名首次出现行 | `:136`-`:140` |

## 数据模型

`N/A` —— 本文件不定义类型；产出是 `Inventory`（见 [data.rs.md](data.rs.md)）。`meta` 三字段的来源：`root` = `canonicalize` 后的绝对路径字符串（`:145`）、`codegraph_indexed` = .codegraph 目录是否存在（`:146`）、`unsupported_languages` = `Other` 扩展名集合（`:147`）、`unit_seq` 先置 0（`:148`）后由 `assign_units` 回写（`:403`）。

---

### build_inventory

- **摘要**：从磁盘机械构建整份清单（含单元号预分配）。（🟢 CONFIRMED `src/docgen/inventory/scanner.rs:6`）
- **行为语义**：`canonicalize(root)` → `scan_source_files` → 逐文件生成 `Module` 条目并按语言补 `Symbol` / `Config` / `Behavior` / `Entry` → 遍历 `scan_support_files` 生成 `Config` / `Data` / `Behavior` → 组装 `meta` → `assign_units(&mut inv)` → 返回。**幂等**：同一磁盘状态重复调用，条目顺序与单元号一致（源文件与支持文件都排序，分组切片也先排序）；**只读**仓库，不写任何文件。
- **参数**：`root: &Path`（仓库根；内部会 `canonicalize`，因此传相对路径也可以，但结果里的 `meta` 的 `root` 与条目路径都基于规范化后的前缀）。
- **返回**：`Result<Inventory>`。
- **异常**：仓库根 `canonicalize` 失败 → `Err`（「解析仓库根 …」语境，`:7`-`:9`）；任一非 `Other` 源文件读失败 → `Err`（「读取源文件 …」，`:32`-`:34`）；`read_dir` 失败被跳过（`:181`-`:184`）。支持文件读失败不影响结果（`:97`-`:99`）。
- **示例**：

  ```rust
  let inv = build_inventory(&repo)?;
  println!("{} 条条目 / 水位 {}", inv.entries.len(), inv.meta.unit_seq);
  inv.save(&repo.join("docs/contexts/_meta/inventory.json"))?;
  ```

  生产消费：`src/main.rs` 的 `doc-inventory` 子命令、`src/docgen/impact.rs:361` 起的增量对照。

### scan_source_files

- **摘要**：枚举仓库里的源文件（相对路径 + 语言），按语言白名单过滤。（🟢 CONFIRMED `src/docgen/inventory/scanner.rs:178`）
- **行为语义**：从 `root` 起 DFS（`Vec` 栈）；跳过以 `.` 开头的名字（`:187`-`:189`）、`is_skipped_dir` 命中的目录（`:191`-`:195`）、.min.js 文件（`:200`-`:202`）；只收 `Lang` 的 `from_path` 认得的扩展名（`:198`）；路径 `strip_prefix(root)` 后把 `\` 换成 `/`（`:203`-`:207`）；结果 `sort()`。**只读**，**确定性**（排序保证）。
- **参数**：`root: &Path`（通常是已 `canonicalize` 的仓库根）。
- **返回**：`Result<Vec<(String, Lang)>>` —— `(相对路径, 语言)`，字典序。
- **异常**：实际不会返回 `Err`（`read_dir` 失败 `continue`，`:181`-`:184`；`Lang` 的 `from_path` 不中的文件跳过）；签名保留 `Result` 供上层统一 `?`。
- **示例**：`scan_source_files(&repo)?` 在 `closure` 里当「分母」，在 `lint` 的实体索引里当「语料清单」。

### is_entry_file

- **摘要**：判断某个相对路径是否算入口点文件（决定是否生成 `Entry` 条目）。（🟢 CONFIRMED `src/docgen/inventory/scanner.rs:226`）
- **行为语义**：先整体小写（`:227`），取最后一段文件名；命中 11 个白名单名（main.py / __main__.py / app.py / server.py / manage.py / main.go / main.rs / lib.rs / index.ts / index.js / cli.py）之一，或以 cmd/、bin/ 开头，或包含 /bin/（`:229`-`:246`）。纯函数、大小写不敏感。
- **参数**：`rel: &str`（相对路径，`/` 分隔）。
- **返回**：`bool`。
- **异常**：`N/A`。
- **示例**：`is_entry_file("src/main.rs") == true`；`is_entry_file("src/docgen/mod.rs") == false`。

### public_symbols

- **摘要**：按语言的行首特征抽公开符号（行号 + 名字）。（🟢 CONFIRMED `src/docgen/inventory/scanner.rs:248`）
- **行为语义**：逐行 `trim_start` 后匹配——Python：`def` / `async def` / `class` 且名字不以 `_` 开头（`:254`-`:268`）；TS/JS：`export ` 后接可选 `default` / `async`，再匹配 `function` / `class` / `const` / `let` / `interface` / `type` / `enum`（`:269`-`:289`）；Go：`func `（可选接收者括号）且名字首字母大写（`:290`-`:308`）；Rust：`pub ` 后接可选 `async` / `unsafe`，再匹配 `fn` / `struct` / `enum` / `trait` / `const` / `mod`（`:309`-`:326`）；`Other` 不抽（`:327`）。纯函数。
- **参数**：`lang: Lang`、`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(1-based 行号, 名字)`，输入顺序。
- **异常**：`N/A`。
- **示例**：`public_symbols(Lang::Rust, "pub fn build_inventory() {}")` → `[(1, "build_inventory")]`。

### assign_units

- **摘要**：给条目预分配单元号（按顶层目录分组、大组切片、编号一次分配不漂移）。（🟢 CONFIRMED `src/docgen/inventory/scanner.rs:340`）
- **行为语义**：① 收集 `Module` / `Config` / `Data` / `Behavior` 条目，按 `group_of(path)` 分组（`:342`-`:350`）；② 扫已有 `unit_id` 求水位（`:352`-`:362`）；③ 每组内按 `path` 排序、`chunks(8)` 切片（`:368`-`:370`）；④ 切片内若已有 `unit_id` 沿用，否则 `next += 1` 生成 `unit-{slug}-{NN}`（`:372`-`:383`）；⑤ `Symbol` / `Entry` 按同路径挂到 `Module` 条目的单元（`:390`-`:401`）；⑥ 回写 `meta` 的 `unit_seq`（`:403`）。**幂等**：对同一 `Inventory` 再调一次不会改变任何 `unit_id`（已有编号全被复用）；**原地修改** `inv`。
- **参数**：`inv: &mut Inventory`。
- **返回**：`()`（结果写在 `inv.entries[*].unit_id` 与 `meta` 的 `unit_seq`）。
- **异常**：`N/A`（无 IO、无 `Result`）。
- **示例**：

  ```rust
  let mut inv = build_inventory(&repo)?;   // 内部已调过一次
  assign_units(&mut inv);                  // 再调：unit_id 不变
  assert_eq!(inv.meta.unit_seq, inv.meta.unit_seq);
  ```

  用例：`unit_numbering_is_stable_across_runs`（`src/docgen/inventory/tests.rs:48`）、`oversized_group_is_split_into_multiple_units`（`:392`）。

## 测试覆盖

本文件没有内联 `#[test]`；行为由 `src/docgen/inventory/tests.rs` 覆盖（该文件归本单元）：

| 用例 | 覆盖的函数 | 位置 |
|---|---|---|
| `build_inventory_collects_modules_symbols_and_entries` | `build_inventory`（条目种类与 id 前缀） | `src/docgen/inventory/tests.rs:12` |
| `private_symbols_are_excluded` | `public_symbols`（`_` 前缀排除） | `src/docgen/inventory/tests.rs:27` |
| `other_languages_are_recorded_as_unsupported_gap` | `build_inventory` 的 `Other` 分支 | `src/docgen/inventory/tests.rs:39` |
| `unit_numbering_is_stable_across_runs` | `assign_units`（幂等/不漂移） | `src/docgen/inventory/tests.rs:48` |
| `skips_vendor_and_hidden_dirs` | `scan_source_files` 排除规则 | `src/docgen/inventory/tests.rs:162` |
| `ts_export_symbols_detected` / `rust_pub_symbols_detected` / `go_exported_symbols_detected` | `public_symbols` 三语言分支 | `src/docgen/inventory/tests.rs:175` / `:194` / `:213` |
| `convention_entries_land_in_inventory_with_units` | 支持文件条目 + `assign_units` 回填 | `src/docgen/inventory/tests.rs:280` |
| `test_files_get_behavior_entries` | 测试规格 → `Behavior` 条目 | `src/docgen/inventory/tests.rs:356` |
| `oversized_group_is_split_into_multiple_units` | 大组切片（`MAX_UNIT_MODULES`） | `src/docgen/inventory/tests.rs:392` |

`is_entry_file` 与 `scan_source_files` 的 `Result` 永不 `Err` 这两点**没有专门用例**（前者只在 `build_inventory` 的调用链里被间接覆盖）——见 `_meta/gaps.md`。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码（路径分隔符已归一）。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧 `tempfile`（`Cargo.toml:27`）；离线 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 无环境变量读进本文件；排除范围由编译期常量 `SKIP_DIRS` / `MAX_UNIT_MODULES` 与 `SOURCE_EXTENSIONS` 决定。
- **启停 + 健康检查**：`N/A` — 无进程/端口；「健康」体现为建完清单后 `closure` 三道闭包是否为空。
- **种子数据**：`N/A` — 输入是仓库磁盘；用例在 `tempfile` 临时仓自建。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::inventory
  ```

  预期（机器相关）：`24 passed; 0 failed`。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即上表 5 个名字（经 `src/docgen/inventory/mod.rs:14` 转发），另有 `pub(super)` 的 `is_skipped_dir` / `MAX_UNIT_MODULES` 只在包内可见。

## 证据与缺口

- 结构事实逐行复核：`build_inventory` `:6`、`Module` 条目 `:14`-`:24`、`Other` 分支 `:27`-`:35`、符号 `:40`-`:51`、配置 `:52`-`:63`、测试规格 `:64`-`:81`、入口点 `:85`-`:96`、支持文件 `:99`-`:141`、`meta` `:143`-`:152`、`SKIP_DIRS` `:156`、`is_skipped_dir` `:173`、`scan_source_files` `:178`、`module_name` `:217`、`is_entry_file` `:226`、`public_symbols` `:248`、`MAX_UNIT_MODULES` `:337`、`assign_units` `:340`、`group_of` `:406`、`sanitize` `:413`、`parse_unit_seq` `:427`。
- 开放问题登记在 `_meta/gaps.md` 的 unit-src-34 段（`assets` 排除是仓库特定取舍、`public_symbols` 漏 `pub(crate)`/`pub(super)`、`is_entry_file` 白名单窄、`scan_source_files` 的 `Result` 无 `Err` 路径）。
- 本轮（unit-src-34）实跑：`cargo test --lib docgen::inventory` → `24 passed; 0 failed; 0 ignored; 595 filtered out`，EXIT=0。
