//! 验证门单测（gap-E）：绿/红归因 / 可选门 / 超时终止边界。

use std::time::{Duration, Instant};

use super::*;
use crate::error::DevforgeError;
use crate::integration::settings::{IntegrationGates, IntegrationSettings};

/// 设置夹具（总开关值与门无关，这里只为构造生产门）。
fn settings(clippy: bool, doc_lint: bool) -> IntegrationSettings {
    IntegrationSettings {
        auto_integrate_enabled: true,
        gates: IntegrationGates { clippy, doc_lint },
    }
}

/// 一步 shell 门（注入面：不读真实 cargo）。
fn shell_step(name: &str, script: &str, timeout: Duration) -> GateStep {
    GateStep::new(
        name,
        vec!["sh".into(), "-c".into(), script.into()],
        timeout,
    )
}

/// 绿门 → Ok + 执行记录（命令、退出码、耗时）。
///
/// 错实现形态：红绿不分（永远 Ok / 永远 Err）→ 本用例红。
#[test]
fn green_gate_returns_report_with_step_record() {
    let dir = tempfile::tempdir().unwrap();
    let spec = GateSpec {
        steps: vec![shell_step("ok", "exit 0", Duration::from_secs(10))],
    };
    let report = spec.run(dir.path(), None).expect("绿门应通过");
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].name, "ok");
    assert_eq!(report.steps[0].exit_code, Some(0));
    assert!(!report.steps[0].timed_out);
}

/// 红门（普通非零退出码）→ Quality（候选自身问题，不是基础设施）。
///
/// 错实现形态：把非零退出码也归 System → 本用例红（质量记录会被基础设施抖动污染）。
#[test]
fn failing_gate_is_quality_attribution_with_output_in_summary() {
    let dir = tempfile::tempdir().unwrap();
    let spec = GateSpec {
        steps: vec![shell_step(
            "cargo_test",
            "echo 'test boom failed'; exit 1",
            Duration::from_secs(10),
        )],
    };
    let err = spec.run(dir.path(), None).unwrap_err();
    assert!(
        matches!(err.root_cause(), DevforgeError::Quality(_)),
        "断言红/编译失败应归质量：{err:?}"
    );
    assert!(
        err.to_string().contains("boom"),
        "失败摘要必须带失败输出（定位不到就等于没留证据）：{err}"
    );
}

/// 被信号杀 → System（基础设施形态：OOM / 外部 kill）。
///
/// 错实现形态：信号杀与普通非零退出码同归 Quality → 本用例红。
#[test]
fn signal_killed_gate_is_system_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let spec = GateSpec {
        steps: vec![shell_step("killed", "kill -9 $$", Duration::from_secs(10))],
    };
    let err = spec.run(dir.path(), None).unwrap_err();
    assert!(
        matches!(err.root_cause(), DevforgeError::System(_)),
        "被信号杀应归系统（基础设施）：{err:?}"
    );
}

/// 可选门按设置进生产门：缺省只有硬门；开启后命令形态固定。
///
/// 错实现形态：可选门无条件执行（缺省也跑 clippy/doc）→ 第一条断言红；
/// 命令形态写错（如 `cargo doc` 少了 `--no-deps`）→ 第三条断言红。
#[test]
fn optional_gates_follow_settings() {
    let off = GateSpec::production(&settings(false, false));
    assert_eq!(
        off.steps.len(),
        1,
        "缺省只应有全量测试硬门：{:?}",
        off.steps
    );
    assert_eq!(off.steps[0].argv, vec!["cargo".to_string(), "test".to_string()]);
    assert!(
        !off.steps
            .iter()
            .any(|s| s.argv.iter().any(|a| a.contains("doc"))),
        "缺省不得执行 cargo doc：{:?}",
        off.steps
    );

    let with_doc = GateSpec::production(&settings(false, true));
    let doc = with_doc
        .steps
        .iter()
        .find(|s| s.name == "doc_lint")
        .expect("开启 doc_lint 后应有该步");
    assert_eq!(doc.argv, vec!["cargo", "doc", "--no-deps"]);

    let with_clippy = GateSpec::production(&settings(true, false));
    assert!(
        with_clippy.steps.iter().any(|s| s.name == "clippy"),
        "开启 clippy 后应有该步：{:?}",
        with_clippy.steps
    );
}

/// 可选门的结果**参与判定**：可选步红 → 整门红（不是「跑完就算过」）。
///
/// 错实现形态：只判第一步（硬门），后续可选门红仍返回 Ok → 本用例红。
#[test]
fn optional_gate_failure_fails_the_whole_gate() {
    let dir = tempfile::tempdir().unwrap();
    let spec = GateSpec {
        steps: vec![
            shell_step("cargo_test", "exit 0", Duration::from_secs(10)),
            shell_step("doc_lint", "exit 1", Duration::from_secs(10)),
        ],
    };
    let err = spec.run(dir.path(), None).unwrap_err();
    assert!(
        err.to_string().contains("doc_lint"),
        "红的是可选门，摘要必须点名：{err}"
    );
}

