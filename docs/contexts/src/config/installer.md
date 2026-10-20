---
type: module
unit: unit-src-01
module: src/config/installer.rs
---

# src/config/installer.rs

装包桥接层：把「装哪些包」的决定权留给清单层，把「怎么装」收敛到一处 `pi install` 调用，并对失败负责——错误信息里必须带得上原因。

## 职责

**负责**：

- 构造 `pi install` 命令：profile 目录经环境变量 `PI_CODING_AGENT_DIR` 注入，不靠当前工作目录——`build_install_command`（`src/config/installer.rs:12`）。
- 定义装包抽象 `PackageInstaller`，让「真跑 pi」与「测试假跑」可互换；trait 带 `Send + Sync` 约束，供并发渲染共享同一装包器（`src/config/installer.rs:47`）。
- 真桥 `PiInstallBridge`：逐包执行、非零退出即判失败，并把 stderr 尾部写进错误（`src/config/installer.rs:52`）。
- 假桥 `FakeInstaller`：在 profile 目录内写两份「已装包」记录，不触网、不依赖本机装 pi（`src/config/installer.rs:71`）。
- 失败错误的构造与截断策略（内部函数 `install_failure_error`，`src/config/installer.rs:23`）。

**不负责**：

- 决定装哪些包：必装四件套与差异化包写在清单层 `src/config/manifest.rs`，本模块只接收一个已确定好的包名切片（`src/config/installer.rs:48`）。
- 渲染 profile 目录（写 `<profile>/AGENTS.md`、`<agent-dir>/mcp.json`、`<profile>/models.json`、复制 skills）——那是渲染层 `src/config/render.rs` 的事。
- 判断「包是否已装、要不要跳过安装」——渲染层读 profile 目录里的设置记录自行判断（`<profile>/settings.json`，见 `src/config/render.rs:260`），本模块不做幂等判断。
- 决定 profile 根目录落在哪里、清单怎么落盘——应用层 `src/main.rs:151` 与清单层 `src/config/manifest.rs:681` 负责。

## 公开成员

