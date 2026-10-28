---
type: module
module_path: src/docgen/inventory/imports.rs
unit: unit-src-34
description: 四语言 import/use/mod 解析——把一个文件的依赖边解析成仓库内相对路径，供入口可达闭包做 BFS
---

# src/docgen/inventory/imports.rs

清单的**依赖边解析层**：给定一个文件的全文，抽出它 import 了仓库内哪些文件，返回「相对仓库根的路径」列表。它是第三道闭包（入口可达）唯一的边来源——`src/docgen/inventory/closure.rs:73` 用它的返回值沿依赖图 BFS。

覆盖 4 类语言族：Python（`from x import` / `import x`，含相对点号）、TS/JS（`from '…'` / `require(…)`）、Go（`import "…"` 与 import 块内的裸字符串行）、Rust（`use crate::…` / `use <crate>::…` / `mod x;`）。其余语言（`Lang` 的 `Other`）直接跳过。纯函数式解析 + 只读探针（用 `is_file` 检查候选是否存在）：不写盘、不联网、不起进程。

## 职责

**负责**：
- 解析语义：`resolve_imports`（`src/docgen/inventory/imports.rs:6`）——按 `Lang` 分派四条解析路径（`:40`/`:68`/`:86`/`:99`），`:124` 的 `Other` 分支什么都不做。
- 候选路径消歧：内部闭包 `candidate`（`:13`）把「模块名」变成真实文件（补扩展名 / index / __init__.py）。
- Rust crate 名识别：`cargo_crate_name`（`:142`，读 `Cargo.toml` 的 `name`，`-`→`_`）。
- Go 包路径后缀匹配：`resolve_by_suffix`（`:159`，按 import 串的最后一段找同名文件）。
- 路径归一化：`norm`（`:183`，`pub(super)`，供 `scanner` / `support` 复用）。

