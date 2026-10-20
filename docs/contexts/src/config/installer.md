---
type: module
unit: unit-src-01
module: src/config/installer.rs
---

# src/config/installer.rs

装包桥接层：把「装哪些包」的决定权留在清单层，把「怎么装」收敛到一处 `pi install` 调用，并对失败负责——错误信息里必须带得上原因。

## 职责

**负责**：

- 构造 `pi install` 命令：profile 目录经环境变量 `PI_CODING_AGENT_DIR` 注入，而不是靠当前工作目录——`build_install_command`（`src/config/installer.rs:12`）。
- 定义装包抽象 `PackageInstaller`，让「真跑 pi」与「测试假跑」可互换；trait 带 `Send + Sync` 约束，供并发渲染共享同一个装包器（`src/config/installer.rs:47`）。
- 真桥 `PiInstallBridge`：逐包执行、非零退出即判失败并把 stderr 尾部写进错误（`src/config/installer.rs:52`）。
- 假桥 `FakeInstaller`：在 profile 目录内写两份「已装包」记录，不触网、不依赖本机装 `pi`（`src/config/installer.rs:71`）。
- 失败错误的构造与截断策略（内部函数 `install_failure_error`，`src/config/installer.rs:23`）。

**不负责**：

- 决定装哪些包：必装四件套与差异化包写在清单层 `src/config/manifest.rs`，本模块只接收一个已经确定好的包名切片。
- 渲染 profile 目录（写 `<profile>/AGENTS.md`、`<profile>/mcp.json`、`<profile>/models.json`、复制 skills 等）——那是渲染层 `src/config/render.rs` 的事。
- 决定 profile 根目录落在哪里——应用层 `src/main.rs` 负责。

## 公开成员

