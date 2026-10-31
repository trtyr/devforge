//! A4 依赖管理读面（P019 配置台·依赖管理页）：外部依赖清单 + 存活/版本探测 +
//! 日志桥接。
//!
//! 口径（P019 用户定稿）：只读监控——**无卸载、无 AI 自动修复**；下载/更新是
//! 写操作+长任务，留待后续任务化（本模块只有读面）。
//! 依赖清单与 SOP 出场表同哲学：静态写死（decisions/002）。探测经
//! [`DependencyProbe`] trait——生产 [`RealProbe`]（tokio Command + 3s 超时），
//! 测试注入 Fake（不依赖本机真装了什么）。

use serde::Serialize;

/// 一个外部依赖的探测结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeOutcome {
    /// 命令可执行且正常退出（exit 0）。
    pub alive: bool,
    /// 版本串（stdout 首行 trim；探测失败 = None）。
    pub version: Option<String>,
    /// 探测失败原因（spawn 失败 / 超时 / 非零退出）。
    pub error: Option<String>,
}

/// 探测器抽象（生产 = 真执行；测试 = Fake 注入）。
pub trait DependencyProbe: Send + Sync {
    fn probe(&self, cmd: &str, args: &[&str]) -> ProbeOutcome;
}

/// 生产探测器：`<cmd> <args>` 带超时（探测不该卡住 HTTP 请求）。
pub struct RealProbe;

impl DependencyProbe for RealProbe {
    fn probe(&self, cmd: &str, args: &[&str]) -> ProbeOutcome {
        let outcome = std::thread::scope(|scope| {
            // 阻塞探测放独立线程 + 接收通道，外层限时等待——std 无 wait_timeout，
            // 不为此引 tokio 依赖进同步逻辑。
            let (tx, rx) = std::sync::mpsc::channel();
            let cmd = cmd.to_string();
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            scope.spawn(move || {
                let out = std::process::Command::new(&cmd).args(&args).output();
                let _ = tx.send(out);
            });
            rx.recv_timeout(std::time::Duration::from_secs(3))
        });
        match outcome {
            Ok(Ok(output)) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let version = stdout.lines().next().map(|l| l.trim().to_string());
                ProbeOutcome {
                    alive: true,
                    version: version.filter(|v| !v.is_empty()),
                    error: None,
                }
            }
            Ok(Ok(output)) => ProbeOutcome {
                alive: false,
                version: None,
                error: Some(format!("exit {}", output.status.code().unwrap_or(-1))),
            },
            Ok(Err(e)) => ProbeOutcome {
                alive: false,
                version: None,
                error: Some(format!("spawn 失败：{e}")),
            },
            Err(_) => ProbeOutcome {
                alive: false,
                version: None,
                error: Some("探测超时（3s）".into()),
            },
        }
    }
}

/// 依赖日志桥接（只读）：候选日志目录的存在性与概要（文件数 + 最近文件名）。
/// 目录不存在 = absent（「没有日志的就算了，不强求」——P019 定稿）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogBridge {
    pub dir: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_file: Option<String>,
}

fn log_bridge(dir: &std::path::Path) -> LogBridge {
    let mut bridge = LogBridge {
        dir: dir.display().to_string(),
        exists: false,
        file_count: None,
        latest_file: None,
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return bridge;
    };
    let mut files: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, e.file_name().to_string_lossy().to_string()))
        })
        .collect();
    bridge.exists = true;
    bridge.file_count = Some(files.len());
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    bridge.latest_file = files.into_iter().next().map(|(_, name)| name);
    bridge
}

/// 依赖声明（静态表）。
pub struct DependencySpec {
    pub id: &'static str,
    pub label: &'static str,
    pub probe_cmd: &'static str,
    pub probe_args: &'static [&'static str],
    /// 已知日志目录候选（「没有日志的就算了」——不存在就如实报 absent）。
    pub log_dirs: &'static [&'static str],
}

/// 日志目录候选展开：`$HOME` 占位 → 实际主目录（HOME 是 OS 层兜底，非
/// DEVFORGE_* env，零 env 红线不涉及）；HOME 不可用 → None（该候选跳过）。
fn expand_log_dir(spec: &str) -> Option<std::path::PathBuf> {
    if let Some(rest) = spec.strip_prefix("$HOME/") {
        let home = std::env::var("HOME").ok().filter(|h| !h.trim().is_empty())?;
        Some(std::path::PathBuf::from(home).join(rest))
    } else {
        Some(std::path::PathBuf::from(spec))
    }
}

/// 外部依赖静态清单（写死；与 SOP 出场表同哲学）。devforge 自身不在清单
/// （监控者不监控自己；服务状态走 A7 /server/ops）。
pub const DEPENDENCIES: &[DependencySpec] = &[
    DependencySpec {
        id: "pi",
        label: "pi agent（agent 运行时）",
        probe_cmd: "pi",
        probe_args: &["--version"],
        log_dirs: &["$HOME/.pi/agent/logs"],
    },
    DependencySpec {
        id: "codegraph",
        label: "codegraph（代码结构索引）",
        probe_cmd: "codegraph",
        probe_args: &["--version"],
        log_dirs: &[],
    },
    DependencySpec {
        id: "git",
        label: "git（版本控制基座）",
        probe_cmd: "git",
        probe_args: &["--version"],
        log_dirs: &[],
    },
];

/// 组装依赖视图（纯逻辑，probe 注入可测）。
pub fn list_dependencies(probe: &dyn DependencyProbe) -> Vec<serde_json::Value> {
    DEPENDENCIES
        .iter()
        .map(|spec| {
            let outcome = probe.probe(spec.probe_cmd, spec.probe_args);
            let logs: Vec<LogBridge> = spec
                .log_dirs
                .iter()
                .filter_map(|d| expand_log_dir(d))
                .map(|d| log_bridge(&d))
                .collect();
            serde_json::json!({
                "id": spec.id,
                "label": spec.label,
                "probe_cmd": format!("{} {}", spec.probe_cmd, spec.probe_args.join(" ")),
                "alive": outcome.alive,
                "version": outcome.version,
                "probe_error": outcome.error,
                "logs": logs,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
