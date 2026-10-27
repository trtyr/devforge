//! （2026-10-26 测试分离：自 src/config/installer.rs 内联平移，纯移动零改动。）
use super::*;

/// 自审 minor（34）回归：安装边界必须验来源形状——旧口径直接 `pi install <用户串>`。
#[test]
fn package_source_shape_is_validated() {
    // 放行：仓内内置清单实际在用的几种形态
    for ok in [
        "pi-lens",
        "@ff-labs/pi-fff",
        "pi-lens@1.2.3",
        "npm:pi-blackhole",
        "git:github.com/NVlabs/SoL-Pi",
    ] {
        validate_package_source(ok).unwrap_or_else(|e| panic!("应放行 {ok}：{e}"));
    }
    // 拒收：本地路径 / URL / 选项形态 / 元字符
    for bad in [
        "/etc/passwd",
        "../evil",
        "./local-pkg",
        "~/pkg",
        "file:/tmp/pkg",
        "link:../x",
        "https://evil.example/pkg.tgz",
        "--registry=http://evil",
        "pkg; rm -rf /",
        "pkg name",
        "",
    ] {
        assert!(
            validate_package_source(bad).is_err(),
            "必须拒收非法来源：{bad}"
        );
    }
}

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

// ── 2026-10-27 自审 installer 批回归 ──

/// 自审 minor（95）：重装路径不得在 packages 数组里堆重复条目（旧口径只 push，
/// 而相关测试只断言安装计数、不校验数组内容 → 这个分叉无护栏）。
#[test]
fn reinstalling_does_not_duplicate_package_entries() {
    let dir = tempfile::tempdir().unwrap();
    let installer = FakeInstaller;
    installer
        .install(dir.path(), &["npm:a".to_string(), "npm:b".to_string()])
        .unwrap();
    installer
        .install(dir.path(), &["npm:a".to_string()])
        .unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    let arr = settings["packages"].as_array().unwrap();
    let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(names, vec!["npm:a", "npm:b"], "重复安装不得堆条目：{names:?}");
}

/// 自审 minor（93/94）：损坏的 settings.json 必须报错（旧口径静默降级成空对象再回写，
/// 丢掉其余所有设置键）；合法 JSON 但形状错误也不得 panic。
#[test]
fn corrupt_settings_is_an_error_not_a_silent_wipe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{not json").unwrap();
    let err = FakeInstaller
        .install(dir.path(), &["npm:a".to_string()])
        .unwrap_err();
    assert!(err.to_string().contains("损坏"), "{err}");
    // 原文件不得被回写成空对象
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");

    // 形状错误：顶层是数组
    std::fs::write(&path, "[]").unwrap();
    let err = FakeInstaller
        .install(dir.path(), &["npm:a".to_string()])
        .unwrap_err();
    assert!(err.to_string().contains("顶层不是对象"), "{err}");

    // packages 不是数组
    std::fs::write(&path, "{\"packages\": 3}").unwrap();
    let err = FakeInstaller
        .install(dir.path(), &["npm:a".to_string()])
        .unwrap_err();
    assert!(err.to_string().contains("不是数组"), "{err}");
}

/// 自审 minor（24）：退出码文案不得重复（旧口径产出「（exit: exit status: 1）」）。
#[test]
fn failure_text_has_no_duplicated_exit_prefix() {
    let out = std::process::Command::new("sh")
        .args(["-c", "exit 3"])
        .output()
        .unwrap();
    let err = install_failure_error("npm:x", &out);
    let msg = err.to_string();
    assert!(msg.contains("exit code 3"), "{msg}");
    assert!(!msg.contains("exit: exit status"), "退出码文案重复：{msg}");
}
