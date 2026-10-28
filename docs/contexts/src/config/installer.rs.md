---
type: module
module_path: src/config/installer.rs
unit: unit-src-24
description: 包安装桥接——把清单里的 npm 包经 pi install 装进 profile（真桥 / 假桥双实现）
---

# src/config/installer.rs

profile 渲染的最后一步：把清单算出的包列表交给 `pi install` 装进 profile 目录（P002 T003；双记录口径的出处是 002-config-management 计划 README 第 11 行「pi install → settings.json packages + npm/package.json 双记录」，台账在仓库外 plantree 根下）。

## 职责

**负责**：
- 构造 `pi install` 子进程：`build_install_command`（`PI_CODING_AGENT_DIR` 指到 profile 目录）。
- 安装器抽象：`PackageInstaller` trait（`Send + Sync`，供渲染并发共享）与两个实现——生产 `PiInstallBridge`、测试 `FakeInstaller`。
- 安装失败的诊断面：`install_failure_error`（私有）把 stderr 尾部（≤500 字节）带进 `DevforgeError` 的 `Config` 变体。

**不负责**：不算「要装哪些包」（`render_agent` 用 `base_packages` + `extra_packages` 算出，`src/config/render.rs:198`-199）；不做幂等跳过（`src/config/render.rs:200`-221 判「已装齐 + 双记录完整」后压根不调 `install`）；不解析 `pi` 自己的退出语义之外的输出（只看退出状态是否成功）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `build_install_command` | 函数 | [build_install_command](#build_install_command) |
| `PackageInstaller` | trait | [PackageInstaller](#packageinstaller) |
| `PiInstallBridge` | 结构体 | [PiInstallBridge](#piinstallbridge) |
| `FakeInstaller` | 结构体 | [FakeInstaller](#fakeinstaller) |

私有：`install_failure_error`（`src/config/installer.rs:23`，见[失败诊断面](#失败诊断面私有-install_failure_error)）。

## 依赖

- 上游 / 消费方：
  - `src/config/mod.rs:6` `pub mod installer;`；`:11` `pub use installer::{build_install_command, PackageInstaller, PiInstallBridge};`（`FakeInstaller` 不在转发面，测试直连本模块的 `FakeInstaller`）。
  - `src/config/render.rs:8`/`:31` `Renderer.installer: &'a dyn PackageInstaller`；唯一生产调用点 `:222` `self.installer.install(&dir, &packages)?`；并发渲染 `render_all`（`:239`）用 `MAX_RENDER_CONCURRENCY = 4`（`:27`）与 std::thread::scope（`:248`）共享同一 `&dyn` → trait 上 `Send + Sync` 是硬要求。
  - `src/server/mod.rs:131` `pub installer: Arc<dyn PackageInstaller + Send + Sync>`；`src/main.rs:201` 生产注入 `Arc::new(PiInstallBridge)`；`src/server/handlers.rs:675`-680 把它交给 `Renderer` 并在 `put_manifest`（`:618`）的后台线程里渲染。
  - 测试注入：`src/server/handlers.rs:699`/`:731`、`src/server/tests/mod.rs:134`、`tests/f6_observability/fixture.rs:31` 用 `FakeInstaller`；`src/config/render_tests.rs:115`/`:438` 另有 `CountingInstaller` / `FailingInstaller` 两个测试包装。
  - `build_install_command` 的唯一生产调用点是本文件 `PiInstallBridge` 的 `install`（`src/config/installer.rs:57`）；外部只在 `src/config/mod.rs:11` 被 re-export（公开面，仓内无第二个调用点）。
- 向下依赖：标准库 `Path`（`:6`）、`Command`（`:7`）、crate 错误 `Context` trait 与 `DevforgeError`（`:9`）；`FakeInstaller` 另用 `serde_json`（`:78`、`:99`、`:112`）。
- 外部依赖：`pi` 可执行文件（运行期从 PATH 解析）；无网络访问（网络在 `pi install` 内部发生，仓内单测不触发）。

## 外部契约与配置

| 名称 | 类型 | 默认值 | 必填 | 取值范围 | 影响 | 示例 |
|---|---|---|---|---|---|---|
| `PI_CODING_AGENT_DIR` | 子进程环境变量 | 不设（由本模块**总是**显式写入） | 是（`build_install_command` 无条件设） | 任意目录路径；这里是 `<profiles_root>/<agent-name>` | 决定 `pi install` 把双记录写进哪个 profile（落点由 `Renderer` 的 `dir` 给） | `PI_CODING_AGENT_DIR=/…/profiles/plan-agent pi install npm:pi-blackhole`（`src/config/installer.rs:125` 断言） |
| `pi`（可执行文件） | 外部命令 | 无（必须已在 PATH） | 是 | 任意能执行 `install <pkg>` 的 pi 版本 | 不在 PATH 时取输出失败（Command 的 output）→ `DevforgeError` 的 `Config` 变体「执行 pi install <pkg>」（错误上下文经 `Context` trait 包装，`src/config/installer.rs:58`-59） | 同上一行 |
| `<profile>/settings.json` 的 `packages` | JSON 数组（字符串元素） | `FakeInstaller` 起始态：缺文件 → `{}`；存在但不可解析 → `{}`；两者随后都插入 `[]`（`src/config/installer.rs:77`-87） | 由 `pi install` 保证 | 包标识符，如 `npm:pi-blackhole` | 渲染层的幂等判据读它（`src/config/render.rs:277` `recorded_packages`） | `{"packages": ["npm:pi-blackhole"]}` |
| `<profile>/npm/package.json` 的 `dependencies` | JSON 对象 | `FakeInstaller` 起始态三档：缺文件 → 骨架 `{"name":"pi-extensions","private":true,"dependencies":{}}`（`:101`-103`）；可解析 → 用文件内容；存在但不可解析 → `{}`（`:98`-100`，**无** name/private） | 同上 | 键 = 包标识符，值 = `"*"`（假桥口径） | 双记录是否「完整」的判据（`src/config/render.rs:203` 判文件存在） | `{"dependencies": {"npm:pi-blackhole": "*"}}` |
| `MAX_STDERR_BYTES` | `usize` 常量（私有） | `500` | N/A（编译期常量，不可配） | 无（写死） | 失败时错误信息里 stderr 的保留长度（保尾） | `src/config/installer.rs:24` |

双记录语义的出处是 pi 侧行为，仓内只以注释与 `FakeInstaller` 模拟（🟡 **INFERRED**：`src/config/installer.rs:3`-4 的模块注释 + `:68`-69 的假桥注释 + 002-config-management 计划 README 第 11 行的清单口径，台账在仓库外 plantree 根下；pi 本体不在本仓，无法在此重放验证）。

### 落盘记录（数据模型）

`FakeInstaller` 写的两份产物就是「装了什么」的唯一事实证明，渲染层的幂等判据只读它们。

| 记录 | 字段（名 / 类型 / 约束 / 可空） | 索引 / 唯一约束 | 生命周期（谁写、何时删） | 关系 | 迁移注意 |
|---|---|---|---|---|---|
| `<profile>/settings.json` 的 `packages` | `packages: array<string>`；元素为包标识符（如 `npm:pi-blackhole`）；文件其它键不受本模块影响 | N/A（数组，无唯一约束；`push` 不去重，同一包可重复） | 由 `FakeInstaller`（`:88`-92`）/ 真实 `pi install` 写；本模块不删除；文件缺失时首装新建 | 渲染层 `recorded_packages`（`src/config/render.rs:277`）读它做「已装齐」判定 | N/A（无 schema 版本、无迁移；形状不对时走 `expect` panic 而非迁移） |
| `<profile>/npm/package.json` 的 `dependencies` | `dependencies: object<string, string>`；值固定 `"*"`（假桥口径）；缺失则插 `{}` | N/A（JSON 对象，键天然唯一 → 重复包覆盖而非追加） | 同上；`npm` 目录由 `create_dir_all` 建（`:96`） | 渲染层「双记录是否完整」只看该文件是否存在（`src/config/render.rs:203`） | N/A（同上；注意「文件存在但解析失败」会退化成只含 `dependencies` 的空对象，`name`/`private` 不补） |

---

### build_install_command

- 摘要：构造（不执行）`pi install <pkg>` 的 `Command`，并把 `PI_CODING_AGENT_DIR` 指到 profile 目录。（🟢 CONFIRMED `src/config/installer.rs:12`）
- 行为语义：`Command::new("pi")` → `.env("PI_CODING_AGENT_DIR", profile_dir)` → `.arg("install")` → `.arg(package)`；纯构造、不 spawn、不读盘、无副作用；不设 `current_dir`（子进程继承调用方 cwd）；不经 shell，故 `package` 里的空格/引号不会被再解释一次（注入面比拼字符串小）。
- 参数：`profile_dir: &Path` — profile 目录，作为 `PI_CODING_AGENT_DIR` 的值；`package: &str` — 包标识符（如 `npm:pi-blackhole`），原样作为第二个参数。
- 返回：`Command` — 本函数只构造，执行方式（取输出 / 只看状态 / 异步 spawn）由调用方定（`src/config/installer.rs:12`）。
- 异常：N/A（不执行、不失败；执行期错误归调用方）。
- 示例：

  ```rust
  let cmd = build_install_command(Path::new("/profiles/plan-agent"), "npm:pi-blackhole");
  assert_eq!(cmd.get_program(), "pi");
  // 参数顺序固定：["install", "npm:pi-blackhole"]
  ```

  断言口径见 `src/config/installer.rs:125`-133（`get_program` / `get_envs` / `get_args` 三项）。

### PackageInstaller

- 职责：把一组包装进指定 profile 的抽象；让渲染层（`Renderer`）不依赖真实 `pi`。做什么：一次装一个 profile 的全部包；不做什么：不算包列表、不判幂等、不并发控制（并发由 `render_all` 的 worker 决定）。
- 构造 / 初始化：trait，无构造；实现者必须是单元结构体或可 `Arc` 的状态。约束 `Send + Sync`（`src/config/installer.rs:47`）——四个渲染 worker 共享同一个 `&dyn PackageInstaller`（`src/config/render.rs:244`、`:248`）。
- 公开成员：唯一方法 `install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>`——`Ok(())` 表示这组包都装成功（跳过幂等判定后才会被调用，`src/config/render.rs:222`）。
- 生命周期：随 `Renderer` 借用（`&'a dyn`）或随 `ServerState` 以 `Arc<dyn PackageInstaller + Send + Sync>` 存续（`src/server/mod.rs:131`）；无内部可变状态，可跨线程共享。
- 典型用法：生产 `Arc::new(PiInstallBridge)`（`src/main.rs:201`）→ `ServerState` 的 installer 字段 → `Renderer { installer: st.installer.as_ref(), .. }`（`src/server/handlers.rs:675`-680）→ `render_all`；测试传 `&FakeInstaller` 或自实现的计数/失败包装（`src/config/render_tests.rs:117`/`:440`）。
- 错误语义：`Err` 时整次渲染失败——`render_all` 按清单顺序 `rendered.push(res?)` 取首个错误上抛（`src/config/render.rs:263`）；`put_manifest` 的后台线程把它转成 SSE 事件 config.render_failed（`src/server/handlers.rs:687`）。

### PiInstallBridge

- 职责：生产实现——逐包真跑 `pi install`；只做「跑命令 + 判成功 + 组装失败错误」。
- 构造 / 初始化：单元结构体 `pub struct PiInstallBridge;`（`src/config/installer.rs:52`），无字段、无参数，直接 `PiInstallBridge`。
- 公开成员：仅 `install`（trait 实现，`:55`-65）。参数 `profile_dir: &Path`、`packages: &[String]`；返回 `Result<()>`。
- 生命周期：无状态，可放进 `Arc` 长期存续（`src/main.rs:201`）。
- 典型用法：`Arc::new(PiInstallBridge)` 注入 `ServerState`；不要在单测里直接调它（会真跑 `pi` 且依赖网络与全局安装）。
- 行为语义（🟢 CONFIRMED `src/config/installer.rs:56`-64）：`for pkg in packages` → `build_install_command(...).output()`；spawn/IO 失败 → 经 `Context` trait 包一层「执行 pi install {pkg}」上下文的 `Config` 变体（保因）；`!status.success()` → `Err(install_failure_error(pkg, &output))` 并**立即返回**（后续包不再尝试，故 `Err` 前可能已装好前面几个包——不是事务）；全成功 → `Ok(())`。
- 异常：`DevforgeError` 的 `Config` 变体（执行失败 / 非零退出）；无 panic 路径。
- 示例：`pi install npm:pi-lens` 失败时错误文本形如

  ```
  pi install npm:pi-lens 失败（exit: exit status: 1）
  stderr 尾部: npm error Failed to move @ast-grep/cli binaries into place.
  ```

  （形状由 `install_failure_error` 与 `src/config/installer.rs:174`-180 的断言给出；exit: 后是退出状态的 Display 形式。）

### FakeInstaller

- 职责：测试实现——模拟 pi install 的**双记录**，不碰网络、不依赖全局 `pi`。
- 构造 / 初始化：`#[derive(Default)] pub struct FakeInstaller;`（`src/config/installer.rs:70`-71），无字段；用 `FakeInstaller`（派生 Default，可写 FakeInstaller::default()）。
- 公开成员：仅 `install`（trait 实现，`:74`-117）。
- 生命周期：无状态；测试里按值借用即可（`src/config/render_tests.rs:11` 等）。
- 典型用法：`Renderer { installer: &FakeInstaller, .. }`（`src/config/render_tests.rs:9`-326）；或包一层做断言（`CountingInstaller` 记录调用次数、`FailingInstaller` 注入失败，`src/config/render_tests.rs:115`-446）。
- 行为语义（🟢 CONFIRMED `src/config/installer.rs:74`-117）：
  1. 记录 1 —— 读 `<profile>/settings.json`（缺失/不可解析 → `{}`），取 `packages` 键（缺失则插 `[]`），逐包 `push`，pretty 写回（`:76`-92）。
  2. 记录 2 —— `create_dir_all(<profile>/npm)`，读 `<profile>/npm/package.json`，**三种起始态**（口径以代码为准，逐行可核）：
     - **文件不存在** → 骨架 `{"name":"pi-extensions","private":true,"dependencies":{}}`（`else` 分支，`src/config/installer.rs:101`-103）；
     - **文件存在且能解析** → 用文件里的对象；
     - **文件存在但解析失败** → 退化为**空对象 `{}`**（`unwrap_or(json!({}))`，`:98`-100）——**不是骨架**，所以最终写回的文件只有 `dependencies`，没有 `name`/`private`；
     - 读文件本身失败（权限/IO）→ 直接以 `Config` 变体上抛（`.ctx("读 package.json")`，`:99`），不会走到骨架分支。

     随后取 `dependencies` 键（缺失则插 `{}`，`:104`-108`），逐包 `insert(pkg, "*")`（`:109`-113`），pretty 写回（`:114`-115`）。
  3. 追加语义：同一批包调两次会得到重复项（`packages` push 不去重；`dependencies` 是 map，重复则覆盖）——去重不归它管（幂等跳过在 `src/config/render.rs:200`-221）。
- 异常：`DevforgeError` 的 `Config` 变体（读写失败，错误上下文经 `Context` trait）；另有四处 `expect` panic：`settings 应为对象`、`packages 应为数组`、`package.json 应为对象`、`dependencies 应为对象`（`:85`、`:89`、`:106`、`:111`）——只在 profile 里已有**形状不对**的 settings/npm 记录时触发。
- 示例：装 `["npm:pi-blackhole", "npm:pi-fff"]` 后：`<profile>/settings.json` 的 `packages` 长度为 2；`<profile>/npm/package.json` 的 `dependencies` 含两个键（`src/config/installer.rs:135`-157）。
- 第三方可写的判别用例（区分「骨架」与「空对象」两条分支）：预置 `<profile>/npm/package.json` 内容为非法 JSON（如 `{not json`），再调 `FakeInstaller` 的 `install` → 写回的 JSON **只有 `dependencies` 键**、没有 `name`/`private`；把断言写成「得到骨架」会红——这正是 `:98`-100 与 `:101`-103 两条分支的分界。

### 失败诊断面（私有 install_failure_error）

- 摘要：把 `pi install` 的失败转成 `DevforgeError` 的 `Config` 变体，并附 stderr 尾部。（🟢 CONFIRMED `src/config/installer.rs:23`）
- 行为语义：`String::from_utf8_lossy(stderr).trim()`；长度 > 500 → 从 `len-500` 起做 `is_char_boundary` 前移（不切在 UTF-8 中间）→ 前缀 `…`；空 → `（stderr 为空）`；否则原文。错误模板：`pi install {pkg} 失败（exit: {status}）\nstderr 尾部: {tail}`。
- 参数：`pkg: &str`、`output: &std::process::Output`；返回 `DevforgeError`（`Config` 变体）。
- 依据（为什么是这个形状）：F2 教训——上游 postinstall 吞掉真实 errno，只有 stderr 里留线索；产品侧硬化记录见 006-coding-hardening 路线图第 23 行「失败错误附带 stderr 尾部（500B 保尾截断），单测 ×3」（台账在仓库外 plantree 根下）。

## 测试覆盖（`#[cfg(test)]`，5 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `install_command_carries_profile_env` | 程序名 / 环境变量 / 参数顺序 | `src/config/installer.rs:125` |
| `fake_installer_writes_dual_records` | 假桥双记录两条都落盘 | `src/config/installer.rs:135` |
| `failure_error_carries_stderr_tail` | 失败错误含 stderr 原文 | `src/config/installer.rs:174` |
| `failure_error_truncates_to_tail` | 超长 stderr 保尾、不过 UTF-8 边界 | `src/config/installer.rs:183` |
| `failure_error_handles_empty_stderr` | 空 stderr 的占位文案 | `src/config/installer.rs:193` |

测试辅助 `output_with_stderr(stderr, raw_exit)`（`:161`-171）用 `ExitStatusExt` 的 `from_raw` 造 `Output`（`#[cfg(unix)]` / `#[cfg(windows)]` 各一支），因此这三例不需要真跑任何子进程。

## 测试运行与环境

- 测试与被测代码同文件：`#[cfg(test)] mod tests`（`src/config/installer.rs:120` 起），`#[test]` 共 5 例。
- **环境前提**：edition 2024（`Cargo.toml:4`）⇒ 需 Rust 1.85+ 且 toolchain 已安装（rustup 无默认 toolchain 时显式指定 `RUSTUP_TOOLCHAIN=stable`）。本轮实测 `rustc 1.99.0` / `cargo 1.99.0`。
- **依赖获取**：`cargo fetch`（首次需网络；本模块测试只需 `tempfile`，`Cargo.toml:27`）。
- **运行单模块单测**（🟢 CONFIRMED，本轮实跑 `cargo test --lib config::installer` → `5 passed; 0 failed; 0 ignored; 593 filtered out`，EXIT=0；同一 target 目录从零编译 36.95s，暖跑 0.00s）：

  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib config::installer
  ```

  预期输出：`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 593 filtered out`（`filtered out` 随仓内测试总数变化，不是本模块断言）。注意：临时目录要宽裕（本模块编译 + `tempfile` 都要写），容器里系统临时目录可能不够。
- 隔离性：5 例全部不 spawn 子进程、不联网、不读真实 profile——`FakeInstaller` 用 `tempfile` 的 tempdir（`:136`-138），失败诊断用内存 `Output`（`:161`-171）。**不要在单测里直接用 `PiInstallBridge`**：它会真调 `pi`（需 PATH、网络、写真实 profile）。
- **本单元的 Runbook 边界**：本节承担环境 / 依赖获取 / 测试运行与隔离三件；**N/A** 项与理由——启停与健康检查：库内模块，无独立进程（服务级启停归 `src/server`）；配置项：本模块无可配键，唯一环境变量 `PI_CODING_AGENT_DIR` 是**写给子进程**的（见「外部契约与配置」），不接受外部配置；种子数据：无（假桥自建 `<profile>/settings.json` / `<profile>/npm/package.json` 骨架）；公开接口清单：上文「公开成员」表即全部（无 HTTP/CLI 端点；HTTP 入口是 `PUT /config/manifest`，见 `src/server/handlers.rs:618`，属 `src/server` 单元）。