/// 门在指定工作目录里跑（生产口径：集成 worktree 内跑，不在主工作树）。
///
/// 错实现形态：忽略 cwd（用进程 cwd 跑）→ 本用例红。
#[test]
fn gate_runs_in_the_given_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let spec = GateSpec {
        steps: vec![shell_step("pwd", "pwd", Duration::from_secs(10))],
    };
    let report = spec.run(dir.path(), None).expect("门应通过");
    assert!(
        report.steps[0].output_tail.contains(&dir.path().to_string_lossy().to_string()),
        "门必须在给定工作目录里跑：{}",
        report.steps[0].output_tail
    );
}

/// 外置构建目录：作为 `CARGO_TARGET_DIR` 传给子进程（不外置 = 每轮全量重编译）。
///
/// 错实现形态：不传该环境变量 → 本用例红。
#[test]
fn cargo_target_dir_is_passed_to_the_step() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("cargo-target");
    let spec = GateSpec {
        steps: vec![shell_step(
            "env",
            "echo \"$CARGO_TARGET_DIR\"",
            Duration::from_secs(10),
        )],
    };
    let report = spec.run(dir.path(), Some(&target)).expect("门应通过");
    assert!(
        report.steps[0]
            .output_tail
            .contains(&target.to_string_lossy().to_string()),
        "构建目录必须外置传给子进程：{}",
        report.steps[0].output_tail
    );
    assert!(target.exists(), "构建目录应被创建");
}

/// 构建目录不可用 → System，且**在跑命令之前**就判掉（不靠解析 cargo 输出猜）。
///
/// 错实现形态：把「目录不可用」当成命令失败 → 归 Quality，本用例红。
#[test]
fn unusable_target_dir_is_system_failure() {
    let dir = tempfile::tempdir().unwrap();
    // 父路径是普通文件 → 建目录必然失败（与运行用户无关，root 也一样）
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    let spec = GateSpec {
        steps: vec![shell_step("ok", "exit 0", Duration::from_secs(10))],
    };
    let err = spec
        .run(dir.path(), Some(&blocker.join("cargo-target")))
        .unwrap_err();
    assert!(
        matches!(err.root_cause(), DevforgeError::System(_)),
        "构建目录不可用应归系统：{err:?}"
    );
}

/// 超时 → System + 快速返回 + **进程组被杀干净**（无残留 pid）。
///
/// 断言钉在孙进程（`sleep`）上：只 kill 直接子进程的实现会留下它继续跑 → 本用例红。
///
/// 错实现形态：超时后只 `child.kill()`（不杀进程组）→ 孙进程残留，本用例红。
#[test]
fn timeout_kills_process_group_and_returns_system_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let sh_pid_file = dir.path().join("sh.pid");
    let sleep_pid_file = dir.path().join("sleep.pid");
    // 后台起 sleep（孙进程）+ 记两个 pid，再 wait 住
    let script = format!(
        "sleep 30 & echo $! > {}; echo $$ > {}; wait",
        sleep_pid_file.display(),
        sh_pid_file.display()
    );
    let spec = GateSpec {
        steps: vec![shell_step("slow", &script, Duration::from_secs(1))],
    };
    let started = Instant::now();
    let err = spec.run(dir.path(), None).unwrap_err();
    let elapsed = started.elapsed();
    assert!(
        matches!(err.root_cause(), DevforgeError::System(_)),
        "超时归系统（基础设施）：{err:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "超时必须快速返回（有终止边界）：{elapsed:?}"
    );
    let sh_pid = read_pid(&sh_pid_file);
    let sleep_pid = read_pid(&sleep_pid_file);
    assert!(
        !process_running(sh_pid),
        "超时后直接子进程必须已死（pid {sh_pid} 仍在跑）"
    );
    assert!(
        !process_running(sleep_pid),
        "超时后进程组必须连孙进程一起杀（pid {sleep_pid} 仍在跑 → 留了残骸）"
    );
}

/// 读 pid 文件。
fn read_pid(path: &Path) -> i32 {
    std::fs::read_to_string(path)
        .expect("子进程应留下 pid")
        .trim()
        .parse()
        .expect("pid 应可解析")
}

/// 进程是否**还在跑**（已死或僵尸都算没在跑——僵尸不占资源，只等父进程收尸）。
fn process_running(pid: i32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false; // /proc 里没有 = 已被回收
    };
    // 形态：`pid (comm) state ...`——comm 可能含空格，取最后一个 ')' 之后的字符作状态。
    let Some(close) = stat.rfind(')') else {
        return true; // 读不出状态 = 不敢断言已死
    };
    let state = stat[close + 1..].trim_start().chars().next().unwrap_or('?');
    state != 'Z'
}
