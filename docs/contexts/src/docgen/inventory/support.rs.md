---
type: module
module_path: src/docgen/inventory/support.rs
unit: unit-src-34
description: 约定面与测试规格抽取——env 键 / SQL 表视图 / Makefile 目标 / package.json scripts / 支持文件枚举 / 测试文件判定 / 测试名反抽
---

# src/docgen/inventory/support.rs

清单里**除源文件符号之外的两类条目**都出自这里：①「约定面」（T002）——代码里读的环境变量键、SQL DDL 的表/视图、Makefile 目标、package.json 的 scripts、.env.example 的键，它们对应清单的 `Config` 与 `Data` 条目；②「测试即可执行规格」（T003）——从测试代码反抽行为名，对应 `Behavior` 条目。

全部是**逐行文本启发式**：不建语法树、不执行代码、不联网、不起进程；只有 `scan_support_files` 读目录（其余函数接字符串）。四个「抽取」函数返回 `Vec<(行号, 名字)>`，行号 1-based，供清单条目直接落 path:line。

## 职责

**负责**：
- 配置项抽取：`config_keys`（`src/docgen/inventory/support.rs:8`，四语言环境变量读取点）与 `env_file_keys`（`:26`，.env.example / .env.sample）。
- 数据层抽取：`sql_objects`（`:48`，`create table` / `create view`）。
- 构建/启动命令抽取：`make_targets`（`:73`）、`package_scripts`（`:97`）。
- 支持文件枚举：`scan_support_files`（`:148`）。
- 测试规格抽取：`is_test_file`（`:183`，判定哪些源文件算测试）、`test_specs`（`:202`，四语言的测试名）。
- 一行内的键提取细节：私有 `keys_after`（`:118`）。

