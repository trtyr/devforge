---
type: module
module_path: src/docgen/inventory/closure.rs
unit: unit-src-34
description: 清单的三道闭包校验（文件覆盖 / 符号覆盖 / 入口可达）与词表导出——D002 分母自检 + D011 glossary 自动视图
---

# src/docgen/inventory/closure.rs

清单（`src/docgen/inventory/`）的**分母自检层**（D002）：清单抽完之后要证明「分母没漏」，靠三道闭包——磁盘上的源文件是否都进了清单、公开符号是否都进了清单、每个文件是否从某个入口点可达。同文件另出**词表**（D011）：`docs/contexts/_meta/glossary.md` 是清单的自动视图，不手写。

本文件是被 ARC-R6 从单文件 inventory.rs 垂直切出来的（`src/docgen/inventory/closure.rs:1` 的注释「纯移动」），对外名字经 `src/docgen/inventory/mod.rs:15` 的 `pub use closure::{check_closures, export_glossary};` 转发，因此调用方仍写 `crate::docgen::inventory::…`。两个函数都是纯读 + 纯拼装：不写盘、不联网、不起进程。

## 职责

**负责**：
- 三道闭包判定与排序输出：`check_closures`（`src/docgen/inventory/closure.rs:6`）。
- 词表文本生成：`export_glossary`（`:98`）。
- 判定结果的两种呈现口径：控制台（CLI 逐行打印 + 退出码）与审查输入（门 0 的 markdown 列表），见 [依赖](#依赖)。

**不负责**：
- 不建清单、不扫源文件——扫描与单元分配在 `src/docgen/inventory/scanner.rs`（`src/docgen/inventory/scanner.rs`），这是本文件的输入。
- 不做符号抽取与 import 解析——分别是 `public_symbols`（`src/docgen/inventory/scanner.rs:248`）与 `resolve_imports`（`src/docgen/inventory/imports.rs:6`），本文件只消费它们的输出。
- 不写 `docs/contexts/_meta/glossary.md` 文件——只返回字符串；落盘由调用方做（`src/main.rs:115` 之后的 CLI 路径、`src/sop/doc/wiring.rs:45`-49、`src/change/docs.rs:41`-45）。
- 不判「清单业务上对不对」（是否该拆单元、条目是否重复）——那是门 0 与审查的职责。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `check_closures` | 函数 | [check_closures](#check_closures) |
| `export_glossary` | 函数 | [export_glossary](#export_glossary) |

本文件没有私有 helper：逻辑全在两个公开函数体内（文件里除 `use super::*` 外没有其它项）。用到的类型全部来自同包：`Closure` / `ClosureFinding` / `Inventory` / `EntryKind` / `Lang`（`src/docgen/inventory/data.rs:108`、`:124`、`:101`、`:53`、`:32`）。

## 依赖

**上游（谁调用）**：

- `src/main.rs:115`——`devforge doc-inventory <repo> [--out <dir>]` 子命令：写完 `docs/contexts/_meta/inventory.json`（`:114`）立刻跑闭包校验，逐行打印 `path:line [closure] message`（`:132`-`:133`），全过 `exit(0)`、有项不通过 `exit(1)`（`:135`-140`）。
- `src/sop/doc/wiring.rs:50`——门 0：先 `:45`-49 把 `export_glossary` 的返回值写进 `_meta/glossary.md`，再把 findings 渲染成「门 0 闭包校验（引擎侧）」列表（`:51`-60）交审查。
- `src/change/docs.rs:43`——`essential` 文档档位下写 `_meta/glossary.md`（`DocAction` 的 `EssentialDone` 路径，`:35`-46）。

**下游（它依赖谁）**：

- `scan_source_files`（`src/docgen/inventory/scanner.rs:178`）→ `Vec<(相对路径, Lang)>`：磁盘上的源文件全集，是 ①②③ 三道的共同分母。
- Inventory 的 `entry_paths`（`src/docgen/inventory/data.rs:133`）与 Inventory 的 `symbol_names`（`:141`）：把清单折成两个集合供比对。**注意命名与实际口径的落差**：`entry_paths` 过滤的是 `Module` 变体（`:136`），不是 `Entry`。
- `public_symbols(lang, text)`（`src/docgen/inventory/scanner.rs:248`）→ `Vec<(行号, 名字)>`；`resolve_imports(lang, rel, text, root)`（`src/docgen/inventory/imports.rs:6`）→ 该文件 import 到的相对路径集合。
- `Lang` 的 `from_path`（`src/docgen/inventory/data.rs:42`）与 `Lang` 的 `Other` 变体（`:38`）：③ 的 BFS 只对能识别出语言的文件继续走边，`Other` 在 ② 里被整类跳过。
- `Closure` 的 `as_str`（`src/docgen/inventory/data.rs:115`）→ `entry-reachability` / `file-coverage` / `symbol-coverage`：排序键与打印用的字面串。
- 标准库：`Path` / `BTreeMap` / `BTreeSet` / `VecDeque`（`src/docgen/inventory/mod.rs:5`-6 引入，本文件经 `use super::*` 取得）；外部 crate 零。

## 关键口径

- 🟢 **三道闭包 = 三个互不替代的分母**：① 文件覆盖（`:13`-24）、② 符号覆盖（`:26`-44）、③ 入口可达（`:46`-89）。任一不过都进 findings，最终由 `:94` 一次性返回；「不过则分母无意义」写在函数文档 `:5`。🟢 CONFIRMED `src/docgen/inventory/closure.rs:13`、`:26`、`:46`
- 🟢 **① 的比对基是「清单里的 Module 条目」**：`inv` 的 `entry_paths()` 只收 `Module` 变体（`src/docgen/inventory/data.rs:136`），所以判定式是「磁盘源文件 ∉ 模块路径集合」——路径相同即视为已登记，不检查该 Module 条目的其它字段。🟢 CONFIRMED `src/docgen/inventory/closure.rs:14`、`src/docgen/inventory/data.rs:133`
- 🟢 **② 对无法抽符号的语言整类豁免**：`if *lang == Lang::Other { continue; }`（`:29`-31），即 java/c/php/rb 等扩展名只做 ①③、不做符号比对；这些语言登记在 `InventoryMeta` 的 `unsupported_languages`（`src/docgen/inventory/data.rs:96`）并进词表尾部（`:123`）。🟢 CONFIRMED `src/docgen/inventory/closure.rs:29`
- 🟢 **③ 的起点只有 `Entry` 变体**：`:47`-52 只把 `kind == Entry` 的条目路径当 BFS 种子；清单里一个 `Entry` 都没有时，直接给**每个**磁盘文件报 `entry-reachability`（`:53`-61）而不是静默通过。🟢 CONFIRMED `src/docgen/inventory/closure.rs:47`、`:53`
- 🟢 **③ 的 BFS 容错读不了的文件**：`read_to_string` 失败 `continue`（`:67`-69）、语言识别不出 `continue`（`:70`-72），只有成功解析的 import 边才入队；因此「读不动」不会变成 find 项，只会表现为后续不可达。🟢 CONFIRMED `src/docgen/inventory/closure.rs:67`
- 🟢 **输出顺序是确定的**：findings 按 `(closure.as_str(), path, line)` 排序（`:91`-93），所以两次跑同一仓得到同一序列，可直接做 diff。🟢 CONFIRMED `src/docgen/inventory/closure.rs:91`
- 🟢 **读源文件失败会让整个闭包检查返回 `Err`**：② 里对每个非 `Other` 文件的 `read_to_string` 用 `Context` 直接 `?`（`:32`-33），与 ③ 的容错相反；仓库根 `canonicalize` 失败同样 `?`（`:7`-9）。🟢 CONFIRMED `src/docgen/inventory/closure.rs:7`、`:32`
- 🟢 **词表只是视图**：`export_glossary` 首行固定 `# 词表（由 inventory 自动导出，勿手改）`（`:99`），只导 `Symbol` 变体（`:102`），按 unit 分组（`BTreeMap`，`:100`）再按名字排序（`:107`），空 unit 归入 `## 未分组`（`:108`）。当前仓的 `docs/contexts/_meta/glossary.md:1` 就是这句话、`:3` 起是 `## unit-src-01` 分组——可对照验证。🟢 CONFIRMED `src/docgen/inventory/closure.rs:99`、`docs/contexts/_meta/glossary.md:1`
- 🟡 **词表内容不做 markdown 转义**：条目模板 `- \`{name}\` — \`{path}:{line}\`{alias}`（`:116`-119`）直接拼接，名字或别名里若含反引号、`|`、换行会破坏词表结构；当前清单里的符号名都是标识符形态，所以未暴露。依据：`:111`-119 无转义调用。🟡 INFERRED
- 🟡 **①「文件覆盖」的语义边界**：`scan_source_files` 只按扩展名收源文件（`src/docgen/inventory/scanner.rs:178`），支持文件（Makefile / requirements.txt / SQL 等）走另一条 `scan_support_files`（`src/docgen/inventory/support.rs:148`），因此 ① 不覆盖支持文件——它们漏登记不会报「分母漏项」。依据：两道扫描函数的分工 + ① 的输入只有 `disk`。🟡 INFERRED

## 三道闭包（判据 / 文案 / 落点）

| 闭包 | `Closure` 变体 | 触发判据 | 文案 | 落点 |
|---|---|---|---|---|
| 文件覆盖 | `FileCoverage` | 磁盘源文件不在清单模块路径集合里 | `源文件不在清单中（分母漏项）` | `:15`-23 |
| 符号覆盖 | `SymbolCoverage` | 非 `Other` 文件里抽出的公开符号不在清单符号集合里 | `公开符号 {name} 不在清单中` | `:28`-43 |
| 入口可达 | `EntryReachability` | 清单无 `Entry` 条目，或文件不在从入口出发的 import 闭包里 | `清单里没有入口点（入度 0 无法确认）` / `从任何入口点都不可达（可能是死代码或漏登入口）` | `:53`-61 / `:79`-88 |

三道的 `line` 字段：文件覆盖与入口可达固定 `1`，符号覆盖用抽取到的符号行号。

## 词表输出形状

```text
# 词表（由 inventory 自动导出，勿手改）

## <unit_id 或 未分组>

- `<canonical_name>` — `<path>:<line>`（别名：a, b）

## 未支持符号抽取的语言（缺口）

<lang1>, <lang2>
```

确定性：分组键是 `BTreeMap`（字典序），组内按 `canonical_name` 排序，每组末尾多一个空行（`:121`）；`unsupported_languages` 为空时末段整节省略（`:123`）。别名段仅在该条目有别名时出现（`:111`-115）。

---

### check_closures

- **摘要**：对一份清单跑三道闭包校验，返回按 `(closure, path, line)` 排好序的问题列表；空列表 = 三道全过。（🟢 CONFIRMED `src/docgen/inventory/closure.rs:6`）
- **行为语义**：① 对 `scan_source_files` 的每个文件比对 `inv` 的 `entry_paths()`；② 对非 `Other` 语言的文件抽 `public_symbols` 并比对 `inv` 的 `symbol_names()`；③ 以清单里的 `Entry` 类条目为种子沿 `resolve_imports` 走 BFS，未进闭包的文件各报一条，无入口时退化为「每个文件都报」。**只读**：不写盘、不改 `inv`、不缓存；同一输入同一输出（`:91` 的排序保证顺序也稳定）。
- **参数**：`root: &Path`（仓库根；内部先 `canonicalize`，`:7`-9）、`inv: &Inventory`（已建好的清单，通常是 `build_inventory` 的产物或从 `docs/contexts/_meta/inventory.json` 读回）。
- **返回**：`Result<Vec<ClosureFinding>>`——`ClosureFinding` 有 `closure` / `path` / `line` / `message` 四个字段（`src/docgen/inventory/data.rs:125`-130`）。
- **异常**：`Err` 有两处——仓库根 `canonicalize` 失败（`:7`-9）、② 读取来源文件失败（`:32`-33`）；两者都是错误链（`Context` 加了路径语境）。③ 的读失败**不会**报错，只跳过。
- **示例**：

  ```rust
  let inv = build_inventory(&repo)?;
  let findings = check_closures(&repo, &inv)?;   // 空 = 三道全过
  for f in &findings {
      println!("{}:{} [{}] {}", f.path, f.line, f.closure.as_str(), f.message);
  }
  ```

  等价于 CLI 的 `devforge doc-inventory <repo>`（`src/main.rs:115`-140，全过退出码 0、有项不通过退出码 1）。

### export_glossary

- **摘要**：把清单渲染成词表 markdown（`docs/contexts/_meta/glossary.md` 的全文）。（🟢 CONFIRMED `src/docgen/inventory/closure.rs:98`）
- **行为语义**：只取 `Symbol` 变体 条目，按 `unit_id` 分组（`BTreeMap`）、组内按 `canonical_name` 排序；每条输出 `` `名字` — `路径:行号` ``，有别名时补 `（别名：…）`；`unit_id` 为空归 `## 未分组`；末尾按需追加「未支持符号抽取的语言（缺口）」一节。**纯函数**：不读盘、不写盘、不依赖时钟；同输入同输出。
- **参数**：`inv: &Inventory`（只读借用，不消耗所有权）。
- **返回**：`String`——完整 markdown（含 `# 词表…` 标题与各组末尾空行）。
- **异常**：`N/A`（不返回 `Result`）。
- **示例**：

  ```rust
  let text = export_glossary(&inv);
  assert!(text.starts_with("# 词表（由 inventory 自动导出，勿手改）"));
  std::fs::write(meta_dir.join("glossary.md"), text)?;
  ```

  生产落盘点：`src/sop/doc/wiring.rs:45`-49（门 0）与 `src/change/docs.rs:41`-45（essential 档）。

## 测试覆盖（用例在同包 tests.rs，属 unit-src-34）

本文件没有 `#[cfg(test)]`；覆盖它两个函数的用例集中在 `src/docgen/inventory/tests.rs`（该文件归 unit-src-34，这里只做引用不重复描述）：

| 用例 | 覆盖的判定 | 位置 |
|---|---|---|
| `file_coverage_closure_catches_missing_module` | ① 文件覆盖 | `src/docgen/inventory/tests.rs:76` |
| `symbol_coverage_closure_catches_missing_symbol` | ② 符号覆盖 | `src/docgen/inventory/tests.rs:90` |
| `entry_reachability_follows_python_imports` | ③ 可达（正向） | `src/docgen/inventory/tests.rs:103` |
| `unreachable_file_is_reported` | ③ 可达（不可达报出） | `src/docgen/inventory/tests.rs:116` |
| `missing_entry_points_reports_every_file` | ③ 无入口的退化分支 | `src/docgen/inventory/tests.rs:128` |
| `glossary_is_derived_from_inventory` | `export_glossary` 分组/排序/证据指针 | `src/docgen/inventory/tests.rs:139` |

## 测试运行与环境

- **环境要求**：Rust 工具链（`Cargo.toml:4` 为 `edition = "2024"` ⇒ Rust 1.85+；rustup 无默认工具链时显式 `RUSTUP_TOOLCHAIN=stable`）。本文件不调用 `git`、不读环境变量。
- **依赖安装**：`cargo fetch`（首次需网络；之后可 `CARGO_NET_OFFLINE=true`）。本文件零外部 crate，测试侧依赖 `tempfile`（`Cargo.toml:27`）。
- **配置项**：`N/A` —— 无运行时可配键；三道闭包的判据在代码里写死，没有开关。
- **启停与健康检查**：`N/A` —— 库内同步纯函数，无进程/端口/健康端点；「健康」体现为返回的 findings 是否为空。
- **种子数据**：`N/A` —— 输入是现仓扫描结果与清单；测试用例用 `tempfile` 临时仓自建种子（见上表，种子细节归 `src/docgen/inventory/tests.rs`）。
- **测试运行**：随宿主包一起跑——`cargo test --lib docgen::inventory`（覆盖整个 inventory 包，含本文件的两个函数）；只验本文件行为时按上表名字过滤，例如 `cargo test --lib docgen::inventory::tests::entry_reachability_follows_python_imports`。本轮实测见文末「证据与缺口」。
- **公开接口清单**：`N/A` —— 无 HTTP/CLI 端点；Rust 面即 `check_closures` / `export_glossary` 两个名字，经 `src/docgen/inventory/mod.rs:15` 转发后再被 `src/main.rs:115`、`src/sop/doc/wiring.rs:47`/`:50`、`src/change/docs.rs:43` 消费。

## 证据与缺口

- 结构事实逐行复核：`check_closures` `:6`、canonicalize `:7`-9、① `:13`-24、② `:26`-44、③ `:46`-89、排序 `:91`-93、`export_glossary` `:98`、标题 `:99`、分组 `:100`、Symbol 过滤 `:102`、组内排序 `:107`、未分组 `:108`、别名 `:111`-115、条目模板 `:116`-119、末段缺口 `:123`-128。
- 调用面：`src/main.rs:113`-140、`src/sop/doc/wiring.rs:45`-60、`src/change/docs.rs:36`-46（全仓无其它消费者）。
- 本轮（unit-src-34）实跑：`cargo test --lib docgen::inventory` → `24 passed; 0 failed; 0 ignored; 595 filtered out`，EXIT=0；本文件两个函数的 6 个用例全在其中（`--list` 显示用例全部落在 `docgen::inventory::tests::*`）。
- 开放问题记在 `_meta/gaps.md`（本单元 unit-src-34 段：源文件覆盖不覆盖支持文件、词表不做 markdown 转义、② 与 ③ 对读失败的处理不一致、③ 的不可达判定依赖 import 解析保真度）。