**不负责**：不判断依赖是否「必要」、不建图、不做传递闭包（BFS 在 `closure`）；不解析动态 import（importlib、`await import()`、反射）；不读环境变量或配置；不返回错误（坏行静默跳过）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `resolve_imports` | 函数 | [resolve_imports](#resolve_imports) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `candidate` | `src/docgen/inventory/imports.rs:13` | 闭包：`base` 是文件即返回；否则依次试 .py/.ts/.tsx/.js/.jsx/.mjs/.go/.rs、index.ts/index.js/index.tsx/mod.rs、__init__.py |
| `add_relative` | `src/docgen/inventory/imports.rs:130` | 只接受以点斜杠或点点斜杠开头的模块串（TS/JS 分支用） |
| `cargo_crate_name` | `src/docgen/inventory/imports.rs:142` | 读 `Cargo.toml` 的 `name = "…"`，`-` 换成 `_`；读不到返回 `None` |
| `resolve_by_suffix` | `src/docgen/inventory/imports.rs:159` | 按 spec 最后一段找同名文件（Go 用），跳过点目录 / `node_modules` / `target` |
| `norm` | `src/docgen/inventory/imports.rs:183` | `pub(super)`：路径转字符串并把 `\` 换成 `/` |

## 依赖

- **上游（谁调用）**：`src/docgen/inventory/closure.rs:73`（入口可达 BFS 的唯一边来源）；`src/docgen/inventory/support.rs:4` 复用其 `norm`（`use super::imports::norm;`）。全仓无其它调用点。
- **下游（它依赖谁）**：经 `use super::*`（`:3`）取 mod.rs 的共享导入——`BTreeSet`、`Path` / `PathBuf`、`Lang`（`data` 子模块）。文件系统只读（`is_file` / `read_dir` / `read_to_string`）。
- **外部依赖**：无网络、无进程；`Lang` 的 `Other` 分支不触碰文件系统。

## 核心口径（解释层，逐条带置信度）

- 🟢 **返回的是「仓库内真实存在的文件」**：所有分支最终都经 `candidate`（`:13`）或 `resolve_by_suffix`（`:159`）确认 `is_file()` 后才插入结果集合（`:24`-`:26`、`:169`-`:172`）；解析不出文件的 import 一律不返回，因此闭包 BFS 不会因为「外部依赖」而多出节点。🟢 CONFIRMED `src/docgen/inventory/imports.rs:13`、`:24`、`:159`
- 🟢 **候选消歧顺序固定**：先原样文件 → 再补 8 种扩展名 → 再 index.ts / index.js / index.tsx / mod.rs → 最后 __init__.py（`:13`-`:35`）；先命中先返回，不做多候选收集。🟢 CONFIRMED `src/docgen/inventory/imports.rs:13`、`:20`、`:27`、`:33`
- 🟢 **Python 相对导入按点号深度上跳**：`.` 的个数 = `depth`，从文件所在目录向上走 `depth - 1` 层，再把模块名里的 `.` 换成 `/` 拼上去（`:52`-`:62`）；绝对导入则从仓库根按模块名拼（`:63`-`:66`）。🟢 CONFIRMED `src/docgen/inventory/imports.rs:52`、`:56`、`:63`
- 🟢 **TS/JS 只认相对路径**：`from '…'` 与 `require(…)` 两条正则式查找（`:69`-`:85`），但最终只有以点斜杠或点点斜杠开头的 spec 会被 `add_relative` 接受（`:131`-`:139`）——裸包名（react）不算仓库内依赖。🟢 CONFIRMED `src/docgen/inventory/imports.rs:69`、`:80`、`:131`
- 🟢 **Rust 只认 `crate` 前缀 与仓名开头的 `use`**：`use` 行先剥可选 `pub `（`:100`），目标再剥 `crate` 前缀 或 `<crate 名>::`（`:102`-`:104`），剩余部分替换 `::`→`/` 后拼在 `src/` 下找（`:110`-`:112`）；因此 `use std::…` / `use serde::…` 不产生边。`mod x;` 按「当前文件目录 + x」找（`:116`-`:121`）。🟢 CONFIRMED `src/docgen/inventory/imports.rs:102`、`:110`、`:116`
- 🟢 **Go 靠「文件名后缀」而不是包路径**：import 串取最后一段（`:160`），深度优先遍历仓库找同名文件（`:161`-`:178`），跳过点目录 / `node_modules` / `target`（`:166`-`:169`）。import 块内的裸字符串行也吃（`:91`-`:97`）。🟢 CONFIRMED `src/docgen/inventory/imports.rs:159`、`:160`、`:166`
- 🟡 **同名文件命中顺序取决于文件系统遍历序**：`resolve_by_suffix` 用 `Vec` 当栈（LIFO）+ `read_dir` 原序，命中第一个就返回（`:161`-`:173`）；同名文件存在于多个目录时，返回哪一个不稳定（跨机器/跨文件系统可能不同）。🟡 INFERRED（依据：`read_dir` 无 `sort`，与 `scanner` 的 `scan_source_files` / `walk` 里显式排序的做法不一致）
- 🟡 **Rust 的 `mod` 解析不认 `#[path]` 与 `mod x { … }` 内联形式**：只处理 `mod name;`（`:116`-`:121`），内联模块与 `#[path = "…"]` 属性不会产生边。🟡 INFERRED（依据：`:116` 只做 `strip_prefix("mod ")` + `trim_end_matches(';')`）
- 🟡 **解析是逐行文本匹配，不是语法树**：注释里的 `// from './x'`、字符串里的 `import` 都会被当成真依赖；反之多行 import、别名 `as`、模板字符串拼接会漏。🟡 INFERRED（依据：`:37` 逐行 + 各分支的 `strip_prefix` / `find`）
- 🟡 **`candidate` 的扩展名列表比 `SOURCE_EXTENSIONS` 窄**：这里只补 8 种（py/ts/tsx/js/jsx/mjs/go/rs，`:20`），`Lang` 的 `Other` 的语言（java/kt/…）即使写了 import 也解析不出文件——与「`Other` 不做符号抽取」的缺口口径一致。🟡 INFERRED（依据：`:20` 字面量与 `src/docgen/inventory/data.rs:6` 对照）

## 解析规则（按语言）

| 语言 | 输入形态 | 定位方式 | 落点 |
|---|---|---|---|
| `Python` | `from pkg.mod import x` / `import pkg.mod` / `from . import x` / `from ..a.b import x` | 点号深度上跳 + `.`→`/`，再走候选消歧 | `:40`-`:67` |
| `TsJs` | `… from 'pkg'` / `… from "pkg"` / `require('pkg')` | 只接受点斜杠、点点斜杠开头，拼到当前目录 | `:68`-`:85`（+`add_relative` `:130`） |
| `Go` | `import "a/b/c"` / import 块内的 "a/b/c" | 取最后一段当文件名，全仓找同名文件 | `:86`-`:98`（+`resolve_by_suffix` `:159`） |
| `Rust` | `use crate::a::b;` / `use <crate>::a::b;` / `mod b;` | `src/` + `::`→`/`；`mod` 用当前目录 | `:99`-`:123` |
| `Other` | 任意 | 不解析 | `:124` |

## 数据模型

`N/A` —— 本文件不定义任何类型；输入是 `Lang` + 文件相对路径 + 全文 + 仓库根，输出是 `Vec<String>`（相对路径，`/` 分隔、去重排序，因为它们先收进 `BTreeSet` 再 `into_iter()`，`:7`、`:127`）。

---

### resolve_imports

- **摘要**：解析单个文件的 import，返回仓库内被依赖文件的相对路径列表（排序去重）。（🟢 CONFIRMED `src/docgen/inventory/imports.rs:6`）
- **行为语义**：逐行扫描（`:37`），按 `lang` 分派到四条规则（Python `:40`、TS/JS `:68`、Go `:86`、Rust `:99`、其它 `:124`）；每个候选都要求「仓库里真的存在该文件」才收录（`candidate` `:13` / `resolve_by_suffix` `:159`）。**幂等**（同输入同输出，除 Go 同名文件的遍历序不确定，见核心口径）；**只读**（`is_file` / `read_dir` / `read_to_string`）；结果顺序由 `BTreeSet` 决定，即路径字典序。
- **参数**：
  - `lang: Lang` —— 由调用方用 `Lang` 的 `from_path` 得到（`closure` 就是这么给的）。
  - `rel: &str` —— 当前文件相对仓库根的路径（`/` 分隔），用于算所在目录与 `mod` 的基准目录；传空串时 `dir` 退化为 `""`（`:8`）。
  - `text: &str` —— 文件全文（调用方已读好；本函数不读该文件）。
  - `root: &Path` —— 仓库根（探针拼绝对路径用）。
  - 候选消歧会读 root/Cargo.toml（`cargo_crate_name` `:142`）与 `read_dir`（Go 分支）。
- **返回**：`Vec<String>` —— 相对路径（`/` 分隔、字典序），空表示解析不出仓库内依赖。
- **异常**：`N/A`（不返回 `Result`）：`Cargo.toml` 读不到 → crate 名缺失、`crate` 前缀 前缀仍可用（`:10`-`:11`）；`read_dir` 失败 → 跳过该目录（`:162`-`:164`）；文件不存在 → 该候选被丢弃。
- **示例**：

  ```rust
  let text = std::fs::read_to_string(root.join(rel))?;
  let lang = Lang::from_path(Path::new(rel)).unwrap();
  for dep in resolve_imports(lang, rel, &text, &root) {
      // dep 形如 "src/docgen/inventory/data.rs"
      println!("{rel} -> {dep}");
  }
  ```

  实际消费点：`src/docgen/inventory/closure.rs:73` 把结果喂给入口可达 BFS。

## 测试覆盖

本文件**没有内联 `#[test]`**；`resolve_imports` 的行为由 `src/docgen/inventory/tests.rs` 的入口可达三例覆盖（`entry_reachability_follows_python_imports` `:103`、`unreachable_file_is_reported` `:116`、`missing_entry_points_reports_every_file` `:128`），全部只走 **Python** 分支——TS/JS、Go、Rust 三条规则在仓内无用例（见 `_meta/gaps.md`）。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；显式 `RUSTUP_TOOLCHAIN=stable` 可避开「无默认 toolchain」。无平台特定代码（Windows 反斜杠由 `norm` 统一）。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧 `tempfile`（`Cargo.toml:27`）；离线 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 无环境变量或配置键。
- **启停 + 健康检查**：`N/A` — 无进程/端口。
- **种子数据**：`N/A` — 输入是仓库现成源码；用例在 `tempfile` 临时仓自建种子。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::inventory
  ```

  预期（机器相关）：`24 passed; 0 failed`（本单元全部用例都在 `src/docgen/inventory/tests.rs`）。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即 `resolve_imports`（经 `src/docgen/inventory/mod.rs:16` 转发）。

## 证据与缺口

- 结构事实逐行复核：`resolve_imports` `:6`、`candidate` `:13`、逐行扫描 `:37`、Python `:40`、TsJs `:68`、Go `:86`、Rust `:99`、`Other` `:124`、返回 `:127`、`add_relative` `:130`、`cargo_crate_name` `:142`、`resolve_by_suffix` `:159`、`norm` `:183`。
- 开放问题登记在 `_meta/gaps.md` 的 unit-src-34 段（TS/JS、Go、Rust 三条解析规则无用例；Go 同名文件顺序不稳；Rust `#[path]`/内联 `mod` 不解析；逐行文本匹配会误吃注释与字符串）。
- 本轮（unit-src-34）实跑：`cargo test --lib docgen::inventory` → `24 passed; 0 failed; 0 ignored; 595 filtered out`，EXIT=0。
