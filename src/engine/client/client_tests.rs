//! （ARC-R7 自 client.rs mod tests 平移，纯移动。）

use super::*;
use crate::domain::RunId;
use std::fs;
use std::path::{Path, PathBuf};

/// 写一个回放脚本：cat 预录记录，尾部行为可配（read 常驻 / 直接退出）。
fn write_replay(dir: &Path, records: &[&str], tail: &str) -> PathBuf {
    let mut body = String::from("cat <<'DEVFORGE_EOF'\n");
    for r in records {
        body.push_str(r);
        body.push('\n');
    }
    body.push_str("DEVFORGE_EOF\n");
    body.push_str(tail);
    let path = dir.join(format!("replay-{}.sh", uuid::Uuid::new_v4()));
    fs::write(&path, body).unwrap();
    path
}

fn spec_for(script: &Path, session: &Path) -> SpawnSpec {
    SpawnSpec {
        program: "sh".into(),
        profile_dir: session.join("profile"),
        role: "plan-agent".into(),
        run_id: RunId::generate().0,
        session_dir: session.to_path_buf(),
        working_dir: PathBuf::from("."),
        extra_args: vec![script.to_string_lossy().into_owned()],
    }
}

/// mock 进程：裸 `sh <script>`（不经 build_spawn_command——pi 的 --mode 前缀 sh 不认）。
fn spawn_sh(script: &Path, session: &Path) -> RpcClient {
    let spec = spec_for(script, session);
    let mut cmd = Command::new("sh");
    cmd.arg(script);
    RpcClient::spawn_with_command(cmd, spec).unwrap()
}

const RSP_OK: &str =
    r#"{"id":"req-1","type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}"#;
const EVT_START: &str = r#"{"type":"agent_start"}"#;
const EVT_SETTLED: &str = r#"{"type":"agent_settled"}"#;
const RSP_FAIL: &str = r#"{"id":"req-2","type":"response","command":"prompt","success":false,"error":"No API key found for the selected model."}"#;

#[test]
fn failed_response_fails_fast_without_waiting_for_settled() {        // DR-1（2026-10-23 真实仓首跑）：无 API key 时 pi 只回 success=false 的 response、
    // 永不 agent_settled——旧实现继续等 → 整个 run 无声挂死 50 分钟无任何事件。
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[RSP_FAIL],
        "read _x\n", // 常驻：模拟 pi 错误路径不再发任何事件
    );
    let mut client = spawn_sh(&script, dir.path());
    let _ = client.prompt("写单元");
    let t0 = std::time::Instant::now();
    let err = client.wait_settled().unwrap_err();
    assert!(
        t0.elapsed().as_secs() < 4,
        "必须快速失败而非傻等 settled：{:?}",
        t0.elapsed()
    );
    let msg = format!("{err}");
    assert!(msg.contains("No API key"), "错误详情要透传: {msg}");
    assert!(msg.contains("plan-agent"), "错误要带角色名: {msg}");
}

#[test]
fn step_telemetry_logs_tool_calls_and_captures_usage() {
    // DR-9 L2：工具调用起止 → agent.step 事件；usage → last_usage（probe4 实证形态）
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[
            RSP_OK,
            r#"{"type":"tool_execution_start","toolCallId":"t1","toolName":"bash","args":{"command":"ls"}}"#,
            r#"{"type":"tool_execution_end","toolCallId":"t1","toolName":"bash","result":{"content":[]}}"#,
            r#"{"type":"message_update","usage":{"input":12000,"output":300},"assistantMessageEvent":{"type":"text_delta","delta":"x"}}"#,
            EVT_SETTLED,
        ],
        "read _x\n",
    );
    let mut client = spawn_sh(&script, dir.path());
    let _ = client.prompt("干活");
    let usage = std::cell::RefCell::new(None);
    let log = crate::observability::test_support::capture_jsonl(|| {
        let outcome = client.wait_settled().unwrap();
        *usage.borrow_mut() = outcome.last_usage;
    });
    assert!(log.contains("agent.step"), "应有步进事件: {log}");
    assert!(log.contains("\"tool\":\"bash\""), "步进事件要带工具名: {log}");
    assert_eq!(usage.into_inner(), Some((12000, 300)), "usage 要被采集");
}

/// 现场（2026-10-19 e2e test_22）：审查官进程中途死亡，但孙进程仍持有 stdout——
/// 阻塞读永远等不到 EOF，run 挂了 50+ 分钟且日志无信号。必须靠子进程状态判死。
#[test]
fn dead_child_with_open_stdout_pipe_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(dir.path(), &[RSP_OK], "sleep 5 &\nexit 0\n");
    let mut client = spawn_sh(&script, dir.path());
    let _ = client.prompt("审一下");
    let t0 = std::time::Instant::now();
    let err = client.wait_settled().unwrap_err();
    assert!(
        t0.elapsed().as_secs() < 4,
        "判死太慢：{:?}",
        t0.elapsed()
    );
    assert!(format!("{err}").contains("已退出"), "{err}");
}

