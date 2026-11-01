//! 生产验证门（autonomy-gaps gap-E）：全量 `cargo test` 硬门 + 可选 clippy / doc-lint 门。
//!
//! 归因二分（前置裁决④，缺省口径见 [`crate::integration`] 模块头）：
//! - 断言红 / 编译失败（**普通非零退出码**）→ [`DevforgeError::Quality`]：候选自身问题 → 修复队列；
//! - 起不来 / 构建目录不可写 / 超时被杀 / 被信号杀 → [`DevforgeError::System`]：基础设施抖动 → 重试一次。
//!
//! 终止边界（运行事故教训 1「测试 spawn 长驻进程必须带终止边界」）：每步自带超时，
//! 超时**杀进程组**——`cargo test` 会派生测试二进制、测试二进制又会 spawn 子进程，
//! 只 kill cargo 会留下孙进程，集成 worktree 删除随即失败、下一轮集成被残骸卡死。
//!
//! 测试注入面（验收 5 的超时用例靠它，**不读真实 cargo、不走环境变量**）：
//! [`GateSpec`] 直接构造步骤（argv + 秒级超时）。

use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{DevforgeError, Result};

use super::settings::IntegrationSettings;

/// 每道门的默认超时（**生产默认**：常量，不做环境变量/配置项——与 F14 零环境变量方向一致；
/// 测试经 [`GateSpec`] 注入秒级超时）。
pub const DEFAULT_GATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 失败摘要保留的输出尾部长度（摘要要进产物 JSON，不能把整段编译日志塞进去）。
const OUTPUT_TAIL_LIMIT: usize = 4000;

/// 等待子进程退出的轮询间隔（超时判定的粗粒度——门本身是分钟级，50ms 无感）。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 一道门步骤（命令 + 超时）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateStep {
    /// 步骤名（执行记录与失败摘要里可读）。
    pub name: String,
    /// 命令行（`argv[0]` = 可执行文件，其余为参数）。
    pub argv: Vec<String>,
    /// 该步超时（超时杀进程组）。
    pub timeout: Duration,
}

impl GateStep {
    /// 构造一步。
    pub fn new(name: impl Into<String>, argv: Vec<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            argv,
            timeout,
        }
    }
}

/// 一组门步骤（按序执行，任一步红即停）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GateSpec {
    /// 步骤序列。
    pub steps: Vec<GateStep>,
}

/// 单步执行结果（执行记录：命令、退出码、耗时、输出尾部）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateStepOutcome {
    /// 步骤名。
    pub name: String,
    /// 命令行。
    pub argv: Vec<String>,
    /// 退出码（被信号杀 = None）。
    pub exit_code: Option<i32>,
    /// 耗时（ms）。
    pub elapsed_ms: u64,
    /// 输出尾部（stdout + stderr 合并）。
    pub output_tail: String,
    /// 是否超时被杀。
    pub timed_out: bool,
}

/// 一组门的执行结果（全绿才有值——红时错误里带同款摘要）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GateReport {
    /// 逐步执行记录。
    pub steps: Vec<GateStepOutcome>,
}

impl GateSpec {
    /// 生产门构造：**硬门 = 全量 `cargo test`**，另两道可选门按设置追加（缺省只有硬门）。
    ///
    /// 可选门（前置裁决②）：clippy = `cargo clippy --all-targets -- -D warnings`，
    /// doc-lint = `cargo doc --no-deps`。
    pub fn production(settings: &IntegrationSettings) -> Self {
        let mut steps = vec![GateStep::new(
            "cargo_test",
            vec!["cargo".into(), "test".into()],
            DEFAULT_GATE_TIMEOUT,
        )];
        if settings.gates.clippy {
            steps.push(GateStep::new(
                "clippy",
                vec![
                    "cargo".into(),
                    "clippy".into(),
                    "--all-targets".into(),
                    "--".into(),
                    "-D".into(),
                    "warnings".into(),
                ],
                DEFAULT_GATE_TIMEOUT,
            ));
        }
        if settings.gates.doc_lint {
            steps.push(GateStep::new(
                "doc_lint",
                vec!["cargo".into(), "doc".into(), "--no-deps".into()],
                DEFAULT_GATE_TIMEOUT,
            ));
        }
        Self { steps }
    }

    /// 跑门（顺序执行，任一步红即停）。
    ///
    /// `cargo_target_dir` = 外置构建目录（`{plans_root}/integrations/{project}/cargo-target`）：
    /// 集成 worktree 每轮 `--detach` 新建，`target/` 留在里面等于每轮全量重编译，
    /// 且与主工作树的增量构建互踩。
    pub fn run(&self, cwd: &Path, cargo_target_dir: Option<&Path>) -> Result<GateReport> {
        if let Some(dir) = cargo_target_dir {
            prepare_target_dir(dir)?;
        }
        let mut outcomes = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let outcome = run_step(step, cwd, cargo_target_dir)?;
            outcomes.push(outcome);
        }
        Ok(GateReport { steps: outcomes })
    }
}