| 符号 | 种类 | 行 | 一句话 | 详见 |
|---|---|---|---|---|
| `build_install_command` | 公开函数 | `src/config/installer.rs:12` | 造一条 `pi install <pkg>` 命令 | [`build_install_command`](#build_install_command) |
| `PackageInstaller` | 公开 trait | `src/config/installer.rs:47` | 装包抽象（`Send + Sync`） | [`PackageInstaller`](#packageinstaller) |
| `PiInstallBridge` | 公开结构体 | `src/config/installer.rs:52` | 真桥：真跑 `pi install` | [`PiInstallBridge`](#piinstallbridge) |
| `FakeInstaller` | 公开结构体 | `src/config/installer.rs:71` | 假桥：模拟双记录，测试用 | [`FakeInstaller`](#fakeinstaller) |

内部符号（不在公开面上，但决定了失败信息长什么样）：`install_failure_error`（`src/config/installer.rs:23`）。

`build_install_command`、`PackageInstaller`、`PiInstallBridge` 由 `src/config/mod.rs:11` 再导出到 config 模块根；`FakeInstaller` 未在 `src/config/mod.rs` 再导出，测试代码经完整路径引用（`src/config/render.rs:301`）。

## 依赖

- **同仓模块**：`src/error.rs`——错误类型 `DevforgeError`、别名 `Result`、上下文扩展 `Context`（`ctx`），本模块三处引用（`src/config/installer.rs:9`）。
- **外部 crate**：`serde_json`——假桥读写 JSON 记录（`src/config/installer.rs:78`）；`tempfile`——仅单测用，建临时 profile 目录（`src/config/installer.rs:136`）。
- **标准库**：`Path`（profile 目录）、`Command` 与 `Output`（执行命令、取回结果）。
- **外部程序**：`pi`——命令程序名写死为 `pi`（`src/config/installer.rs:13`）。🟡 运行期 `pi` 必须在 `PATH` 上，否则起进程即失败（依据：`Command::new("pi")` 走系统 PATH 查找；本单元未实测该失败路径）。
- **上游调用方**：渲染层 `src/config/render.rs`——渲染 profile 时调装包（`src/config/render.rs:205`）；应用层 `src/main.rs:196`——启动时把真桥注入服务器状态（`src/server/mod.rs:886` 存为 `Arc<dyn PackageInstaller + Send + Sync>`）。
- **外部工具契约（pi 侧）**：🟢 `PI_CODING_AGENT_DIR` 覆盖 pi 的配置目录（默认为家目录下的 .pi/agent），见本机安装的 pi 副本 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/environment-variables.md:81`；`pi install <source>` 是官方安装命令形态，见 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/cli.md:9`。

## 成员详述

### `build_install_command`

#### 摘要

构造一条执行 `pi install` 的命令；profile 目录通过环境变量传给 pi，而不是拼进参数。

#### 行为语义

- 命令程序名恒为 `pi`，参数顺序恒为 `install`、包名（`src/config/installer.rs:13`、`src/config/installer.rs:15`、`src/config/installer.rs:16`）。
- 环境变量 `PI_CODING_AGENT_DIR` 被设为传入的 profile 目录（`src/config/installer.rs:14`）——pi 用它决定「装到哪个 profile 的配置目录里」，这是双记录能落在正确 profile 下的前提。
- 纯函数、无副作用：只构造不执行（`Command` 直到 `output()`/`spawn()` 才真正起进程）。因此可在单测里断言程序名、环境变量、参数而不触网。
- 边界：包名原样透传，不做校验、不做转义；空串包名也会被原样拼进参数。
- 幂等性：无副作用，重复调用只产出等价命令。

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `profile_dir` | `&Path` | 传给 `PI_CODING_AGENT_DIR` 的目录；本函数不要求它存在（存在性由调用方保证） | 无默认值，必填 |
| `package` | `&str` | pi 的包标识（形如 `npm:pi-lens`）；原样透传 | 无默认值，必填 |

#### 返回

`Command`（std::process）——已带好程序名、参数与环境变量的命令对象；调用方自行决定执行方式（真桥用 `output()`）。

#### 异常

N/A（不返回 `Result`）——构造阶段不做 IO，不会失败；失败只发生在后续执行命令时（见 `PiInstallBridge` 的 `install`）。

#### 示例

```rust
let cmd = build_install_command(Path::new("/profiles/plan-agent"), "npm:pi-blackhole");
assert_eq!(cmd.get_program(), "pi");
```

### `PackageInstaller`

#### 职责

装包能力的抽象边界：渲染层只认这个 trait，不关心底下是真调 `pi` 还是测试替身。带 `Send + Sync` 超约束，使同一个装包器实例可以被并发渲染的多个 worker 共享（`src/config/installer.rs:46`）。

#### 构造或初始化

N/A（trait，不实例化）——实现者提供零大小或带状态的结构体即可。

#### 公开成员

| 成员 | 签名 | 语义 |
|---|---|---|
| `install` | `fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>` | 把给定包装进给定 profile 目录；全部成功才返回 `Ok(())`（`src/config/installer.rs:48`） |

#### 生命周期

- 无状态契约：实现应把状态放在自身字段里，不依赖调用顺序。
- 单次调用针对一个 profile 目录；跨 profile 复用同一实例是允许的（渲染层并发就是这么用的，worker 数上限 4，见 `src/config/render.rs:27`）。
- 实现若要「幂等/跳过已装」需自行实现——本 trait 不提供该语义（渲染层另有「已装则跳过」的判断，`src/config/render.rs:457`）。

#### 典型用法

```rust
fn render_one(installer: &dyn PackageInstaller, dir: &Path) -> devforge::error::Result<()> {
    installer.install(dir, &["npm:pi-lens".to_string()])
}
```

### `PiInstallBridge`

#### 职责

生产用的真桥：逐包调用 `pi install`，把包装到指定 profile 目录里；任何一包失败立即中止并上抛带 stderr 尾部的错误。

#### 构造或初始化

N/A（零字段单元结构体）——`PiInstallBridge` 无字段、无状态、无构造参数（`src/config/installer.rs:52`）；直接以值创建即可。

#### 公开成员

| 成员 | 签名 | 语义 |
|---|---|---|
| `install` | `fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>` | 逐包真跑 `pi install`；首个失败包即返回错误（`src/config/installer.rs:55`） |

#### 生命周期

- 无状态、可跨 profile 复用；`Send + Sync` 约束由 trait 提供，可放进 `Arc` 长期持有（`src/server/mod.rs:886`）。
- 每次调用都真的起子进程、真的走网络；耗时随包数线性增长（渲染层为此把并发度压到 4，见 `src/config/render.rs:25`）。
- 失败即返回，不做重试、不做回退（重试策略不在本模块职责内）。

#### 典型用法

```rust
use crate::config::installer::PiInstallBridge;
let bridge = PiInstallBridge;
bridge.install(Path::new("/home/u/.devforge/profiles/coding-agent"), &packages)?;
```

### `FakeInstaller`

#### 职责

测试替身：不调用 pi，改为在 profile 目录里模拟 pi install 的「双记录」效果——`<profile>/settings.json` 的 `packages` 数组 + `<profile>/npm/package.json` 的 `dependencies` 对象（`src/config/installer.rs:68`）。

#### 构造或初始化

N/A（零字段单元结构体，派生了 `Default`）——`FakeInstaller` 无字段、无构造参数（`src/config/installer.rs:71`）。

#### 公开成员

| 成员 | 签名 | 语义 |
|---|---|---|
| `install` | `fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>` | 写两份记录到 profile 目录；不触网、不调用 pi（`src/config/installer.rs:74`） |

#### 生命周期

- 无状态、可跨 profile 复用；可安全用于并发渲染（渲染层测试即如此用，`src/config/render.rs:308`）。
- 副作用：创建/改写 `<profile>/settings.json`、创建 `<profile>/npm/` 目录、创建/改写 `<profile>/npm/package.json`（`src/config/installer.rs:91`、`src/config/installer.rs:96`、`src/config/installer.rs:114`）。
- 幂等性：不是幂等追加——`<profile>/settings.json` 的 `packages` 数组每次调用都会再 push 一遍（不查重，`src/config/installer.rs:89`）；`<profile>/npm/package.json` 的 `dependencies` 按包名 insert，重复调用结果相同（`src/config/installer.rs:112`）。
- 依赖：目标 profile 目录必须已存在（否则写 `<profile>/settings.json` 失败，`src/config/installer.rs:91`）。
- 容错：已存在的 `<profile>/settings.json` / `<profile>/npm/package.json` 若不是合法 JSON，解析失败会被静默退化为空对象（`src/config/installer.rs:78`、`src/config/installer.rs:100`）——原内容被覆盖丢弃。
- 已知边界：若 `<profile>/settings.json` 或 `<profile>/npm/package.json` 的内容是合法 JSON 但不是对象（如数组），`expect("settings 应为对象")` 会 panic（`src/config/installer.rs:85`、`src/config/installer.rs:106`）。

#### 典型用法

```rust
use crate::config::installer::FakeInstaller;
FakeInstaller.install(Path::new("/tmp/profile-a"), &["npm:pi-lens".to_string()])?;
```

### `install_failure_error`（内部）

#### 摘要

把一次失败的 `pi install` 变成一条可诊断的错误：带上包名、退出状态与 stderr 尾部摘要。

#### 行为语义

- stderr 先按 UTF-8 宽松解码（非法字节替换为 U+FFFD）再 `trim()`（`src/config/installer.rs:25`、`src/config/installer.rs:26`）。
- 长度上限按**字节**计：超过 500 字节时只保留尾部 500 字节，并向前推进到 UTF-8 字符边界后再切，输出以 `…` 开头（`src/config/installer.rs:24`、`src/config/installer.rs:27`—`src/config/installer.rs:32`）。🟢 依据：长度判断与切片都作用在字符串的字节长度上（`src/config/installer.rs:27`）。
- stderr 为空（trim 后）时写占位文案「（stderr 为空）」（`src/config/installer.rs:33`、`src/config/installer.rs:34`）。
- 错误类型是 `DevforgeError` 的 `Config` 变体，消息形状为 `pi install <pkg> 失败（exit: <status>）` 换行后接 `stderr 尾部: <tail>`（`src/config/installer.rs:38`—`src/config/installer.rs:42`）。
- 🟡 保留 stderr 尾部是刻意的产品决策（模块注释写明「上游 postinstall 吞掉真实 errno 只留谜语」，`src/config/installer.rs:20`）——依据是注释原文，实际排障收益未在本机复现。

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `pkg` | `&str` | 失败包名，进错误消息 | 无默认值，必填 |
| `output` | `&Output` | 已结束进程的输出；只读 `status` 与 `stderr` | 无默认值，必填 |

#### 返回

`DevforgeError`（`Config` 变体）——不返回 `Result`，调用方直接 `Err(...)` 上抛。

#### 异常

N/A（不返回 `Result`，不 panic）——入参是已完成的进程输出，无可失败的操作。

## 行为规格

| behavior | 落点 | 断言 | 置信度 |
|---|---|---|---|
| `install_command_carries_profile_env` | `src/config/installer.rs:125` | 命令程序名为 `pi`；`PI_CODING_AGENT_DIR` 等于传入的 profile 目录；参数序列为 `install`、包名 | 🟢 `src/config/installer.rs:125` |
| `fake_installer_writes_dual_records` | `src/config/installer.rs:135` | 假桥写出的 `<profile>/settings.json` 的 `packages` 数组有 2 项；`<profile>/npm/package.json` 的 `dependencies` 含两个包名 | 🟢 `src/config/installer.rs:135` |
| `failure_error_carries_stderr_tail` | `src/config/installer.rs:174` | 错误串含 `pi install npm:pi-lens 失败` 与 stderr 原文片段 | 🟢 `src/config/installer.rs:174` |
| `failure_error_truncates_to_tail` | `src/config/installer.rs:183` | 2000 字符 stderr 的末行内容仍在错误串里，且错误串总长 < 800 | 🟢 `src/config/installer.rs:183` |
| `failure_error_handles_empty_stderr` | `src/config/installer.rs:193` | 空 stderr 时错误串含 `stderr 为空` | 🟢 `src/config/installer.rs:193` |

## 错误矩阵

| 场景 | 触发点 | 返回 | 错误体形状 | 客户端应如何处理 |
|---|---|---|---|---|
| 起进程失败（`pi` 不在 PATH 等） | `src/config/installer.rs:58` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `执行 pi install <pkg>`，内层为 `Io` | 确认 `pi` 已在 PATH 且可执行；包名拼写错误时 pi 仍会起进程，改包名不会报这条错 |
| `pi install` 非零退出 | `src/config/installer.rs:60` | `Err` | `DevforgeError` 的 `Config` 变体，消息为 `pi install <pkg> 失败（exit: <status>）` + `stderr 尾部: <tail>`（`src/config/installer.rs:38`） | 直接读错误里的 stderr 尾部——它就是上游 postinstall 的真实失败原因（超 500 字节时只留尾部） |
| profile 目录不存在（假桥） | `src/config/installer.rs:91` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `写 settings` | 先建好 profile 目录再调 `install`（渲染层已保证） |
| 建 npm 目录失败（假桥） | `src/config/installer.rs:96` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `建 npm 目录` | 检查 profile 目录写权限 |
| 写 package.json 失败（假桥） | `src/config/installer.rs:114` | `Err` | `DevforgeError` 的 `WithContext` 变体，上下文 `写 package.json` | 同上（磁盘满/只读） |
| settings.json 是合法 JSON 但不是对象（假桥） | `src/config/installer.rs:85` | panic | `expect("settings 应为对象")` 触发，不返回错误 | 把 profile 的设置文件改成 JSON 对象（顶层是数组/字符串时才会触发） |

## 运行与验证

- **环境要求**：Rust 工具链（edition 2024，见 `Cargo.toml`）；本单元无独立进程与端口。
- **依赖安装**：仓库根目录 `cargo build` 即可（依赖 `serde`、`serde_json`、`thiserror`；单测另需 `tempfile`，见 `Cargo.toml`）。
- **配置项**：本模块无配置项；唯一外部约定是环境变量 `PI_CODING_AGENT_DIR`（由本模块**写入**子进程，不是本模块读取的配置）。
- **启停与健康检查**：N/A——本模块是库，没有可启停的进程；健康检查等价于「渲染层调 `install` 不报错」。
- **种子数据**：N/A——无数据库；假桥的输入是「已存在的 profile 目录」，空目录即可（`src/config/installer.rs:138`）。
- **测试运行**：`cargo test config::installer`（覆盖本模块 5 个单测）。🔴 本机无 Rust 工具链（`rustup toolchain list` 输出 no installed toolchains），本轮未实际执行——见 `_meta/gaps.md` #1。
- **公开接口清单**：`build_install_command`、`PackageInstaller`（`install`）、`PiInstallBridge`（`install`）、`FakeInstaller`（`install`）——见「公开成员」表。
