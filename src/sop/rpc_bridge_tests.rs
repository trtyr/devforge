//! （2026-10-26 测试分离：自 src/sop/rpc_bridge.rs 内联平移，纯移动零改动。）
use super::*;
use crate::error::DevforgeError;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct ScriptUser {
    answers: VecDeque<String>,
    asked: Vec<String>,
}
impl ScriptUser {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|s| s.to_string()).collect(),
            asked: Vec::new(),
        }
    }
}
impl UserLink for ScriptUser {
    fn ask(&mut self, question: &str, _options: &[&str]) -> Result<String> {
        self.asked.push(question.into());
        self.answers
            .pop_front()
            .ok_or_else(|| DevforgeError::System("无更多回答".into()))
    }
}

fn write_script(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join(format!("replay-{}.sh", uuid::Uuid::new_v4()));
    fs::write(&p, body).unwrap();
    p
}

/// 回放（模拟真 pi 时序）：先读走 prompt 行 → 抛 UI 请求 → 读 extension_ui_response（落盘留证）→ 回 ack + settled。
fn ui_replay_script(dir: &Path, response_capture: &Path) -> PathBuf {
    let capture = response_capture.to_string_lossy().replace('\'', "");
    let body = format!(
        "read _cmd\n\
         cat <<'DEVFORGE_EOF'\n\
         {{\"id\":\"ui-1\",\"type\":\"extension_ui_request\",\"method\":\"select\",\"title\":\"规划完成了吗\",\"options\":[\"完成\",\"还没\"]}}\n\
         DEVFORGE_EOF\n\
         read _r\n\
         printf '%s\\n' \"$_r\" > '{capture}'\n\
         cat <<'DEVFORGE_EOF'\n\
         {{\"id\":\"req-1\",\"type\":\"response\",\"command\":\"prompt\",\"success\":true,\"data\":{{\"disposition\":\"started\"}}}}\n\
         {{\"type\":\"agent_settled\"}}\n\
         DEVFORGE_EOF\n"
    );
    write_script(dir, &body)
}

#[test]
fn ui_request_goes_to_user_and_answer_flows_back() {
    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("captured-response.jsonl");
    let script = ui_replay_script(dir.path(), &capture);

    let spec = SpawnSpec {
        program: "sh".into(),
        profile_dir: dir.path().join("profile"),
        role: "plan-agent".into(),
        run_id: RunId::generate().0,
        session_dir: dir.path().to_path_buf(),
        working_dir: std::path::PathBuf::from("."),
        extra_args: vec![script.to_string_lossy().into_owned()],
        ledger_path: None,
    };
    // 裸 sh 跑回放脚本（pi 的 --mode 前缀 sh 不认）
    let mut cmd = Command::new("sh");
    cmd.arg(&script);
    let client = RpcClient::spawn_with_command(cmd, spec).unwrap();
    let mut session = RpcPlanSession::new(client);
    let mut user = ScriptUser::new(&["完成"]);

    let text = session.converse("开始规划", &mut user).unwrap();
    assert_eq!(text, ""); // 回放无 text_delta

    // 用户被问到的问题（宽松提取自 UI 请求）
    assert_eq!(user.asked.len(), 1);
    assert!(user.asked[0].contains("规划完成了吗"), "问句: {}", user.asked[0]);

    // 应答记录：同一 request id + 我们的回答（联调校准点的最小契约）
    let response = fs::read_to_string(&capture).unwrap();
    assert!(response.contains("ui-1"), "应带原 request id: {response}");
    assert!(response.contains("完成"), "应带用户回答: {response}");
}

/// 回放：进程正常结束但不产出任何正文（C19-R2 现场：审查官静默退出）。
fn silent_review_script(dir: &Path) -> PathBuf {
    let body = "read _cmd\n\
                cat <<'DEVFORGE_EOF'\n\
                {\"id\":\"req-1\",\"type\":\"response\",\"command\":\"prompt\",\"success\":true,\"data\":{\"disposition\":\"started\"}}\n\
                {\"type\":\"agent_settled\"}\n\
                DEVFORGE_EOF\n";
    write_script(dir, body)
}

#[test]
fn empty_review_reply_is_infrastructure_error() {
    // C19-R2：空回复不得当作「审查结论」（否则保守解析会误判为协议违例并回注被审 Agent）
    let dir = tempfile::tempdir().unwrap();
    let script = silent_review_script(dir.path());
    let spec = SpawnSpec {
        program: "sh".into(),
        profile_dir: dir.path().join("profile"),
        role: "coding-review".into(),
        run_id: RunId::generate().0,
        session_dir: dir.path().to_path_buf(),
        working_dir: PathBuf::from("."),
        extra_args: vec![script.to_string_lossy().into_owned()],
        ledger_path: None,
    };
    let mut cmd = Command::new("sh");
    cmd.arg(&script);
    let client = RpcClient::spawn_with_command(cmd, spec).unwrap();
    let mut session = RpcReviewSession {
        client,
        role: "coding-review".into(),
    };
    let err = session.review("开始审查").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("无有效回复"), "应报空回复基础设施故障: {msg}");
    assert!(msg.contains("coding-review"), "应指明角色: {msg}");
}

#[test]
fn review_subprocess_marker_carries_the_parent_run_id() {
    // 引擎批次 #1（critical）回归：审查子进程的 `DEVFORGE_MANAGED` 必须是**父 run-id**，
    // 否则 ledger::kill_run_processes（e.run_id != run_id → continue）与调度熔断
    // 都杀不到审查 pi——cancel / watchdog / 熔断三条路径全打空。
    // 旧实现每轮 `RunId::generate()`，本用例必红。
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    crate::observability::begin_run_in_deferred(root.clone(), "run-parent-42", "coding");
    let f = RpcReviewFactory::per_round_in(
        root.join("profile"),
        "plan-review",
        root.join("sessions"),
        root.join("planroot"),
        root.join("spawn-ledger.json"),
    );
    let s1 = (f.make_spec.lock().expect("spec 生成器锁"))();
    let s2 = (f.make_spec.lock().expect("spec 生成器锁"))();
    assert_eq!(
        s1.run_id, "run-parent-42",
        "审查子进程必须带父 run-id，实际：{}",
        s1.run_id
    );
    assert_eq!(
        s2.run_id, "run-parent-42",
        "第二轮也必须带父 run-id，实际：{}",
        s2.run_id
    );
    // 会话目录仍按轮次隔离（run 标记与目录分离）
    assert_ne!(s1.session_dir, s2.session_dir, "会话目录仍按轮次隔离");
    assert!(s1.session_dir.ends_with("review-0"));
    assert!(s2.session_dir.ends_with("review-1"));

    // per_round_in 是同一条路径（编码 / 文档审查走它）
    let g = RpcReviewFactory::per_round_in(
        root.join("profile"),
        "coding-review",
        root.join("sessions2"),
        root.join("bb"),
        root.join("spawn-ledger.json"),
    );
    let s3 = (g.make_spec.lock().expect("spec 生成器锁"))();
    assert_eq!(
        s3.run_id, "run-parent-42",
        "per_round_in 也必须带父 run-id，实际：{}",
        s3.run_id
    );
    assert_eq!(s3.role, "coding-review");
    crate::observability::end_run("run-parent-42");
}