/// 构建目录准备：不存在则建，且必须可写。
///
/// 不可写 = 基础设施故障（→ System，不记质量账）——判据在跑 cargo **之前**，
/// 不靠解析 cargo 的错误输出去猜「编译失败」还是「目录不可写」。
fn prepare_target_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| {
        DevforgeError::System(format!("验证门构建目录 {} 创建失败：{e}", dir.display()))
    })?;
    let probe = dir.join(".devforge-write-probe");
    std::fs::write(&probe, b"probe").map_err(|e| {
        DevforgeError::System(format!("验证门构建目录 {} 不可写：{e}", dir.display()))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// 跑一步：返回执行记录（绿）或错误（红：Quality = 候选自身问题，System = 基础设施）。
fn run_step(
    step: &GateStep,
    cwd: &Path,
    cargo_target_dir: Option<&Path>,
) -> Result<GateStepOutcome> {
    let Some((program, args)) = step.argv.split_first() else {
        return Err(DevforgeError::System(format!(
            "验证门「{}」步骤为空（无命令行）",
            step.name
        )));
    };
    let started = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cargo_target_dir {
        cmd.env("CARGO_TARGET_DIR", dir);
    }
    // 进程组：超时能连孙进程一起杀（cargo → 测试二进制 → 测试 spawn 的子进程）。
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| {
        DevforgeError::System(format!("验证门「{}」无法启动（{program}）：{e}", step.name))
    })?;

    // 管道必须边跑边读：管道缓冲写满会让子进程阻塞在写上（死锁）。
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let out_handle = thread::spawn(move || drain_tail(stdout_pipe.as_mut()));
    let err_handle = thread::spawn(move || drain_tail(stderr_pipe.as_mut()));

    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if started.elapsed() >= step.timeout {
                    kill_process_group(&child);
                    let _ = child.wait();
                    let out = out_handle.join().unwrap_or_default();
                    let err = err_handle.join().unwrap_or_default();
                    let elapsed_ms = started.elapsed().as_millis() as u64;
                    tracing::error!(
                        component = "integration",
                        event = "integration.gate.timeout",
                        step = %step.name,
                        argv = ?step.argv,
                        elapsed_ms,
                        "验证门超时：已杀进程组（不留孙进程）"
                    );
                    return Err(DevforgeError::System(format!(
                        "验证门「{}」超时（>{:?}，已杀进程组）：\n{}",
                        step.name,
                        step.timeout,
                        tail_of(&out, &err)
                    )));
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                kill_process_group(&child);
                let _ = child.wait();
                return Err(DevforgeError::System(format!(
                    "验证门「{}」等待子进程失败：{e}",
                    step.name
                )));
            }
        }
    };

    let out = out_handle.join().unwrap_or_default();
    let err = err_handle.join().unwrap_or_default();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let outcome = GateStepOutcome {
        name: step.name.clone(),
        argv: step.argv.clone(),
        exit_code: status.code(),
        elapsed_ms,
        output_tail: tail_of(&out, &err),
        timed_out: false,
    };

    if let Some(signal) = status.signal() {
        // 被信号杀（OOM / 外部 kill）——基础设施形态，不是候选的质量问题。
        tracing::error!(
            component = "integration",
            event = "integration.gate.signal",
            step = %outcome.name,
            signal,
            elapsed_ms,
            "验证门被信号杀死（按基础设施故障归因）"
        );
        return Err(DevforgeError::System(format!(
            "验证门「{}」被信号 {} 杀死（基础设施故障）：\n{}",
            outcome.name,
            signal,
            outcome.output_tail
        )));
    }

    match outcome.exit_code {
        Some(0) => {
            tracing::info!(
                component = "integration",
                event = "integration.gate.pass",
                step = %outcome.name,
                elapsed_ms,
                "验证门通过"
            );
            Ok(outcome)
        }
        Some(code) => {
            // 普通非零退出码 = 断言红 / 编译失败 → 候选自身问题（质量）。
            tracing::warn!(
                component = "integration",
                event = "integration.gate.fail",
                step = %outcome.name,
                exit_code = code,
                elapsed_ms,
                "验证门失败（质量归因）"
            );
            Err(DevforgeError::Quality(format!(
                "验证门「{}」失败（命令 {:?}，退出码 {code}，耗时 {elapsed_ms}ms）：\n{}",
                outcome.name, outcome.argv, outcome.output_tail
            )))
        }
        None => Err(DevforgeError::System(format!(
            "验证门「{}」退出状态不可读（基础设施故障）",
            outcome.name
        ))),
    }
}

/// 杀进程组（超时边界）。
///
/// 只 kill 直接子进程会留下孙进程（测试二进制 spawn 的子进程）——它们继续占着
/// 集成 worktree 目录，下一轮 `worktree remove` 失败。
fn kill_process_group(child: &std::process::Child) {
    let pgid = child.id() as i32;
    // SAFETY: kill(2) 只用到进程组号与 SIGKILL 两个标量，无空指针参与；
    // pgid 来自本进程 spawn 的子进程（`process_group(0)` 保证它是本进程组的组长），
    // 负号语义 = 杀整个进程组。
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

/// 读干一个管道，只保留尾部（防无界内存；管道写满会让子进程阻塞）。
fn drain_tail<R: Read>(r: Option<&mut R>) -> String {
    let Some(r) = r else { return String::new() };
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > OUTPUT_TAIL_LIMIT * 2 {
                    let drop = buf.len() - OUTPUT_TAIL_LIMIT;
                    buf.drain(..drop);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

/// stdout + stderr 合并尾部（失败摘要里两路都要能看到）。
fn tail_of(stdout: &str, stderr: &str) -> String {
    let mut s = String::new();
    if !stderr.trim().is_empty() {
        s.push_str("--- stderr ---\n");
        s.push_str(stderr.trim_end());
        s.push('\n');
    }
    if !stdout.trim().is_empty() {
        s.push_str("--- stdout ---\n");
        s.push_str(stdout.trim_end());
    }
    s.trim().to_string()
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod gate_tests;