#[test]
fn replay_settles_and_collects_response() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[RSP_OK, EVT_START, EVT_SETTLED],
        "read _x\n", // 常驻等 stdin，避免 spawn/write 竞态
    );
    let mut client = spawn_sh(&script, dir.path());

    let id = client.prompt("你好").unwrap();
    assert_eq!(id, "req-1");

    let outcome = client.wait_settled().unwrap();
    assert_eq!(outcome.responses.len(), 1);
    assert!(outcome.responses[0].success);
    assert_eq!(outcome.text, "");
    client.kill().unwrap();
}

#[test]
fn the_turn_end_milestone_is_logged_exactly_once_per_turn() {
    // 错实现形态：①交卷不落 agent.turn_end（或缺角色名/耗时）
    // ②同一轮落两条（重复记或 change 侧又落一条）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[RSP_OK, EVT_START, EVT_SETTLED],
        "read _x\n", // 常驻等 stdin，避免 spawn/write 竞态
    );
    let mut client = spawn_sh(&script, dir.path());
    let text = crate::observability::test_support::capture_jsonl(|| {
        client.prompt("你好").unwrap();
        client.wait_settled().unwrap();
    });
    let turns: Vec<&str> = text.lines().filter(|l| l.contains("agent.turn_end")).collect();
    assert_eq!(turns.len(), 1, "一轮只落一条交卷事件：{text}");
    assert!(
        turns[0].contains("\"role\":\"plan-agent\""),
        "交卷要带角色名：{}",
        turns[0]
    );
    assert!(turns[0].contains("\"turn_ms\""), "交卷要带本轮耗时：{}", turns[0]);
    assert!(turns[0].contains("\"chars\""), "交卷要带产出字符数：{}", turns[0]);
    assert!(
        turns[0].contains("\"target\":\"devforge::engine::client\""),
        "里程碑的 target 必须是发出它的模块：{}",
        turns[0]
    );
    // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
    for line in text.lines().filter(|l| l.contains("\"event\":")) {
        crate::observability::test_support::assert_milestone_shape(line);
    }
    client.kill().unwrap();
}

#[test]
fn dialog_methods_classified() {
    // P8-ISSUE-07 修复：分类表——dialog 四方法 vs fire-and-forget 五方法
    let mk = |method: &str| {
        let mut m = serde_json::Map::new();
        m.insert("method".into(), serde_json::Value::String(method.into()));
        m
    };
    for ok in ["select", "confirm", "input", "editor"] {
        assert!(is_dialog_method(&mk(ok)), "{ok} 应属 dialog");
    }
    for no in [
        "notify",
        "setStatus",
        "setWidget",
        "setTitle",
        "set_editor_text",
    ] {
        assert!(!is_dialog_method(&mk(no)), "{no} 应属 fire-and-forget");
    }
    assert!(!is_dialog_method(&serde_json::Map::new()), "缺 method 不算 dialog");
}

