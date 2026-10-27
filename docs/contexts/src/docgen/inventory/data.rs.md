---
type: module
module_path: src/docgen/inventory/data.rs
unit: unit-src-34
description: 清单的数据模型与落盘——条目/元信息/清单结构、语言与条目类型枚举、三道闭包的类型、JSON 读写
---

# src/docgen/inventory/data.rs

清单的**类型层**：定义「一条条目长什么样」「语言怎么识别」「闭包结果怎么表达」，并提供清单的 JSON 落盘/读回。整份 `_meta/inventory.json` 的形状就是本文件的 `Inventory`（`src/docgen/inventory/data.rs:102`）——`meta` + `entries` 两个字段，没有第三层。

纯数据结构 + 两个 IO 方法（`save` / `load`）：无网络、无进程；除这两个方法外不碰磁盘。子模块与外部都用 `crate::docgen::inventory::…`（经 `src/docgen/inventory/mod.rs:12` 的 `pub use`）访问本文件的名字。

## 职责

**负责**：
- 语言识别：`SOURCE_EXTENSIONS`（`:6`）与 `Lang` 的 `from_path`（`:42`）。
- 条目分类：`EntryKind`（`:53`）、`Coverage`（`:64`）。
- 数据形状：`InventoryEntry`（`:70`）、`InventoryMeta`（`:92`）、`Inventory`（`:102`）。
- 闭包表达：`Closure`（`:108`）、`Closure` 的 `as_str`（`:115`）、`ClosureFinding`（`:125`）。
- 集合查询：`Inventory` 的 `entry_paths`（`:133`）、`Inventory` 的 `symbol_names`（`:141`）。
- 持久化：`Inventory` 的 `save`（`:149`）、`Inventory` 的 `load`（`:158`）。

