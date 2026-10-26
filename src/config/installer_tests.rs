//! （2026-10-26 测试分离：自 src/config/installer.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn install_command_carries_profile_env() {
    let cmd = build_install_command(Path::new("/profiles/plan-agent"), "npm:pi-blackhole");
    assert_eq!(cmd.get_program(), "pi");
    let env = cmd.get_envs().find(|(k, _)| *k == "PI_CODING_AGENT_DIR").unwrap();
    assert_eq!(env.1, Some(Path::new("/profiles/plan-agent").as_os_str()));
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, vec!["install", "npm:pi-blackhole"]);
}

/// 自审 major 回归：超时强杀 + 快命令不受影响（合并为单测：env 是进程全局，
/// 拆两个会自竞态）。
#[test]
fn install_timeout_kills_but_fast_completes() {
    // SAFETY: 测试进程内独占该变量（无并发读者），edition 2024 下 set_var 为 unsafe
    // 超时路径：挂死进程被强杀，不再永久占住渲染线程
    let mut slow = Command::new("sleep");
    slow.arg("30");
    unsafe { std::env::set_var("DEVFORGE_INSTALL_TIMEOUT_MS", "300") };
    let err = run_with_timeout(slow, "sleep").unwrap_err();
    assert!(err.to_string().contains("超时"), "{err}");
    assert!(err.to_string().contains("强杀"), "{err}");
    // 快路径：输出完整回收
    let mut fast = Command::new("echo");
    fast.arg("hello");
    unsafe { std::env::set_var("DEVFORGE_INSTALL_TIMEOUT_MS", "10000") };
    let out = run_with_timeout(fast, "echo").unwrap();
    unsafe { std::env::remove_var("DEVFORGE_INSTALL_TIMEOUT_MS") };
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
}

#[test]
fn fake_installer_writes_dual_records() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("plan-agent");
    std::fs::create_dir_all(&profile).unwrap();
    FakeInstaller
        .install(&profile, &["npm:pi-blackhole".into(), "npm:pi-fff".into()])
        .unwrap();

    // 记录 1：settings.json packages
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(profile.join("settings.json")).unwrap())
            .unwrap();
    let pkgs = settings["packages"].as_array().unwrap();
    assert_eq!(pkgs.len(), 2);

    // 记录 2：npm/package.json dependencies
    let pkg_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(profile.join("npm/package.json")).unwrap())
            .unwrap();
    let deps = pkg_json["dependencies"].as_object().unwrap();
    assert!(deps.contains_key("npm:pi-blackhole"));
    assert!(deps.contains_key("npm:pi-fff"));
}

// ---------- F2 硬化：安装失败错误附带 stderr 尾部 ----------

fn output_with_stderr(stderr: &[u8], raw_exit: i32) -> std::process::Output {
    #[cfg(unix)]
    let status = std::os::unix::process::ExitStatusExt::from_raw(raw_exit);
    #[cfg(windows)]
    let status = std::os::windows::process::ExitStatusExt::from_raw(raw_exit as u32);
    std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

#[test]
fn failure_error_carries_stderr_tail() {
    // 失败原因（如 ENOSPC/谜语报错）直接进错误信息，不再抓瞎
    let out = output_with_stderr(b"npm error Failed to move @ast-grep/cli binaries into place.", 256);
    let err = install_failure_error("npm:pi-lens", &out).to_string();
    assert!(err.contains("pi install npm:pi-lens 失败"), "{err}");
    assert!(err.contains("Failed to move"), "应含 stderr 内容: {err}");
}

#[test]
fn failure_error_truncates_to_tail() {
    // 超长 stderr 只保尾部（错误通常在末尾），且不越 UTF-8 边界
    let long = "x".repeat(2000) + "\n真原因在最后一行";
    let out = output_with_stderr(long.as_bytes(), 256);
    let err = install_failure_error("pkg", &out).to_string();
    assert!(err.contains("真原因在最后一行"), "应保尾部: {err}");
    assert!(err.len() < 800, "应截断: len={}", err.len());
}

#[test]
fn failure_error_handles_empty_stderr() {
    let out = output_with_stderr(b"", 256);
    let err = install_failure_error("pkg", &out).to_string();
    assert!(err.contains("stderr 为空"), "{err}");
}
