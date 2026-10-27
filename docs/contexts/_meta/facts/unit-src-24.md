# 勘察事实 — unit-src-24（src/config/installer.rs）

> **本单元范围（断点复核口径）**：inventory 把 `unit-src-24` 定为 `src/config/installer.rs`（本文）；而 plan 侧的 `state.json` 把 `unit-src-24` 的产出记为 `src/config/manifest.rs.md`（inventory 里那是 `unit-src-25`）。两套编号不一致，故本轮两个文件一并重核：本文件是 installer 事实，manifest 事实在 `_meta/facts/unit-src-25.md`（本轮按 HEAD `19b3717`、948 行整体重写）。

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/config/installer.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。
> 复核时间：本轮重写；`wc -l` = **198 行**（上一版事实记 230 行，已作废），下列行号全部按当前文件重核。

## 文件与模块位置

- 文件：`src/config/installer.rs`，198 行，无子模块。
- 声明：`src/config/mod.rs:6` `pub mod installer;`。
- 转发面：`src/config/mod.rs:11` `pub use installer::{build_install_command, PackageInstaller, PiInstallBridge};`（**`FakeInstaller` 不在转发面**，测试直连模块路径）。
- `use`（`:6`-:9`）：`std::path::Path`、`std::process::Command`、`crate::error::{Context as _, DevforgeError, Result}`。
- 文件头注释（`:1`-:4`）：pi install 一把梭（用户拍板 2026-10-16）；称 pi install 自动完成双记录（`settings.json` 的 `packages` 数组 + `npm/package.json` 的 `dependencies`）；单测用 `FakeInstaller` 模拟，不碰网络。

## 公开条目

| 符号 | 行 | 形状 |
|---|---|---|
| `build_install_command` | `:12` | `pub fn(&Path, &str) -> Command` |
| `PackageInstaller` | `:47` | `pub trait PackageInstaller: Send + Sync`，单方法 `install(&self, &Path, &[String]) -> Result<()>`（`:48`） |
| `PiInstallBridge` | `:52` | `pub struct PiInstallBridge;`（单元结构体）；`impl` 在 `:54`，方法 `:55`-65 |
| `FakeInstaller` | `:71` | `#[derive(Default)]`（`:70`）`pub struct FakeInstaller;`；`impl` `:73`，方法 `:74`-117 |

## 私有条目

- `install_failure_error(pkg: &str, output: &std::process::Output) -> DevforgeError`（`:23`）：`const MAX_STDERR_BYTES: usize = 500`（`:24`）；`from_utf8_lossy(&output.stderr)` → `trim()`；超长 → 起点 `len-500` 起做 `is_char_boundary` 前移（`:29`）→ 前缀 `…`；空 → `（stderr 为空）`（`:34`）；否则原文；产出 `DevforgeError::Config("pi install {pkg} 失败（exit: {status}）\nstderr 尾部: {tail}")`。

## 函数内部可见的分支

- `build_install_command`（`:12`-:18`）：`Command::new("pi")` → `.env("PI_CODING_AGENT_DIR", profile_dir)` → `.arg("install")` → `.arg(package)`。不设 `current_dir`，不过 shell，不 spawn。
- `PiInstallBridge::install`（`:55`-65）：`for pkg in packages` → `build_install_command(profile_dir, pkg).output()`（`:57`-59`）`.ctx(format!("执行 pi install {pkg}"))?`；`!output.status.success()` → `return Err(install_failure_error(pkg, &output))`（`:60`-62`，**立即返回，后续包不再尝试**）；全成功 `Ok(())`。
- `FakeInstaller::install`（`:74`-117）：
  - 记录 1：读 `<profile>/settings.json`；不存在 → `json!({})`；存在但读失败 → `.ctx("读 settings")?` 上抛（`:77`-79`）；解析失败 → `unwrap_or(json!({}))`（`:79`）；`entry("packages")` 缺失插 `[]`（`:83`-87`）；逐包 `push`（`:88`-90`）；pretty 写回（`:91`-92`）。
  - 记录 2：`create_dir_all(<profile>/npm)`（`:96`）；读 `npm/package.json`——**不存在** → 骨架 `{"name":"pi-extensions","private":true,"dependencies":{}}`（`:102`，`else` 分支）；**存在可解析** → 用文件内容；**存在但解析失败** → 空对象 `{}`（`:100`，`unwrap_or`）；读失败 → `.ctx("读 package.json")?`（`:99`）；`entry("dependencies")` 缺失插 `{}`（`:105`-108`）；逐包 `insert(pkg, "*")`（`:109`-113`）；pretty 写回（`:114`-115`）。
  - `expect` panic 面四处：`"settings 应为对象"`（`:85`）、`"packages 应为数组"`（`:89`）、`"package.json 应为对象"`（`:106`）、`"dependencies 应为对象"`（`:111`）。
- 追加语义：`packages` 数组 `push` 不去重；`dependencies` 是 map，重复键覆盖。