#[test]
fn fire_and_forget_not_answered_but_dialog_is() {
    let dir = tempfile::tempdir().unwrap();
    let set_status = r#"{"type":"extension_ui_request","id":"u1","method":"setStatus","statusKey":"bh","statusText":"xx"}"#;
    let select = r#"{"type":"extension_ui_request","id":"u2","method":"select","title":"完成?","options":["完成"]}"#;
    let received = dir.path().join("received.txt");
    let tail = format!(
        r#"while IFS= read -r l; do printf '%s\n' "$l" >> '{}'; done"#,
        received.display()
    );
    let script = write_replay(dir.path(), &[RSP_OK, set_status, select, EVT_SETTLED], &tail);
    let _ = fs::copy(&script, "/tmp/diag-replay-run16.sh"); // 诊断：保留脚本供人工检查
    let mut client = spawn_sh(&script, dir.path());
    client.prompt("go").unwrap();

    let mut asked: Vec<String> = Vec::new();
    let outcome = client
        .wait_settled_with_ui(|extra| {
            asked.push(
                extra
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
            );
            Ok("完成".to_string())
        })
        .unwrap();

    // 先等 shell 把管道缓冲消费进 received.txt（kill 会丢缓冲，竞态实证）
    for _ in 0..30 {
        if received.exists()
            && fs::read_to_string(&received)
                .unwrap_or_default()
                .contains("extension_ui_response")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    client.kill().unwrap();

    // 回调只对 dialog 触发一次
    assert_eq!(asked, vec!["select".to_string()]);
    // 两类请求都进事件流（可观测不丢）
    let ui_events: Vec<_> = outcome
        .events
        .iter()
        .filter(|e| e.r#type == "extension_ui_request")
        .collect();
    assert_eq!(ui_events.len(), 2);
    // pi 只收到 select 的应答线（setStatus 无 response；prompt 行也被记录但非应答）
    let got = fs::read_to_string(&received).unwrap_or_default();
    let answers: Vec<&str> = got
        .lines()
        .filter(|l| l.contains("extension_ui_response"))
        .collect();
    assert_eq!(answers.len(), 1, "只应有一条应答线: {got:?}");
    assert!(answers[0].contains("\"id\":\"u2\""), "应答对应 select 的 id: {got:?}");
    assert!(!got.contains("\"id\":\"u1\""), "setStatus 不得被应答: {got:?}");
}

#[test]
fn text_deltas_accumulate() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[
            RSP_OK,
            r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"he"}}"#,
            r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"llo"}}"#,
            EVT_SETTLED,
        ],
        "read _x\n",
    );
    let mut client = spawn_sh(&script, dir.path());
    client.prompt("hi").unwrap();
    let outcome = client.wait_settled().unwrap();
    assert_eq!(outcome.text, "hello");
    client.kill().unwrap();
}

#[test]
fn eof_before_settled_is_protocol_error() {
    let dir = tempfile::tempdir().unwrap();
    // 无 settled 记录，脚本自然退出 → EOF
    let script = write_replay(dir.path(), &[RSP_OK], "");
    let mut client = spawn_sh(&script, dir.path());
    let err = client.wait_settled().unwrap_err();
    assert!(err.to_string().contains("EOF"), "应为 EOF 协议错误: {err}");
}

#[test]
fn kill_terminates_process() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(dir.path(), &[EVT_START], "sleep 30\n");
    let mut client = spawn_sh(&script, dir.path());
    let entry = client.ledger_entry();
    assert_eq!(entry.role, "plan-agent");
    client.prompt("wake").unwrap();
    client.kill().unwrap();
    assert!(!crate::engine::ledger::process_alive(entry.pid));
}

#[test]
fn stderr_lines_land_in_session_log() {
    // P8-ISSUE-08 修复：pi stderr 不再黑洞——结构化日志 + session_dir 留档
    let dir = tempfile::tempdir().unwrap();
    let script = write_replay(
        dir.path(),
        &[RSP_OK, EVT_START, EVT_SETTLED],
        "echo 'PI-STDERR-MARK-42' >&2\nread _x\n",
    );
    let _ = fs::copy(&script, "/tmp/diag-replay-stderr.sh");
    let mut client = spawn_sh(&script, dir.path());
    client.prompt("hi").unwrap();
    let outcome = client.wait_settled().unwrap();

    // 先等 echo 的 stderr 送达 pi-stderr.log（脚本 cat 后立即 echo；
    // 别让 kill 抢在 echo 前面——SIGKILL 会丢掉尚未执行的脚本尾部）
    let log = dir.path().join("pi-stderr.log");
    for _ in 0..40 {
        if fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("PI-STDERR-MARK-42")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    client.kill().unwrap();
    assert!(!outcome.responses.is_empty());

    for _ in 0..40 {
        if fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("PI-STDERR-MARK-42")
        {
            return; // 通过
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let listing: Vec<(String, u64)> = std::fs::read_dir(log.parent().unwrap())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    (e.file_name().to_string_lossy().into_owned(), len)
                })
                .collect()
        })
        .unwrap_or_default();
    panic!(
        "stderr 应落盘。内容={:?} 目录清单={:?}",
        fs::read_to_string(&log).unwrap_or_default(),
        listing
    );
}

#[test]
fn require_success_maps_failure_to_agent_error() {
    let failed = ResponseRecord {
        id: Some("req-9".into()),
        r#type: "response".into(),
        command: "prompt".into(),
        success: false,
        error: Some("Model not found".into()),
        data: None,
    };
    let outcome = WaitOutcome {
        responses: vec![failed],
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let client = spawn_sh(
        &write_replay(dir.path(), &[EVT_SETTLED], "sleep 30\n"),
        dir.path(),
    );
    let err = client.require_success(&outcome, "req-9").unwrap_err();
    assert!(err.is_agent_failure());
    assert!(err.to_string().contains("Model not found"));
    // 响应缺失场景
    assert!(client.require_success(&outcome, "req-404").is_err());
}