**不负责**：不建清单条目、不分配单元（`scanner` 的 `build_inventory` 负责把返回值包成 `InventoryEntry`）；不判这些键/表是否真被用；不解析 SQL 语法（连 `ALTER TABLE` 都不认）；不跑测试。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `config_keys` | 函数 | [config_keys](#config_keys) |
| `env_file_keys` | 函数 | [env_file_keys](#env_file_keys) |
| `sql_objects` | 函数 | [sql_objects](#sql_objects) |
| `make_targets` | 函数 | [make_targets](#make_targets) |
| `package_scripts` | 函数 | [package_scripts](#package_scripts) |
| `scan_support_files` | 函数 | [scan_support_files](#scan_support_files) |
| `is_test_file` | 函数 | [is_test_file](#is_test_file) |
| `test_specs` | 函数 | [test_specs](#test_specs) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| `keys_after` | `src/docgen/inventory/support.rs:118` | 在一行里找每个 marker 之后的键：剥掉 `[" ` / `(' ` / `[` / `(` / 引号等前缀，取到引号/右括号/`]`/`,`/`;`/空格/`}` 为止，字符白名单 `[A-Za-z0-9_.-]`，行内去重 |
| `NAMES` | `src/docgen/inventory/support.rs:149` | `scan_support_files` 内的文件名白名单常量：`Makefile` / `makefile` / package.json（另加 *.sql / `*.env.example` / `*.env.sample` 后缀） |

## 依赖

- **上游（谁调用）**：`scanner` 的 `build_inventory`——`config_keys` / `env_file_keys` / `sql_objects` / `make_targets` / `package_scripts` 用来生成 `Config` / `Data` 条目，`is_test_file` 用来把测试文件分流，`test_specs` 用来生成 `Behavior` 条目，`scan_support_files` 用来扩展「支持文件」范围（`src/docgen/inventory/scanner.rs:6` 起的 `build_inventory`）。
- **下游（它依赖谁）**：`imports` 的 `norm`（`:4`，路径归一化）、`scanner` 的 `is_skipped_dir`（`:5`，跳过 `docs` / `target` / `node_modules` 等目录）、经 `use super::*`（`:3`）取得 `Lang` / `Path` / `Result` / `serde_json`。文件系统只读。
- **外部依赖**：`serde_json`（仅 `package_scripts`）；无网络、无进程。

## 核心口径（解释层，逐条带置信度）

- 🟢 **行号一律 1-based 且指向「那一行本身」**：所有抽取函数都用 `enumerate()` 后 `idx + 1`（`:19`、`:30`、`:54`、`:77`、`:222`）；`package_scripts` 特殊——它在全文里找第一处包含 `"键名"` 的行（`:107`-`:111`），找不到就给 `1`。🟢 CONFIRMED `src/docgen/inventory/support.rs:19`、`:107`
- 🟢 **环境变量抽取是「marker + 紧随其后的字面量」**：四语言的 marker 表写死（Python os.environ.get( / os.environ[ / os.getenv(；TS/JS process.env. / `process.env[`；Go os.Getenv( / os.LookupEnv(；Rust env::var( / env::var_os(；`Other` 空表，`:10`-`:16`），命中后由 `keys_after` 剥前缀、按白名单截名（`:120`-`:143`）。因此动态拼出来的键（`os.environ[prefix + key]`）抽不到。🟢 CONFIRMED `src/docgen/inventory/support.rs:10`、`:118`
- 🟢 **.env 键的字符白名单比代码侧更严**：`env_file_keys` 只接受 `[A-Za-z0-9_.]`（`:36`-`:40`），不含 `-`；而代码侧 `keys_after` 的白名单含 `-`（`:136`）。两者对同一个「带连字符的键」会给不同结论。🟢 CONFIRMED `src/docgen/inventory/support.rs:36`、`:136`
- 🟢 **SQL 只认「行首的 create table / create view」**：先 `trim()` 再 `to_ascii_lowercase()`（`:51`），因此缩进写法可以、`CREATE OR REPLACE VIEW` 不行（前缀不匹配，`:52`-`:55`）；`if not exists` 会被剥掉（`:57`）；名字取到第一个空白或 `(` 为止并去掉引号/反引号/分号（`:58`-`:63`）。🟢 CONFIRMED `src/docgen/inventory/support.rs:51`、`:52`、`:57`
- 🟢 **Makefile 只认「冒号前是合法目标名」的行**：跳过以 Tab 开头的 recipe 行与 `#` 注释（`:75`-`:77`）；取 `:` 之前的部分，要求非空、不含 `=`、不含空格、字符均在 `[A-Za-z0-9_.-]`（`:80`-`:87`）——因此 `VAR := x`（赋值）、`a b: x`（多目标）、`src/x.o: src/x.c`（带斜杠）都不算目标。🟢 CONFIRMED `src/docgen/inventory/support.rs:75`、`:80`、`:85`
- 🟢 **package.json 走真 JSON 解析，但行号靠字符串搜索**：`serde_json` 的 `from_str` 失败 → 返回空表（`:98`-`:100`）；没有 `scripts` 或不是对象 → 空表（`:101`-`:103`）；每个键再全文找 `"键名"` 定位行号（`:107`-`:111`）。因此同名键出现在别处（如 `dependencies` 里同名包）会指向错误的行。🟢 CONFIRMED `src/docgen/inventory/support.rs:98`、`:101`、`:107`
- 🟢 **支持文件枚举 = 3 个固定名 + 3 个后缀**：`Makefile` / `makefile` / package.json（`:149`）+ *.sql / `*.env.example` / `*.env.sample`（`:165`-`:167`）；遍历是 DFS 栈、跳过 `is_skipped_dir` 命中的目录（`:158`-`:161`），结果 `sort()` 后返回（`:177`）。🟢 CONFIRMED `src/docgen/inventory/support.rs:149`、`:158`、`:177`
- 🟢 **测试文件判定是「路径片段 + 文件名模式」而非语言感知**：任一目录片段是 `tests` / `test` / `__tests__`（`:187`），或 stem 以 `_test` 结尾 / 以 `test_` 开头 / 以 .test / .spec 结尾，或文件名以 _test.go 结尾（`:189`-`:194`）。`src/docgen/inventory/tests.rs` 这种「单文件测试模块」不算——它没有 `_test` 后缀也不在 `tests/` 目录下。🟢 CONFIRMED `src/docgen/inventory/support.rs:187`、`:189`
- 🟢 **测试名反抽只取名字不取断言**：函数注释写明「只取名字与位置，不照抄断言」（`:198`-`:201`）；Rust 分支要求 `#[test]` 后紧跟的 `fn `（`:235`-`:246`），因此 `#[tokio::test]` / `#[rstest]` 不被识别。🟢 CONFIRMED `src/docgen/inventory/support.rs:198`、`:235`
- 🟡 **`package_scripts` 的键序来自 `serde_json` 的 Map 实现**：`scripts` 的 `keys` 的顺序在默认特性下是 `BTreeMap`（字典序），但一旦开启 preserve_order 特性就变成插入序；仓内没有断言依赖这个顺序，文档不应承诺「按 package.json 里的书写顺序」。🟡 INFERRED（依据：`serde_json` 未在 `Cargo.toml` 显式开 preserve_order）
- 🟡 **`scan_support_files` 的 `Result` 永不返回 `Err`**：`read_dir` 失败 `continue`（`:155`-`:157`），`strip_prefix` 失败只是不收该文件（`:170`-`:172`），函数体只以 `Ok(out)` 收尾（`:178`）。调用方无法区分「没有支持文件」与「目录读不动」。🟡 INFERRED（依据：函数体无 `?` 与 `Err(...)` 构造）

## 抽取规则速查

| 函数 | 输入 | 识别形态 | 反例（抽不到） |
|---|---|---|---|
| `config_keys` | 源码文本 + `Lang` | `os.environ.get("K")`、process.env.K、`os.Getenv("K")`、`env::var("K")` 等 marker 后紧跟的键 | 拼接键、`Other` 语言、自定义封装函数 |
| `env_file_keys` | .env.example 文本 | `K=V`、`export K=V`；跳过空行与 `#` 注释 | 键含 `-`、只有 `K` 没有 `=` |
| `sql_objects` | SQL 文本 | 行首 `create table` / `create view`（大小写不敏感） | `create or replace view`、`alter table`、多行 DDL |
| `make_targets` | Makefile 文本 | name: 且名字是 `[A-Za-z0-9_.-]+` | recipe 行（Tab 起）、`VAR := x`、src/x.o: |
| `package_scripts` | package.json 文本 | `scripts` 对象的每个键 | 非法 JSON、无 `scripts` |
| `test_specs` | 测试源码文本 + `Lang` | `def test_*` / `it("…")` / `test("…")` / `func Test*` / `#[test]` + `fn` | `#[tokio::test]`、`describe(...)` 块名 |

## 数据模型

`N/A` —— 本文件不定义类型；所有抽取函数统一返回 `Vec<(usize, String)>`（行号 + 名字），`scan_support_files` 返回 `Result<Vec<String>>`（相对路径，`/` 分隔、字典序），`is_test_file` 返回 `bool`。

---

### config_keys

- **摘要**：从源码文本里抽出「读环境变量」的键及其行号。（🟢 CONFIRMED `src/docgen/inventory/support.rs:8`）
- **行为语义**：按 `lang` 取 marker 表（`:10`-`:16`；`Other` 为空表，直接得到空结果）；逐行对每个 marker 反复 `find`（同一行多个键都能抽到），键的解析交给 `keys_after`（`:118`）。**幂等、无副作用**（纯字符串处理）。
- **参数**：`lang: Lang`（决定 marker 表）、`text: &str`（文件全文）。
- **返回**：`Vec<(usize, String)>` —— `(1-based 行号, 键名)`；同一行内同名的重复出现被去重（`:139`-`:142`），不同行各自保留。
- **异常**：`N/A`（不返回 `Result`）。
- **示例**：`config_keys(Lang::Rust, "let k = env::var(\"DEVFORGE_HOME\");")` → `[(1, "DEVFORGE_HOME")]`。

### env_file_keys

- **摘要**：从 .env.example / .env.sample 文本里抽出键及其行号。（🟢 CONFIRMED `src/docgen/inventory/support.rs:26`）
- **行为语义**：逐行 `trim`；空行与 `#` 开头的行跳过（`:29`-`:32`）；剥掉可选的 `export `（`:33`）；`split_once('=')` 取等号左边，要求非空且字符全在 `[A-Za-z0-9_.]`（`:34`-`:40`）。等号右边内容完全忽略。纯函数。
- **参数**：`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(行号, 键名)`，输入顺序。
- **异常**：`N/A`。
- **示例**：`env_file_keys("# K=V\nFOO=1\nexport A.B=2\n")` → `[(2, "FOO"), (3, "A.B")]`。

### sql_objects

- **摘要**：从 SQL 文本里抽出 `create table` / `create view` 的名字与行号。（🟢 CONFIRMED `src/docgen/inventory/support.rs:48`）
- **行为语义**：逐行 `trim` 后转小写（`:51`），匹配 `create table ` 或 `create view ` 前缀（`:52`-`:55`），剥 `if not exists `（`:57`），名字截到第一个空白或 `(`（`:58`-`:63`），再去掉两端引号/反引号/分号/单引号（`:63`）；名字为空则不收。纯函数。
- **参数**：`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(行号, 对象名)`；名字保留原文大小写？**否**——`name` 取自小写化后的行，因此返回的是小写名（`:51` 的 `lower`）。
- **异常**：`N/A`。
- **示例**：`sql_objects("CREATE TABLE IF NOT EXISTS notes (id int);")` → `[(1, "notes")]`。

### make_targets

- **摘要**：从 Makefile 文本里抽出目标名与行号。（🟢 CONFIRMED `src/docgen/inventory/support.rs:73`）
- **行为语义**：跳过 Tab 开头的 recipe 行与 `#` 注释行（`:75`-`:77`）；`split_once(':')` 取冒号前部分 `trim` 作名字；要求非空、不含 `=`、不含空格、字符全在 `[A-Za-z0-9_.-]`（`:80`-`:87`）。纯函数。
- **参数**：`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(行号, 目标名)`，输入顺序。
- **异常**：`N/A`。
- **示例**：`make_targets("build:\n\tcargo build\n")` → `[(1, "build")]`；`VAR := 1` 与 `src/x.o: src/x.c` 都不收。

### package_scripts

- **摘要**：从 package.json 文本里抽出 `scripts` 的每个脚本名与「首次出现」的行号。（🟢 CONFIRMED `src/docgen/inventory/support.rs:97`）
- **行为语义**：`serde_json` 的 `from_str` 整文件解析（`:98`），取 `scripts` 对象（`:101`）；对每个键在原文里找第一处包含 `"键名"` 的行（`:107`-`:111`），找不到给 `1`。纯函数。
- **参数**：`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(行号, 脚本名)`；空表表示 JSON 非法或没有 `scripts`。
- **异常**：`N/A`（JSON 错误被吞成空表，`:98`-`:100`）。
- **示例**：`package_scripts("{\"scripts\":{\"test\":\"vitest\"}}")` → 一条 `("test")`，行号指向包含 `"test"` 的那一行。

### scan_support_files

- **摘要**：递归枚举「约定面文件」的相对路径（排序）。（🟢 CONFIRMED `src/docgen/inventory/support.rs:148`）
- **行为语义**：从 `root` 起 DFS（`Vec` 栈，`:151`），目录按 `is_skipped_dir` 剪枝（`:160`-`:166`）；文件名命中 `Makefile` / `makefile` / package.json 或以 .sql / .env.example / .env.sample 结尾即收（`:167`-`:170`），再 `strip_prefix` 归一（`:171`-`:175`）；路径 `strip_prefix(root)` 后经 `norm` 归一（`/` 分隔）；最后 `sort()`（`:177`）。只读。
- **参数**：`root: &Path`（仓库根）。
- **返回**：`Result<Vec<String>>` —— 相对路径列表（字典序）。
- **异常**：实际不会返回 `Err`（目录读失败被 `continue` 掉，`:155`-`:157`）；签名保留 `Result` 供调用方统一 `?`。
- **示例**：仓根有 `Makefile`、scripts/build.sh（不收）、db/schema.sql（收）→ 结果含 `Makefile` 与 db/schema.sql。

### is_test_file

- **摘要**：判断一个相对路径是否算测试文件（决定是否走 `test_specs` 抽行为条目）。（🟢 CONFIRMED `src/docgen/inventory/support.rs:183`）
- **行为语义**：路径任一 `/` 片段是 tests / test / __tests__（`:190`）；或 stem 以 `_test` 结尾 / 以 `test_` 开头 / 以 .test / .spec 结尾（`:191`-`:194`）；或文件名以 _test.go 结尾（`:195`）。纯函数、大小写敏感。
- **参数**：`rel: &str`（相对路径，`/` 分隔）。
- **返回**：`bool`。
- **异常**：`N/A`。
- **示例**：`is_test_file("tests/e2e/test_api.py") == true`；`is_test_file("src/docgen/inventory/tests.rs") == false`（`src/docgen/inventory/tests.rs` 是文件名不是目录，且 stem 不以 `_test` 结尾）。

### test_specs

- **摘要**：从测试源码里抽出测试名与行号（行为条目的名字来源）。（🟢 CONFIRMED `src/docgen/inventory/support.rs:202`）
- **行为语义**：逐行按语言分支——Python：`def ` / `async def ` 且名字以 `test` 开头（`:208`-`:215`）；TS/JS：`it(` / `test(` 后的第一个引号串（`:216`-`:226`）；Go：`func Test…`（`:227`-`:233`）；Rust：先见到 `#[test]`（或 `#[test …]`）置位，下一行是 `fn ` 才收（`:234`-`:246`）；`Other` 不收（`:247`）。**只取名字，不解析断言**。
- **参数**：`lang: Lang`、`text: &str`。
- **返回**：`Vec<(usize, String)>` —— `(行号, 测试名)`；Rust 分支的行号指向 `fn` 那一行（`:243`），而不是 `#[test]` 行。
- **异常**：`N/A`。
- **示例**：`test_specs(Lang::Rust, "#[test]\nfn adds() {}\n")` → `[(2, "adds")]`。

## 测试覆盖

本文件没有内联 `#[test]`；行为由 `src/docgen/inventory/tests.rs` 覆盖（该文件归本单元）：

| 用例 | 覆盖的函数 | 位置 |
|---|---|---|
| `config_keys_cover_four_languages` | `config_keys`（四语言 marker） | `src/docgen/inventory/tests.rs:234` |
| `env_example_keys_ignore_comments_and_blank_lines` | `env_file_keys` | `src/docgen/inventory/tests.rs:251` |
| `sql_ddl_yields_tables_and_views` | `sql_objects`（table + view） | `src/docgen/inventory/tests.rs:259` |
| `make_targets_and_package_scripts_extracted` | `make_targets` + `package_scripts` | `src/docgen/inventory/tests.rs:268` |
| `test_specs_extracted_per_language` | `test_specs` | `src/docgen/inventory/tests.rs:329` |
| `test_files_get_behavior_entries` | `is_test_file` + 行为条目落库 | `src/docgen/inventory/tests.rs:356` |
| `test_file_detection_covers_common_layouts` | `is_test_file`（多种布局） | `src/docgen/inventory/tests.rs:382` |
| `convention_entries_land_in_inventory_with_units` | 全部约定面函数 → 条目 + 单元号 | `src/docgen/inventory/tests.rs:280` |

`scan_support_files` 在仓内**没有直接用例**（它的剪枝依赖 `is_skipped_dir`，而 `skips_vendor_and_hidden_dirs` `:162` 只覆盖源文件扫描）——见 `_meta/gaps.md`。

## 测试运行（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；测试侧 `tempfile`（`Cargo.toml:27`）；离线 `CARGO_NET_OFFLINE=true`。
- **配置项**：`N/A` — 无环境变量读进本文件（抽取的是别人代码里的键名，不是本进程的配置）。
- **启停 + 健康检查**：`N/A` — 无进程/端口。
- **种子数据**：`N/A` — 输入是仓库里的 Makefile / package.json / SQL / 测试源码；用例在 `tempfile` 临时仓自建。
- **测试运行**：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib docgen::inventory
  ```

  预期（机器相关）：`24 passed; 0 failed`。
- **公开接口清单**：`N/A`（无 HTTP/CLI 端点）；Rust 面即上表 8 个名字（经 `src/docgen/inventory/mod.rs:13` 转发，`is_test_file` 也在其中）。

## 证据与缺口

- 结构事实逐行复核：`config_keys` `:8`、`env_file_keys` `:26`、`sql_objects` `:48`、`make_targets` `:73`、`package_scripts` `:97`、`keys_after` `:118`、`scan_support_files` `:148`、`NAMES` `:149`、`is_test_file` `:183`、`test_specs` `:202`；跨子模块导入 `:4`、`:5`。
- 开放问题登记在 `_meta/gaps.md` 的 unit-src-34 段（`scan_support_files` 无用例且 `Result` 无 `Err` 路径、`package_scripts` 行号靠字符串搜索、`sql_objects` 返回小写名、.env 与代码侧键字符白名单不一致）。
- 本轮（unit-src-34）实跑：`cargo test --lib docgen::inventory` → `24 passed; 0 failed; 0 ignored; 595 filtered out`，EXIT=0。
