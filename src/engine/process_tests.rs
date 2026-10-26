//! （2026-10-26 测试分离：自 src/engine/process.rs 内联平移，纯移动零改动。）
use super::*;

fn spec() -> SpawnSpec {
    SpawnSpec::new(
        PathBuf::from("/profiles/plan-agent"),
        "plan-agent",
        "run-abc",
        PathBuf::from("/sessions/run-abc"),
    )
}

#[test]
fn command_has_rpc_mode_and_session_dir() {
    let cmd = build_spawn_command(&spec());
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        vec!["--mode", "rpc", "--session-dir", "/sessions/run-abc"]
    );
    // 默认中性工作目录（不污染服务器 cwd 语义）
    assert_eq!(cmd.get_current_dir(), Some(std::path::Path::new(".")));
}

#[test]
fn command_sets_working_dir_for_plan_root() {
    // P8-ISSUE-04 修复：子进程 cwd = plans/<项目>/<任务>/（decisions/002）
    let spec = spec().with_working_dir(PathBuf::from("/plans/demo-app/task-1"));
    let cmd = build_spawn_command(&spec);
    assert_eq!(
        cmd.get_current_dir(),
        Some(std::path::Path::new("/plans/demo-app/task-1"))
    );
}

#[test]
fn command_injects_all_three_markers() {
    let cmd = build_spawn_command(&spec());
    let envs: Vec<(String, String)> = cmd
        .get_envs()
        .filter_map(|(k, v)| v.map(|v| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())))
        .collect();
    assert!(envs.contains(&("PI_CODING_AGENT_DIR".into(), "/profiles/plan-agent".into())));
    assert!(envs.contains(&("DEVFORGE_MANAGED".into(), "run-abc".into())));
    assert!(envs.contains(&("DEVFORGE_ROLE".into(), "plan-agent".into())));
}

#[test]
fn extra_args_appended_for_mock() {
    let mut s = spec();
    s.program = PathBuf::from("sh");
    s.extra_args = vec!["/tmp/replay.sh".into()];
    let cmd = build_spawn_command(&s);
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(args.contains(&"/tmp/replay.sh".to_string()));
    assert_eq!(cmd.get_program(), "sh");
}
