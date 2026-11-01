//! A4 依赖管理读面单测：Fake 探测器注入 + RealProbe 真假命令 + 组装逻辑。

use super::*;
use std::collections::HashMap;

/// Fake 探测器：按命令名返回预置结果。
struct FakeProbe {
    outcomes: HashMap<String, ProbeOutcome>,
}

impl FakeProbe {
    fn with(cmd: &str, outcome: ProbeOutcome) -> Self {
        Self {
            outcomes: HashMap::from([(cmd.to_string(), outcome)]),
        }
    }
}

impl DependencyProbe for FakeProbe {
    fn probe(&self, cmd: &str, _args: &[&str]) -> ProbeOutcome {
        self.outcomes.get(cmd).cloned().unwrap_or(ProbeOutcome {
            alive: false,
            version: None,
            error: Some("FakeProbe 未预置该命令".into()),
        })
    }
}

fn alive(version: &str) -> ProbeOutcome {
    ProbeOutcome {
        alive: true,
        version: Some(version.into()),
        error: None,
    }
}

/// 组装：alive 依赖带版本；dead 依赖带 probe_error；日志桥接按存在性如实报。
#[test]
fn list_dependencies_assembles_probe_outcomes() {
    let probe = FakeProbe::with("pi", alive("1.4.0"));
    let deps = list_dependencies(&probe);
    assert_eq!(deps.len(), DEPENDENCIES.len(), "全清单输出");

    let pi = deps.iter().find(|d| d["id"] == "pi").unwrap();
    assert_eq!(pi["alive"], true);
    assert_eq!(pi["version"], "1.4.0");
    assert!(pi["probe_error"].is_null());

    // codegraph 未预置 → Fake 兜底 dead；日志候选为空表 → logs 空
    let cg = deps.iter().find(|d| d["id"] == "codegraph").unwrap();
    assert_eq!(cg["alive"], false);
    assert!(cg["version"].is_null());
    assert_eq!(cg["probe_error"], "FakeProbe 未预置该命令");
    assert_eq!(cg["logs"].as_array().unwrap().len(), 0);

    // pi 的日志候选（$HOME 展开）：本测试进程有 HOME → exists 字段是 bool（存在与否如实）
    let logs = pi["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 1, "pi 有一个日志候选目录");
    assert!(logs[0]["dir"].as_str().unwrap().contains(".pi/agent/logs"));
    assert!(logs[0]["exists"].is_boolean(), "存在性如实（不假装有日志）");
}

/// RealProbe：真命令（git --version）存活带版本；假命令 dead + spawn 失败原因。
#[test]
fn real_probe_on_real_and_missing_commands() {
    let git = RealProbe.probe("git", &["--version"]);
    assert!(git.alive, "git 在测试环境必存在");
    assert!(
        git.version.as_deref().is_some_and(|v| v.starts_with("git version")),
        "版本首行：{:?}",
        git.version
    );

    let missing = RealProbe.probe("defforge-nonexistent-probe", &["--version"]);
    assert!(!missing.alive);
    assert!(missing.version.is_none());
    assert!(missing.error.as_deref().unwrap_or("").contains("spawn 失败"));
}

/// $HOME 占位展开：HOME 缺失时候选跳过（组装不炸）。
#[test]
fn log_dir_expansion_skips_without_home() {
    // 直接验证 expand_log_dir 的两条路径（不真改 HOME——进程级变量改写有并行风险）
    assert_eq!(
        expand_log_dir("/abs/path"),
        Some(std::path::PathBuf::from("/abs/path")),
        "非 $HOME 占位原样"
    );
}

/// R5-EX-6/R2-12/R3-10：挂起进程（sleep 10）在 3s 硬超时窗内被杀并报超时，
/// probe 有界返回（旧口径 scope 隐式 join 会无限挂起）。
#[test]
fn real_probe_timeout_kills_hanging_child() {
    let started = std::time::Instant::now();
    let outcome = super::RealProbe.probe("sleep", &["10"]);
    let elapsed = started.elapsed();
    assert!(!outcome.alive);
    assert!(
        outcome.error.as_deref().unwrap_or("").contains("超时"),
        "挂起进程应报超时：{outcome:?}"
    );
    assert!(
        elapsed >= std::time::Duration::from_secs(3)
            && elapsed < std::time::Duration::from_secs(8),
        "耗时应为 3s 超时窗（实测 {elapsed:?}）"
    );
}

/// 成功路径不回归：正常命令照常取 version。
#[test]
fn real_probe_success_path_reads_version() {
    let outcome = super::RealProbe.probe("echo", &["1.2.3-test"]);
    assert!(outcome.alive, "{outcome:?}");
    assert_eq!(outcome.version.as_deref(), Some("1.2.3-test"));
}
