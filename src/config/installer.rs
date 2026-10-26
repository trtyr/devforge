//! 包安装桥接：pi install 一把梭（用户拍板 2026-10-16）。
//!
//! pi install 自动完成双记录（settings.json `packages` 数组 + `npm/package.json` dependencies），
//! 桥接只需在 profile 的配置目录语境下调用它。单测用 [`FakeInstaller`] 模拟双记录，不碰网络。

use std::path::Path;
use std::process::Command;

use crate::error::{Context as _, DevforgeError, Result};

/// 构造 pi install 命令（纯函数，可测）：`PI_CODING_AGENT_DIR=<profile> pi install <pkg>`。
pub fn build_install_command(profile_dir: &Path, package: &str) -> Command {
    let mut cmd = Command::new("pi");
    cmd.env("PI_CODING_AGENT_DIR", profile_dir)
        .arg("install")
        .arg(package);
    cmd
}

/// 构造安装失败错误：附带 stderr 尾部摘要（F2 教训：上游 postinstall 吞掉
/// 真实 errno 只留谜语，devforge 侧再把 stderr 丢掉就彻底抓瞎——
/// 把「为什么装不上」直接带进错误信息）。
fn install_failure_error(pkg: &str, output: &std::process::Output) -> DevforgeError {
    const MAX_STDERR_BYTES: usize = 500;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    let tail = if trimmed.len() > MAX_STDERR_BYTES {
        let mut start = trimmed.len() - MAX_STDERR_BYTES;
        while start < trimmed.len() && !trimmed.is_char_boundary(start) {
            start += 1;
        }
        format!("…{}", &trimmed[start..])
    } else if trimmed.is_empty() {
        "（stderr 为空）".to_string()
    } else {
        trimmed.to_string()
    };
    // 自审 major：安装失败归 System（可重试）而非 Config——网络抖动/ENOSPC 是
    // 环境瞬时故障，与仓库自身「瞬时故障归 System 可重试」的约定一致。
    DevforgeError::System(format!(
        "pi install {pkg} 失败（exit: {}）\nstderr 尾部: {tail}",
        output.status
    ))
}

/// 带超时的子进程运行（自审 major：pi install 无超时 = 一次网络挂死永久占住
/// 渲染线程，完成通知永不发出）。轮询 try_wait + 后台线程排空管道（防管道满
/// 阻塞子进程）。超时上限可用 `DEVFORGE_INSTALL_TIMEOUT_MS` 调整，0 = 不限时
///（零硬限制哲学）；默认每包 10 分钟。
fn run_with_timeout(mut cmd: Command, pkg: &str) -> Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let timeout_ms: u64 = std::env::var("DEVFORGE_INSTALL_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600_000);
    if timeout_ms == 0 {
        return cmd
            .output()
            .ctx(format!("执行 pi install {pkg}"));
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ctx(format!("执行 pi install {pkg}"))?;
    let mut stdout_pipe = child.stdout.take().expect("stdout 已 piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr 已 piped");
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_thread.join().unwrap_or_default();
                let stderr = stderr_thread.join().unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout: stdout.into_bytes(),
                    stderr: stderr.into_bytes(),
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(DevforgeError::System(format!(
                        "pi install {pkg} 超时（{timeout_ms}ms，DEVFORGE_INSTALL_TIMEOUT_MS 可调，0=不限）——进程已强杀"
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(DevforgeError::System(format!(
                    "pi install {pkg} 等待失败：{e}"
                )));
            }
        }
    }
}

/// 包安装器抽象。真桥 = [`PiInstallBridge`]；测试 = fake（模拟双记录）。
///
/// `Send + Sync` 是 `render_all` 并发渲染的要求（四个 worker 共享 `&dyn PackageInstaller`）。
pub trait PackageInstaller: Send + Sync {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>;
}

/// 真桥：逐包调 pi install（运行期用；单测不真跑——避免网络与全局 pi 依赖）。
pub struct PiInstallBridge;

impl PackageInstaller for PiInstallBridge {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()> {
        // 自审 major：安装段全链路最慢（e2e 实测 8 profile 602s）且失败率最高，
        // 零埋点 = 事件时间线里完全隐身。逐包落 info/失败 WARN。
        tracing::info!(
            component = "installer",
            profile = %profile_dir.display(),
            packages = packages.len(),
            "开始安装 profile 扩展包"
        );
        for pkg in packages {
            let output = run_with_timeout(build_install_command(profile_dir, pkg), pkg)?;
            if !output.status.success() {
                let err = install_failure_error(pkg, &output);
                tracing::warn!(
                    component = "installer",
                    package = %pkg,
                    profile = %profile_dir.display(),
                    error = %err,
                    "扩展包安装失败"
                );
                return Err(err);
            }
            tracing::info!(
                component = "installer",
                package = %pkg,
                profile = %profile_dir.display(),
                "扩展包安装成功"
            );
        }
        Ok(())
    }
}

/// 测试桥：模拟 pi install 的双记录行为——
/// 追加 `<profile>/settings.json` 的 `packages` 数组 + 写 `<profile>/npm/package.json`。
#[derive(Default)]
pub struct FakeInstaller;

impl PackageInstaller for FakeInstaller {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()> {
        // 记录 1：settings.json packages 数组追加
        let settings_path = profile_dir.join("settings.json");
        let mut settings: serde_json::Value = if settings_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&settings_path).ctx("读 settings")?)
                .unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        let arr = settings
            .as_object_mut()
            .expect("settings 应为对象")
            .entry("packages")
            .or_insert_with(|| serde_json::json!([]));
        for pkg in packages {
            arr.as_array_mut().expect("packages 应为数组").push(serde_json::json!(pkg));
        }
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap())
            .ctx("写 settings")?;

        // 记录 2：npm/package.json dependencies
        let npm_dir = profile_dir.join("npm");
        std::fs::create_dir_all(&npm_dir).ctx("建 npm 目录")?;
        let pkg_json_path = npm_dir.join("package.json");
        let mut pkg_json: serde_json::Value = if pkg_json_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&pkg_json_path).ctx("读 package.json")?)
                .unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({"name": "pi-extensions", "private": true, "dependencies": {}})
        };
        let deps = pkg_json
            .as_object_mut()
            .expect("package.json 应为对象")
            .entry("dependencies")
            .or_insert_with(|| serde_json::json!({}));
        for pkg in packages {
            deps.as_object_mut()
                .expect("dependencies 应为对象")
                .insert(pkg.clone(), serde_json::json!("*"));
        }
        std::fs::write(&pkg_json_path, serde_json::to_string_pretty(&pkg_json).unwrap())
            .ctx("写 package.json")?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "installer_tests.rs"]
mod tests;