## 测试（`#[cfg(test)]` `:120` 起，`mod tests` `:121`，5 例）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `install_command_carries_profile_env` | `:125` | `get_program()=="pi"`；`PI_CODING_AGENT_DIR == /profiles/plan-agent`；`get_args() == ["install","npm:pi-blackhole"]` |
| `fake_installer_writes_dual_records` | `:135` | `settings.json` 的 `packages` 长度 2；`npm/package.json` 的 `dependencies` 含两键 |
| `failure_error_carries_stderr_tail` | `:174` | 错误串含 `pi install npm:pi-lens 失败` 与 stderr 文本 `Failed to move` |
| `failure_error_truncates_to_tail` | `:183` | 2000 字节 `x` + `真原因在最后一行` → 含末行且 `err.len() < 800` |
| `failure_error_handles_empty_stderr` | `:193` | 空 stderr → 含 `stderr 为空` |

- 辅助 `output_with_stderr(stderr: &[u8], raw_exit: i32) -> std::process::Output`（`:161`-171）：`#[cfg(unix)]` `ExitStatusExt::from_raw(raw_exit)`（`:162`-163`）/`#[cfg(windows)]` `from_raw(raw_exit as u32)`（`:164`-165`）；stdout 空。
- 5 例都不 spawn 子进程、不联网；只有 `fake_installer_writes_dual_records` 建临时目录（`tempfile`）。

## 仓内消费者（grep `--include=*.rs`，排除本文件）

- `src/config/mod.rs:6` 声明、`:11` re-export（3 个名字）。
- `src/config/render.rs:8` 引入 `PackageInstaller`；`:31` `Renderer.installer: &'a dyn PackageInstaller`；`:222` `self.installer.install(&dir, &packages)?`（`render_agent`）；`:239` `render_all`；`:244` `workers = MAX_RENDER_CONCURRENCY(4).min(len).max(1)`（常量定义 `:27`）；`:248` `std::thread::scope`。
- `src/config/render.rs:198`-199 组包（`base_packages` + `extra_packages`）；`:202`-203 幂等判据（`packages` 全在 previous 中 + `npm/package.json` 存在）；`:205`-221 跳过安装只回写 settings；`:263` `rendered.push(res?)`（按清单顺序取首个错误）；`:277` `recorded_packages` 读 `settings.json`。
- `src/config/render.rs:318`（测试）引入 `FakeInstaller`；`:325` fixture `installer: &FakeInstaller`；`:429`/`:438` `CountingInstaller` / `FailingInstaller` 两个测试实现。
- `src/server/mod.rs:26` 引入 `PackageInstaller`；`:131` `pub installer: Arc<dyn PackageInstaller + Send + Sync>`；`:162` `ServerState::new` 的 `installer` 参数。
- `src/main.rs:201` 生产注入 `Arc::new(devforge::config::installer::PiInstallBridge)`。
- `src/server/handlers.rs:675`-680 `render_body` 构造 `Renderer { installer: st.installer.as_ref(), … }`；`:618` `put_manifest`；`:639` `std::thread::spawn` 起后台渲染；`:687`-688 失败转 SSE `config.render_failed`。
- 测试注入：`src/server/handlers.rs:699`/`:731`、`src/server/tests/mod.rs:134`、`tests/f6_observability/fixture.rs:31` 用 `FakeInstaller`。
- `build_install_command` 唯一生产调用点：本文件 `:57`；外部仅 `src/config/mod.rs:11` re-export（仓内无第二个调用点）。

## 可重放命令（本轮实跑）

```
TMPDIR=/var/tmp/dfz-src24b/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable \
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/var/tmp/dfz-src24b/target \
cargo test --lib config::installer
```

- 结果：`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 593 filtered out; finished in 0.00s`，EXIT=0；同一 target 目录从零编译 36.95s、暖跑 0.00s（本轮复跑；旧稿记 44.18s 为上一台/上一轮数据）。另有同位取证：`cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 550 filtered out`，EXIT=0。
- `rustc 1.99.0 (b940084d7 2026-09-28)`、`cargo 1.99.0 (5f94df478 2026-08-27)`；`edition = "2024"`（`Cargo.toml:4`）。

## 外部依赖与环境

- 外部命令：`pi`（运行时由 `Command::new` 从 PATH 解析）；无 shell、无 `current_dir` 设置。
- 写入子进程的环境变量：`PI_CODING_AGENT_DIR` = profile 目录。
- crate 依赖：`serde_json`（假桥读写双记录）；`crate::error` 的 `Context` trait（`.ctx`）与 `DevforgeError::Config`。
- 测试依赖：`tempfile`（`Cargo.toml:27`）、`std::os::unix|windows::process::ExitStatusExt`（`#[cfg]` 二选一）。
- 落盘产物（假桥口径）：`<profile>/settings.json` 的 `packages` 数组；`<profile>/npm/package.json` 的 `dependencies` 对象（值 `"*"`）。