**不负责**：不扫描仓库、不判闭包、不分配单元号（分别是 `scanner` / `closure` 子模块）；不做清单校验（`load` 只做 JSON 反序列化）；不认识符号语法。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `SOURCE_EXTENSIONS` | 常量 | [SOURCE_EXTENSIONS](#source_extensions) |
| `Lang` | 枚举（5 变体） | [Lang](#lang) |
| `Lang` 的 `from_path` | 关联函数 | [from_path](#from_path) |
| `EntryKind` | 枚举（6 变体） | [EntryKind](#entrykind) |
| `Coverage` | 枚举（2 变体） | [Coverage](#coverage) |
| `InventoryEntry` | 结构体 | [InventoryEntry](#inventoryentry) |
| `InventoryMeta` | 结构体 | [InventoryMeta](#inventorymeta) |
| `Inventory` | 结构体 | [Inventory](#inventory) |
| `Closure` | 枚举（3 变体） | [Closure](#closure) |
| `Closure` 的 `as_str` | 方法 | [as_str](#as_str) |
| `ClosureFinding` | 结构体 | [ClosureFinding](#closurefinding) |
| `Inventory` 的 `entry_paths` | 方法 | [entry_paths](#entry_paths) |
| `Inventory` 的 `symbol_names` | 方法 | [symbol_names](#symbol_names) |
| `Inventory` 的 `save` | 方法 | [save](#save) |
| `Inventory` 的 `load` | 方法 | [load](#load) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `one` | `src/docgen/inventory/data.rs:87` | `line` 字段的 serde 默认值函数（返回 `1`），供 `#[serde(default = "one")]` 用 |

## 依赖

- **上游（谁调用）**：`scanner`（构造 `InventoryEntry` / `InventoryMeta` / `Inventory`、用 `Lang` / `SOURCE_EXTENSIONS`）、`support`（按 `Lang` 分支抽约定面）、`imports`（按 `Lang` 解析依赖）、`closure`（构造 `ClosureFinding`）、`src/docgen/audit.rs`（读 `EntryKind` / `InventoryEntry` / `Inventory`）、`src/docgen/impact.rs`（`Inventory` 的 `load`、`build_inventory` 返回值）、`src/change/gate0.rs` 与 src/sop/doc/*（单元号与清单读写）。
- **下游（它依赖谁）**：经 `use super::*`（`:3`）拿 mod.rs 的共享导入——`BTreeSet`、`Path`、`serde::{Serialize, Deserialize}`、`crate::error::{Context as _, Result}`、`serde_json`（`save` / `load` 用）。
- **外部依赖**：无网络、无进程；`save` / `load` 是唯一的磁盘接触面。

## 核心口径（解释层，逐条带置信度）

- 🟢 **JSON 形状即数据形状**：`Inventory` 序列化成 `{ "meta": {…}, "entries": [ … ] }`，字段名就是 Rust 字段名（没有 `rename`），枚举值经 `#[serde(rename_all = "lowercase")]` 变成小写字符串（`:30`-`:32`、`:51`-`:53`、`:62`-`:64`）。🟢 CONFIRMED `src/docgen/inventory/data.rs:30`、`:51`、`:62`、`:102`
- 🟢 **三个字段有 serde 默认值，老清单可被新代码读**：`InventoryEntry` 的 `line` 缺省为 `1`（`:76`-`:77`，函数 `one` `:87`）、`aliases` 缺省空表（`:79`）、`unit_id` 缺省空串（`:83`-`:84`）。反过来 `id` / `kind` / `path` / `canonical_name` / `status` 没有默认值——缺任何一个都会反序列化失败。🟢 CONFIRMED `src/docgen/inventory/data.rs:76`-`:84`
- 🟢 **`Closure` 不进 JSON**：`Closure` / `ClosureFinding` 不 derive `Serialize`（`:107`、`:124`），闭包结果是**当次运行**的判定，只用于打印与退出码，不落进 inventory.json。🟢 CONFIRMED `src/docgen/inventory/data.rs:107`、`:124`
- 🟢 **`Other` 语言是「登记缺口」而非「不支持」**：`SOURCE_EXTENSIONS` 把 13 种扩展名映射到 `Lang` 的 `Other`（`:15`-`:27`），`from_path` 仍返回 `Some(Other)`；`Other` 的文档注释明说「已知扩展名但不做符号抽取（登记为缺口）」（`:37`-`:38`）。🟢 CONFIRMED `src/docgen/inventory/data.rs:6`、`:15`-`:27`、`:42`
- 🟡 **`entry_paths` 的名字与实际返回不一致**：名字像「入口点路径」，实际返回的是**全部 `Module` 条目**的路径集合（`:133`-`:139`），即「已登记的模块文件」，不是 `EntryKind` 的 `Entry` 的入口点。调用方 `closure` 用它判「文件覆盖」，语义上恰好正确，但读名字容易误解。🟡 INFERRED（依据：`:136` 的 `filter(|e| e.kind == EntryKind::Module)` 与 `closure` 的用法对照）

## 数据模型（`Inventory` / `InventoryEntry` / `InventoryMeta`）

三者都可 `Serialize` + `Deserialize`；`Inventory` / `InventoryMeta` 还 derive `Default`（`:91`、`:101`）。

| 结构 | 字段 | 类型 | 约束 / 可空 | 语义 |
|---|---|---|---|---|
| `Inventory` | `meta` | `InventoryMeta` | 必有 | 抽取环境与水位 |
| | `entries` | `Vec<InventoryEntry>` | 可空（空清单合法） | 全部条目，顺序 = 扫描顺序 |
| `InventoryMeta` | `root` | `String` | 必有（可为空串） | 抽取时的仓库根（绝对路径字符串） |
| | `codegraph_indexed` | `bool` | 必有 | 建清单时是否已建 codegraph 索引 |
| | `unsupported_languages` | `Vec<String>` | 可空 | 出现但未做符号抽取的语言（D004 缺口登记） |
| | `unit_seq` | `u32` | 必有（serde 无默认） | 单元编号序列水位，保证预分配不漂移 |
| `InventoryEntry` | `id` | `String` | 必填 | 条目稳定标识（形如 `sym:<path>:<name>`） |
| | `kind` | `EntryKind` | 必填 | 条目类型 |
| | `path` | `String` | 必填 | 相对仓库根、`/` 分隔 |
| | `line` | `usize` | 默认 `1` | 1-based 行号；`Module` 条目恒为 1 |
| | `canonical_name` | `String` | 必填 | 规范名（符号名 / 模块名 / 配置键名） |
| | `aliases` | `Vec<String>` | 默认空 | 别名 |
| | `status` | `Coverage` | 必填 | 覆盖状态（清单生成时一律 `Uncovered`） |
| | `unit_id` | `String` | 默认空串 | 门 0 预分配的单元号；空 = 未分组 |

- **索引 / 唯一约束**：`N/A`（一个 JSON 文件，无数据库；`id` 的唯一性靠生成方 `scanner` 保证，`load` 不校验重复）。
- **迁移注意**：字段改名即破坏旧 inventory.json 的可读性（无版本字段）；新增字段须给 `#[serde(default)]`（现有 3 个默认值就是为兼容老清单加的），否则旧清单读回会失败。

---

### SOURCE_EXTENSIONS

- 名称 / 类型：`pub const SOURCE_EXTENSIONS: &[(&str, Lang)]`（🟢 CONFIRMED `src/docgen/inventory/data.rs:6`）
- 默认值：**21 条**扩展名映射——`py`→Python；`ts`/`tsx`/`js`/`jsx`/`mjs`→TsJs；`go`→Go；`rs`→Rust；`java`/`cs`/`php`/`rb`/`c`/`h`/`cc`/`cpp`/`hpp`/`kt`/`swift`/`scala`/`dart`→Other（`:7`-`:28`）。
- 是否必填：编译期常量，无「必填」语义；`Lang` 的 `from_path` 只认这张表，不在表内的扩展名返回 `None`。
- 取值范围：`(&str, Lang)` 二元组数组；同一扩展名不应重复（`from_path` 取第一个命中）。
- 影响：加一行 = 该扩展名进入清单扫描与符号抽取；只加到 `Other` = 进清单但只记模块条目、并在 `unsupported_languages` 里登记。
- 示例：`Lang::from_path(Path::new("src/lib.rs")) == Some(Lang::Rust)`。

### Lang

- **职责**：源文件语言分类——决定用哪套符号抽取与 import 解析规则。**不做**内容识别（只看扩展名）。
- **构造 / 初始化**：`Lang` 的 `from_path`（[from_path](#from_path)）是唯一入口；也可直接写变体字面量（`Copy`）。
- **公开成员**：`Python` / `TsJs` / `Go` / `Rust` / `Other`（`:33`-`:38`）；无公开方法（`from_path` 在 `impl Lang` 内，见下）。
- **生命周期**：`Copy` 值对象；derive `Ord`（`:30`），因此可直接进 `BTreeMap` / `BTreeSet` 排序。
- **典型用法**：`match Lang::from_path(p) { Some(Lang::Other) => unsupported.insert(ext), … }`（`scanner` 的缺口登记）。

### from_path

- **摘要**：按扩展名把路径映射成 `Lang`。（🟢 CONFIRMED `src/docgen/inventory/data.rs:42`）
- **行为语义**：取 `Path` 的 `extension` 的字符串（区分大小写，`RS` 不命中），在 `SOURCE_EXTENSIONS` 里线性查找第一个相等项；无扩展名 / 非 UTF-8 扩展名 / 未登记扩展名 → `None`。纯函数、无 IO。
- **参数**：`path: &Path`。
- **返回**：`Option<Lang>`。
- **异常**：`N/A`（不返回 `Result`；未识别就是 `None`）。
- **示例**：Lang::from_path(Path::new("a/b.tsx")) → `Some(Lang::TsJs)`；`Lang::from_path(Path::new("README"))` → `None`。

### EntryKind

- **职责**：条目类型——决定覆盖审计要求哪些条目（`Essential` / `Complete` / `Detailed` 三档的口径见 `src/docgen/audit.rs`）。**不做**优先级排序。
- **构造 / 初始化**：枚举字面量；JSON 里是小写字符串（`:51`-`:53`）。
- **公开成员**：`Entry` / `Module` / `Symbol` / `Config` / `Data` / `Behavior`（`:54`-`:59`）。
- **生命周期**：`Copy` 值对象，进 `InventoryEntry`，随清单落盘。
- **典型用法**：`e.kind == EntryKind::Module`（`entry_paths` 与 `audit` 的模块覆盖判定）。

### Coverage

- **职责**：条目的覆盖状态位。**不负责**计算覆盖（那是 `src/docgen/audit.rs` 的事）。
- **构造 / 初始化**：枚举字面量；JSON 为 `"uncovered"` / `"covered"`。
- **公开成员**：`Uncovered` / `Covered`（`:65`-`:66`）。
- **生命周期**：`Copy`；清单生成时一律 `Uncovered`，覆盖审计不回写它（审计结果是内存值）。
- **典型用法**：`InventoryEntry { status: Coverage::Uncovered, … }`。

### InventoryEntry

- **职责**：一条可对账的条目（模块 / 符号 / 入口点 / 配置 / 数据 / 行为）。**不做**内容校验。
- **构造 / 初始化**：结构体字面量直构（生产路径在 `scanner` / `support`）。
- **公开成员**：`id` / `kind` / `path` / `line` / `canonical_name` / `aliases` / `status` / `unit_id`，全 `pub`；无方法。
- **生命周期**：随 `Inventory` 落盘；`CoverageMiss`（审计侧）会拷贝其字段。
- **典型用法**：`InventoryEntry { id, kind: EntryKind::Symbol, path, line, canonical_name, aliases: vec![], status: Coverage::Uncovered, unit_id }`。

### InventoryMeta

- **职责**：清单的抽取环境与编号水位。**不做**校验。
- **构造 / 初始化**：`Default`（`:91`）+ 字段赋值；`scanner` 的 `build_inventory` 负责填 `root` / `unsupported_languages` / `unit_seq`。
- **公开成员**：`root` / `codegraph_indexed` / `unsupported_languages` / `unit_seq`，全 `pub`。
- **生命周期**：随清单落盘；`unit_seq` 被 `assign_units` 续写（单元号不漂移的关键）。
- **典型用法**：inv.meta.unit_seq 决定下一批单元号从几开始。

### Inventory

- **职责**：整份清单（`meta` + `entries`）及其查询/持久化方法。**不负责**生成与校验。
- **构造 / 初始化**：`Default`（空清单）或 `scanner` 的 `build_inventory`；读盘用 `Inventory` 的 `load`。
- **公开成员**：`meta` / `entries`（`pub`）；方法 `entry_paths` / `symbol_names` / `save` / `load`。
- **生命周期**：值对象；进程内经 `&Inventory` 传递（审计、影响面、闭包都只读），落盘为 `_meta/inventory.json`。
- **典型用法**：见 [save](#save) / [load](#load)。

### Closure

- **职责**：三道闭包的标识——文件覆盖 / 符号覆盖 / 入口可达。**不做**判定（判定在 `closure` 的 `check_closures`）。
- **构造 / 初始化**：枚举字面量；不参与 JSON（无 `Serialize`）。
- **公开成员**：`EntryReachability` / `FileCoverage` / `SymbolCoverage`（`:109`-`:111`）；方法 `as_str`。
- **生命周期**：`Copy`，随 `ClosureFinding` 只在内存里活一次运行。
- **典型用法**：`closure` 的 `as_str` 拼报告前缀。

### as_str

- **摘要**：闭包标识的稳定字符串（报告/CLI 用）。（🟢 CONFIRMED `src/docgen/inventory/data.rs:115`）
- **行为语义**：`EntryReachability` → `entry-reachability`、`FileCoverage` → `file-coverage`、`SymbolCoverage` → `symbol-coverage`（`:117`-`:121`）。常量返回，无分配。
- **参数**：`self`（`Copy`）。
- **返回**：`&'static str`。
- **异常**：`N/A`。
- **示例**：`Closure::FileCoverage.as_str() == "file-coverage"`。

### ClosureFinding

- **职责**：一条闭包违规（哪道闭包、哪个文件、哪一行、什么话）。**不做**聚合或排序（排序在 `check_closures`）。
- **构造 / 初始化**：结构体内字面量（生产路径在 `closure` 的 `check_closures`）。
- **公开成员**：`closure` / `path` / `line` / `message`，全 `pub`；无方法。
- **生命周期**：内存值，进 `Vec<ClosureFinding>` 后被打印/计数。
- **典型用法**：`format!("[{}] {}:{} {}", f.closure.as_str(), f.path, f.line, f.message)`。

### entry_paths

- **摘要**：全部 `Module` 条目的路径集合。（🟢 CONFIRMED `src/docgen/inventory/data.rs:133`）
- **行为语义**：`filter(kind == Module)` → `path` 去重排序（`BTreeSet`）。纯读、无副作用。
- **参数**：`&self`。
- **返回**：`BTreeSet<String>`（字典序）。
- **异常**：`N/A`。
- **示例**：inv.entry_paths().contains("src/lib.rs")。

### symbol_names

- **摘要**：全部 `Symbol` 条目的 `(路径, 名字)` 集合。（🟢 CONFIRMED `src/docgen/inventory/data.rs:141`）
- **行为语义**：`filter(kind == Symbol)` → `(path, canonical_name)` 去重排序。纯读。
- **参数**：`&self`。
- **返回**：`BTreeSet<(String, String)>`（先按路径、再按名字）。
- **异常**：`N/A`。
- **示例**：`inv.symbol_names().contains(&("src/lib.rs".into(), "build_inventory".into()))`。

### save

- **摘要**：把清单写成格式化 JSON 文件（父目录不存在则创建）。（🟢 CONFIRMED `src/docgen/inventory/data.rs:149`）
- **行为语义**：`Path` 的 `parent` 存在时先 `create_dir_all`；`serde_json` 的 `to_string_pretty` 后整文件覆盖写。**副作用**：建目录 + 覆写目标文件（非原子，先截断再写）。幂等（同内容重复写结果相同）。
- **参数**：`&self`、`path: &Path`（目标文件路径）。
- **返回**：`Result<()>`。
- **异常**：建目录失败 → `Err` 带「建目录 …」语境；序列化失败 → `Err`（serde 错误直接上抛，无语境）；写文件失败（权限/磁盘）→ `Err` 带「写清单 …」语境。
- **示例**：inv.save(&docs_root.join("_meta/inventory.json"))。

### load

- **摘要**：从 JSON 文件读回清单。（🟢 CONFIRMED `src/docgen/inventory/data.rs:158`）
- **行为语义**：整文件读成字符串 → `serde_json` 的 `from_str`。无 schema 校验：`entries` 可以为空；重复 `id` 不报错；未知字段被忽略（serde 默认行为，未加 deny_unknown_fields）。
- **参数**：`path: &Path`。
- **返回**：`Result<Inventory>`。
- **异常**：文件不存在/读失败 → `Err` 带「读清单 …」语境（`src/docgen/impact.rs:361`-`:363` 的增量入口就靠这条错误提示「增量需要上次的清单」）；JSON 非法或缺必填字段 → serde 错误上抛。
- **示例**：`let inv = Inventory::load(&docs.join("_meta/inventory.json"))?;`。

## 测试覆盖

本文件**没有内联 `#[cfg(test)]`**；`Inventory` 的 JSON 往返、`save` / `load` 的建目录行为由 `src/docgen/inventory/tests.rs` 的 `inventory_roundtrip_json`（`:150`）覆盖，语言识别与扩展名由 `skips_vendor_and_hidden_dirs`（`:162`）、`other_languages_are_recorded_as_unsupported_gap`（`:39`）间接覆盖（详见 [tests.rs.md](tests.rs.md)）。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；显式 `RUSTUP_TOOLCHAIN=stable` 可避免「无默认 toolchain」。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧 `tempfile`（`Cargo.toml:27`）；离线 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 无环境变量读进本文件。
- **启停 + 健康检查**：`N/A` — 无进程/端口。
- **种子数据**：`N/A` — 清单由 `scanner` 的 `build_inventory` 现场生成；本文件只负责形状与读写。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::inventory
  ```

  预期（机器相关）：`24 passed; 0 failed`（本单元 7 个文件的全部用例都在 `src/docgen/inventory/tests.rs`）。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即上表 14 个名字 + `from_path`。

## 证据与缺口

- 结构事实逐行复核：`SOURCE_EXTENSIONS` `:6`、`Lang` `:32`、`from_path` `:42`、`EntryKind` `:53`、`Coverage` `:64`、`one` `:87`、`InventoryEntry` `:70`、`InventoryMeta` `:92`、`Inventory` `:102`、`Closure` `:108`、`as_str` `:115`、`ClosureFinding` `:125`、`entry_paths` `:133`、`symbol_names` `:141`、`save` `:149`、`load` `:158`。
- 扩展名表实有 21 条（`:7`-`:28`），其中 13 条落在 `Lang` 的 `Other`（`:15`-`:27`）。
- 开放问题登记在 `_meta/gaps.md` 的 unit-src-34 段。
