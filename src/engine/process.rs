//! 子进程 spawn 命令构造（decisions/007：spawn 时注入身份标记）。
//!
//! 身份体系：`PI_CODING_AGENT_DIR`（profile 隔离）+ `DEVFORGE_MANAGED`（run-id）
//! + `DEVFORGE_ROLE`。用户的 pi 没有后两个标记，一眼可辨。

use std::path::PathBuf;
use std::process::Command;

/// spawn 规格。
///
/// 自审 minor（35）：**不再派生 Serialize/Deserialize**——全仓无序列化调用点，
/// 而一旦被落盘就会把可执行名与 profile/session **绝对路径**写进持久化文件。
#[derive(Debug, Clone)]
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
    /// 台账路径（自审 major）：None = 退回旧行为（working_dir.parent() 涌现推导）。
    /// 生产必须显式给：change SOP 的 working_dir 是 plans/<p>/<t>/change，涌现推导
    /// 会把台账落 task 层，而熔断与启动对账只读 project 层——六个角色全部隐身。
    pub ledger_path: Option<PathBuf>,
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
            ledger_path: None,
        }
    }

    /// 设置工作目录（链式；P8-ISSUE-04）。
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = dir;
        self
    }

    /// 显式指定 spawn 台账路径（自审 major：熔断/对账的事实源必须落在
    /// plans/<项目>/ 层，不能靠 working_dir 涌现推导）。
    pub fn with_ledger_path(mut self, path: PathBuf) -> Self {
        self.ledger_path = Some(path);
        self
    }
}

/// 构造 spawn 命令：`pi --mode rpc --session-dir <dir>` + 三层环境标记。
///
/// 自审 minor（137）：stdio 在这里就配好（piped）——旧口径返回一个未配 stdio 的
/// 半成品 Command，最危险的那段留给调用方补；漏配的失败模式是子进程**直写服务器
/// stdout**，把 SSE/CLI 输出流污染成协议垃圾。调用方要用 inherit 请显式覆写。
pub fn build_spawn_command(spec: &SpawnSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
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

/// spawn 失败的错误上下文（自审 minor 138）：ENOENT 无法区分「可执行程序不存在」
/// 与「工作目录不存在」——把两路径都带上，排障不用猜。
pub fn spawn_failure_context(spec: &SpawnSpec) -> String {
    format!(
        "spawn {}（role={} working_dir={} session_dir={}）",
        spec.program.display(),
        spec.role,
        spec.working_dir.display(),
        spec.session_dir.display()
    )
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
