//! 子进程 spawn 命令构造（decisions/007：spawn 时注入身份标记）。
//!
//! 身份体系：`PI_CODING_AGENT_DIR`（profile 隔离）+ `DEVFORGE_MANAGED`（run-id）
//! + `DEVFORGE_ROLE`。用户的 pi 没有后两个标记，一眼可辨。

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// spawn 规格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnSpec {
    /// pi 可执行程序（测试注入 mock 用，生产恒为 "pi"）。
    pub program: PathBuf,
    /// profile 目录（PI_CODING_AGENT_DIR 指向）。
    pub profile_dir: PathBuf,
    /// 角色（DEVFORGE_ROLE，如 plan-agent / plan-review）。
    pub role: String,
    /// 流水线 run-id（DEVFORGE_MANAGED）。
    pub run_id: String,
    /// 会话存档目录（--session-dir，按 run-id 组织，原则 §4）。
    pub session_dir: PathBuf,
    /// 子进程工作目录（P8-ISSUE-04 修复：规划产出的 plan tree 必须落在
    /// plans/<项目>/<任务>/ 约定路径，而非服务器进程 cwd）。
    pub working_dir: PathBuf,
    /// 附加参数（测试注入 mock 脚本用）。
    pub extra_args: Vec<String>,
}

impl SpawnSpec {
    /// 生产规格：程序 = pi。
    pub fn new(profile_dir: PathBuf, role: impl Into<String>, run_id: impl Into<String>, session_dir: PathBuf) -> Self {
        Self {
            program: PathBuf::from("pi"),
            profile_dir,
            role: role.into(),
            run_id: run_id.into(),
            session_dir,
            working_dir: PathBuf::from("."),
            extra_args: Vec::new(),
        }
    }

    /// 设置工作目录（链式；P8-ISSUE-04）。
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = dir;
        self
    }
}

/// 构造 spawn 命令：`pi --mode rpc --session-dir <dir>` + 三层环境标记。
pub fn build_spawn_command(spec: &SpawnSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.arg("--mode")
        .arg("rpc")
        .arg("--session-dir")
        .arg(&spec.session_dir);
    for arg in &spec.extra_args {
        cmd.arg(arg);
    }
    cmd.env("PI_CODING_AGENT_DIR", &spec.profile_dir)
        .env("DEVFORGE_MANAGED", &spec.run_id)
        .env("DEVFORGE_ROLE", &spec.role);
    // P8-ISSUE-04 修复：子进程工作目录 = plan root（decisions/002）
    cmd.current_dir(&spec.working_dir);
    cmd
}

#[cfg(test)]
mod tests {
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
}