| 符号 | 种类 | 行 | 一句话 | 详见 |
|---|---|---|---|---|
| `build_install_command` | 公开函数 | `src/config/installer.rs:12` | 造一条 `pi install <pkg>` 命令 | [build_install_command](#build_install_command) |
| `PackageInstaller` | 公开 trait | `src/config/installer.rs:47` | 装包抽象（`Send + Sync`） | [PackageInstaller](#packageinstaller) |
| `PiInstallBridge` | 公开结构体 | `src/config/installer.rs:52` | 真桥：真跑 `pi install` | [PiInstallBridge](#piinstallbridge) |
| `FakeInstaller` | 公开结构体 | `src/config/installer.rs:71` | 假桥：模拟双记录，测试用 | [FakeInstaller](#fakeinstaller) |

内部符号（不在公开面上，但决定了失败信息长什么样）：`install_failure_error`（`src/config/installer.rs:23`）。

## 依赖

- **同仓模块**：`src/error.rs`——错误类型 `DevforgeError`、别名 `Result`、上下文扩展 `Context`（`ctx`）。
- **外部 crate**：`serde_json`——假桥读写 JSON 记录（`src/config/installer.rs:77`）。
- **标准库**：`Path`（std::path，profile 目录）、`Command` 与 `Output`（std::process，执行与取回结果）。
- **外部程序**：`pi`——命令程序名写死为 `pi`（`src/config/installer.rs:13`）。🟡 由此推断运行期 `pi` 必须在 `PATH` 上，否则起进程就失败（依据：`Command::new("pi")` 的进程查找规则；本机未安装 pi，未实测）。
- **测试依赖**：`tempfile`（dev-dependency，测试建临时 profile 目录用，`src/config/installer.rs:136`）。
- **上游调用方**：渲染层 `src/config/render.rs`（渲染 profile 时在 `src/config/render.rs:205` 调装包）、应用层 `src/main.rs`（启动时把 `PiInstallBridge` 注入服务器状态）。

## 成员详述

### `build_install_command`

#### 摘要

构造一条执行 `pi install` 的命令；profile 目录通过环境变量传给 pi，而不是拼进参数。

#### 行为语义

- 命令程序名恒为 `pi`，参数顺序恒为 `install`、包名（`src/config/installer.rs:13`）。
- 环境变量 `PI_CODING_AGENT_DIR` 被设为传入的 profile 目录（`src/config/installer.rs:14`）——pi 用它决定「装到哪个 profile 的配置目录里」，这是双记录能落在正确 profile 下的前提。
- 纯函数、不产生副作用：只构造不执行（`Command` 直到 `output()`/`spawn()` 才真正起进程）。因此可在单测里断言程序名、环境变量、参数而不触网。
- 边界：包名原样透传，不做校验、不做转义；空串包名也会被原样拼进参数。

#### 参数

| 参数 | 类型 | 约束 | 默认 |
|---|---|---|---|
| `profile_dir` | `&Path` | 传给 `PI_CODING_AGENT_DIR` 的目录；不要求存在（存在性由调用方保证） | 无默认值，必填 |
| `package` | `&str` | pi 的包标识（如 `npm:pi-lens`）；原样透传 | 无默认值，必填 |

#### 返回

`Command`（std::process）——已带好程序名、参数与环境变量的命令对象；调用方自行决定执行方式（真桥用 `output()`）。

#### 异常

N/A（不返回 `Result`）——构造阶段不做 IO，不会失败；失败只发生在后续执行命令时（见 `PiInstallBridge` 的 `install` 方法）。

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
| `install` | `fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>` | 把给定包装进给定 profile 目录；全部成功才返回 `Ok(())` |

#### 生命周期

- 无状态契约：实现应把状态放在自身字段里，不依赖调用顺序。
- 单次调用针对一个 profile 目录；跨 profile 复用同一实例是允许的（渲染层并发就是这么用的）。
- 实现若要「幂等/跳过已装」需自行实现——本 trait 不提供该语义（渲染层另有「已装则跳过」的判断，`src/config/render.rs:457`）。

#### 典型用法

```rust
fn render_one(installer: &dyn PackageInstaller, dir: &Path) -> devforge::error::Result<()> {
    installer.install(dir, &["npm:pi-lens".to_string()])
}
```

### `PiInstallBridge`

#### 职责

生产用的真桥：逐包调用 `pi install`，把 pi 装到 profile 目录里。

#### 构造或初始化

单元结构体（无字段）——直接 `PiInstallBridge` 即可创建，不需要任何初始化（`src/config/installer.rs:52`）。

#### 公开成员

- `install`（`impl PackageInstaller`）：`src/config/installer.rs:55`。

#### 生命周期

- 零字段的轻量类型，可随服务器状态长期持有（`src/main.rs:196` 启动时注入）。
- 每个包起一个独立子进程；进程生命周期短于单次调用。

#### 典型用法

```rust
let bridge = PiInstallBridge;
bridge.install(Path::new("/home/u/.devforge/profiles/plan-agent"), &packages)?;
```

### `FakeInstaller`

#### 职责

测试替身：不调用 pi，改为在 profile 目录里模拟 pi install 的「双记录」效果——`<profile>/settings.json` 的 `packages` 数组 + `<profile>/npm/package.json` 的 `dependencies` 对象（`src/config/installer.rs:68`）。

#### 构造或初始化

单元结构体 + `#[derive(Default)]`——直接写 `FakeInstaller`，或用它的 `default()` 构造，皆可（`src/config/installer.rs:70`）。

#### 公开成员

- `install`（`impl PackageInstaller`）：`src/config/installer.rs:74`。
- 副作用：创建/改写 `<profile>/settings.json`、创建 `<profile>/npm/` 目录、创建/改写 `<profile>/npm/package.json`。

#### 生命周期

- 无状态，可反复使用；每次 `install` 读改写目标文件。
- 幂等性：不是幂等追加——`<profile>/settings.json` 的 `packages` 数组每次调用都会再 push 一遍（不查重）；`<profile>/npm/package.json` 的 `dependencies` 是按包名插入，重复调用结果相同（`src/config/installer.rs:88`、`src/config/installer.rs:109`）。
- 依赖：目标 profile 目录必须已存在（否则写 `<profile>/settings.json` 失败）。

#### 典型用法

```rust
FakeInstaller.install(&profile, &["npm:pi-blackhole".into(), "npm:pi-fff".into()])?;
```

### `install_failure_error`（内部符号）

#### 摘要

把一次失败的 `pi install` 过程结果转成 `DevforgeError` 的 `Config` 变体，错误文本里带上退出码与 stderr 尾部（`src/config/installer.rs:23`）。

#### 行为语义

- stderr 先按 UTF-8 宽松解码（非法字节替换而非报错），再 `trim()`（`src/config/installer.rs:25`）。
- 长度超过 500 字节时只保留尾部，并在前面加 `…`；截断点会回退到 UTF-8 字符边界，不会切坏多字节字符（`src/config/installer.rs:27`）。
- stderr 为空时写「（stderr 为空）」占位，避免错误信息出现空的「原因」栏（`src/config/installer.rs:33`）。
- 错误消息模板：`pi install {pkg} 失败（exit: {status}）\nstderr 尾部: {tail}`（`src/config/installer.rs:38`）。

## 行为规格

对应清单 `src/config/installer.rs` 的 behavior 条目：

| 行为 | 断言 | 证据 |
|---|---|---|
| `install_command_carries_profile_env` | 命令程序名为 `pi`；`PI_CODING_AGENT_DIR` 等于传入的 profile 目录；参数为 `install` + 包名 | 🟢 `src/config/installer.rs:125` |
| `fake_installer_writes_dual_records` | 假桥写出两份记录：`<profile>/settings.json` 的 `packages` 数组、`<profile>/npm/package.json` 的 `dependencies` | 🟢 `src/config/installer.rs:135` |
| `failure_error_carries_stderr_tail` | 失败错误含 `pi install <pkg> 失败` 且含 stderr 原文片段 | 🟢 `src/config/installer.rs:174` |
| `failure_error_truncates_to_tail` | 超长 stderr 只保尾部（尾部中文完整、错误总长 < 800 字节） | 🟢 `src/config/installer.rs:183` |
| `failure_error_handles_empty_stderr` | 空 stderr 时错误含「stderr 为空」 | 🟢 `src/config/installer.rs:193` |

补充事实：

- 双记录是 pi 自己的行为，桥接只是在 profile 的配置目录语境下调用它（模块头注释 `src/config/installer.rs:1`）。🟢 假桥按同一形状模拟，落点为 `<profile>/settings.json` 与 `<profile>/npm/package.json`（`src/config/installer.rs:76`、`src/config/installer.rs:95`）。
- 失败即中止：真桥遇到第一个失败包就返回错误，后续包不再装（`src/config/installer.rs:61`）。🟢
- 装包错误一律归到 `DevforgeError` 的 `Config` 变体（配置类），不是 `Agent` 变体（`src/config/installer.rs:38`）。🟢
- 🟡 装包失败不区分「包不存在」与「网络/权限问题」——两种情况都表现为 pi 非零退出，devforge 侧只能靠 stderr 文本区分（依据：`src/config/installer.rs:60` 只看退出码是否成功，无错误码分类）。

## 错误与失败模式

| 场景 | 触发条件 | 对外错误 | 证据 |
|---|---|---|---|
| 起进程失败 | `pi` 不在 `PATH` / 权限不足 | `DevforgeError` 的 `Io` 变体被 `ctx("执行 pi install <pkg>")` 包成 `WithContext` | 🟢 `src/config/installer.rs:59` |
| pi 非零退出 | 包名错、postinstall 失败、磁盘满 | `DevforgeError` 的 `Config` 变体，含退出码 + stderr 尾部 | 🟢 `src/config/installer.rs:60` |
| stderr 为空 | pi 静默失败 | 错误里原因栏为「（stderr 为空）」 | 🟢 `src/config/installer.rs:33` |
| stderr 超长 | 上游输出巨量日志 | 只留最后 500 字节，前缀 `…` | 🟢 `src/config/installer.rs:24` |
| 假桥写盘失败 | profile 目录不存在 / 只读 | `DevforgeError` 的 `Io` 变体，经 `ctx("写 settings")`、`ctx("建 npm 目录")`、`ctx("写 package.json")` 包装 | 🟢 `src/config/installer.rs:92` |

## 调用方与边界

- **上游**：`src/config/render.rs:205` 的渲染流程按 profile 调装包；`src/main.rs:196` 启动时把 `PiInstallBridge` 作为生产装包器注入服务器状态。
- **下游**：`pi` 命令行程序（外部依赖，不在本仓）。
- **与清单层的边界**：包名从哪来不由本模块决定——必装四件套与各 Agent 的差异化包在 `src/config/manifest.rs` 定义，本模块只消费传进来的包名切片。
- **与渲染层的边界**：本模块只往 profile 目录里写「已装包记录」；`<profile>/AGENTS.md`、提示词、skills、MCP 文件的渲染在 `src/config/render.rs`。

## 测试落点

- 模块内单测：`src/config/installer.rs:121` 起的 5 个用例（见「行为规格」表）。
- 渲染层用假桥做端到端验证：`src/config/render.rs:301` 引入 `FakeInstaller`。
- 可重放命令：`cargo test --lib config::installer`（本机未执行，原因见下方缺口登记）。
- 🔴 本单元单测未在本机执行：仓库有 Rust 工程（`Cargo.toml` 存在），但本机 `rustup toolchain list` 输出为 `no installed toolchains`，无法编译运行；上述断言均为源码与单测代码的静态事实。
