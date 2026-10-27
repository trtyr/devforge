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

/// 包来源形状校验（自审 minor 34）。
///
/// 边界事实：清单可经 HTTP API 改写，而 `pi install` 会真实执行包的 postinstall 脚本——
/// 即「改清单」等价于「以 devforge 进程身份跑任意代码」。旧口径对来源零校验。
///
/// 规则（只放行 npm 注册表安全子集）：
/// - `name` / `name@version` / `@scope/name` / `@scope/name@version`；
/// - 字符集限 `[A-Za-z0-9._@/-]`（不允空白/控制符/引号/元字符）；
/// - 拒本地与相对绝对路径（`/`、`./`、`../`、`~`）、传输协议前缀（`git+`、`file:`、
///   `http(s):`、`npm:` 等）与选项形态（`-` 开头，防被当成 pi 自己的参数）。
/// 包来源形状校验（自审 minor 34）。
///
/// 边界事实：清单可经 HTTP API 改写，而 `pi install` 会真实执行包的 postinstall 脚本。
/// 旧口径对来源零校验——一个被改写的清单就能把本地任意目录/脚本拉进来执行，
/// 或者把参数写成 `--flag` 让 pi 当成自己的选项。
///
/// 放行口径（与仓内内置清单一致）：`name` / `name@ver` / `@scope/name` /
/// `npm:<name>` / `git:host/path`。拒收：本地与相对/绝对路径（`file:`、`link:`、
/// `/abs`、`./`、`../`、`~`）、`http(s):`/`ssh:` URL、选项形态（`-` 开头）、
/// 以及含空白/控制字符/引号/元字符的串。
///
/// 注：远程包（npm/git）自己的 postinstall 仍会执行——那是 admin 信任模型下的既定行为；
/// 本函数只堵「把安装面变成任意本地路径执行」。
pub fn validate_package_source(pkg: &str) -> Result<()> {
    fn bad(pkg: &str, why: &str) -> DevforgeError {
        DevforgeError::Config(format!(
            "非法请求：扩展包来源 `{pkg}` {why}——只接受 `name` / `@scope/name`（可带 `@版本`）、\
             `npm:<name>` 或 `git:<host>/<path>`；本地路径与 URL 一律拒"
        ))
    }
    if pkg.is_empty() || pkg.trim() != pkg {
        return Err(bad(pkg, "为空或首尾含空白"));
    }
    if pkg.starts_with('-') {
        return Err(bad(pkg, "以 - 开头（会被当成 pi 的命令行选项）"));
    }
    let lower = pkg.to_ascii_lowercase();
    for prefix in ["file:", "link:", "http:", "https:", "ssh:"] {
        if lower.starts_with(prefix) {
            return Err(bad(pkg, "是 URL 而不是包名"));
        }
    }
    // 本地/相对/绝对路径（npm: 与 git: 前缀剥掉后再判）
    let body = lower
        .strip_prefix("npm:")
        .or_else(|| lower.strip_prefix("git:"))
        .unwrap_or(lower.as_str());
    if body.starts_with('/')
        || body.starts_with('.')
        || body.starts_with('~')
        || body.contains("..")
        || body.contains('\\')
    {
        return Err(bad(pkg, "是路径而不是包名"));
    }
    let charset_ok = pkg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '/' | '-' | ':'));
    if !charset_ok {
        return Err(bad(pkg, "含非法字符（空白/控制符/引号/元字符）"));
    }
    Ok(())
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
    // 自审 minor（24）：`ExitStatus` 的 Display 已是「exit status: N」，旧模板再拼
    // 「exit: 」会得到「（exit: exit status: 1）」。用 code() 取纯数字。
    let code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| format!("terminated by signal（{}）", output.status));
    DevforgeError::System(format!(
        "pi install {pkg} 失败（exit code {code}）\nstderr 尾部: {tail}"
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
            // 自审 minor（34）：落地前先验来源形状——postinstall 会以 devforge 进程身份执行。
            validate_package_source(pkg)?;
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
        // 自审 minor（93）：损坏的 settings.json 旧口径被 `.unwrap_or(json!({}))` 静默降级成
        // 空对象再回写——**丢掉其余所有设置键**，既不报错也不留日志。现在：损坏即报错
        // （宁可失败，不可静默吞用户配置）。
        let mut settings: serde_json::Value = if settings_path.exists() {
            let text = std::fs::read_to_string(&settings_path).ctx("读 settings")?;
            serde_json::from_str(&text).map_err(|e| {
                DevforgeError::Config(format!(
                    "settings.json 损坏（{}）：{e}——拒绝回写（会丢掉其余设置键）",
                    settings_path.display()
                ))
            })?
        } else {
            serde_json::json!({})
        };
        // 自审 minor（94）：形状错误不再 expect panic（合法 JSON 但类型不对）
        let obj = settings.as_object_mut().ok_or_else(|| {
            DevforgeError::Config(format!(
                "settings.json 顶层不是对象（{}）",
                settings_path.display()
            ))
        })?;
        let arr = obj
            .entry("packages")
            .or_insert_with(|| serde_json::json!([]));
        let arr = arr.as_array_mut().ok_or_else(|| {
            DevforgeError::Config(format!(
                "settings.json 的 packages 不是数组（{}）",
                settings_path.display()
            ))
        })?;
        for pkg in packages {
            // 自审 minor（95）：重装路径不得在数组里堆重复条目（旧口径只 push）
            if !arr.iter().any(|v| v.as_str() == Some(pkg.as_str())) {
                arr.push(serde_json::json!(pkg));
            }
        }
        let body = serde_json::to_string_pretty(&settings).map_err(|e| {
            DevforgeError::Config(format!("settings.json 序列化失败：{e}"))
        })?;
        std::fs::write(&settings_path, body).ctx("写 settings")?;

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
