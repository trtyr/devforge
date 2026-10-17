//! RPC 生命周期客户端（decisions/005）：保活 = agent_settled 后 idle；唤醒 = 同进程再 prompt；终止 = 关 stdin + kill。
//!
//! MVP 同步实现（std::process + BufRead）；P003 服务器层经 spawn_blocking 桥接 tokio。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::domain::now_ms;
use crate::engine::ledger::LedgerEntry;
use crate::engine::process::{build_spawn_command, SpawnSpec};
use crate::engine::protocol::{decode_line, CommandRecord, EventRecord, ResponseRecord};
use crate::error::{Context as _, DevforgeError, Result};

/// 一次 wait_settled 的收集结果。
#[derive(Debug, Default, Clone)]
pub struct WaitOutcome {
    /// 期间收到的全部响应（含 prompt 的 ack）。
    pub responses: Vec<ResponseRecord>,
    /// 期间累积的流式文本（text_delta 拼接）。
    pub text: String,
    /// 期间收到的事件（可观测数据层原料）。
    pub events: Vec<EventRecord>,
}

/// extension_ui_request 是否为 dialog 类（阻塞等应答）。
/// dialog：select/confirm/input/editor；fire-and-forget：notify/setStatus/setWidget/setTitle/set_editor_text。
/// （rpc-extension-ui.md 契约；P8-ISSUE-07 修复的分流依据）
pub(crate) fn is_dialog_method(extra: &serde_json::Map<String, serde_json::Value>) -> bool {
    matches!(
        extra.get("method").and_then(|v| v.as_str()),
        Some("select") | Some("confirm") | Some("input") | Some("editor")
    )
}

/// 长驻 RPC 子进程客户端。
pub struct RpcClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    spec: SpawnSpec,
    next_id: u64,
    started_at_ms: u64,
}

impl RpcClient {
    /// spawn 子 pi 进程（RPC 模式）。台账条目由调用方立即落盘（预防孤儿）。
    pub fn spawn(spec: SpawnSpec) -> Result<Self> {
        Self::spawn_with_command(build_spawn_command(&spec), spec)
    }

    /// 用显式命令 spawn（测试注入 mock 回放进程用；生产走 [`spawn`]）。
    pub fn spawn_with_command(mut cmd: Command, spec: SpawnSpec) -> Result<Self> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // P8-ISSUE-08 修复：stderr 落日志（此前 Stdio::null 黑盒）
            .spawn()
            .ctx(format!(
                "spawn {} (role={}, run={})",
                spec.program.display(),
                spec.role,
                spec.run_id
            ))?;
        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");
        let stderr = child.stderr.take().expect("stderr 已 piped");
        // stderr 伴读线程：结构化日志 + 会话目录留档（pi 侧故障从此可观测可诊断）
        {
            let role = spec.role.clone();
            let run_id = spec.run_id.clone();
            let stderr_log = spec.session_dir.join("pi-stderr.log");
            std::thread::spawn(move || {
                use std::io::BufRead;
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(l) if !l.trim().is_empty() => {
                            tracing::warn!(
                                component = "pi-stderr",
                                role = %role,
                                run_id = %run_id,
                                "{l}"
                            );
                            if let Ok(mut f) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&stderr_log)
                            {
                                use std::io::Write as _;
                                let _ = writeln!(f, "{l}");
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
        }
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            spec,
            next_id: 0,
            started_at_ms: now_ms(),
        })
    }

    /// 台账条目（spawn 后立即落盘）。
    pub fn ledger_entry(&self) -> LedgerEntry {
        LedgerEntry {
            pid: self.child.id(),
            role: self.spec.role.clone(),
            run_id: self.spec.run_id.clone(),
            started_at_ms: self.started_at_ms,
        }
    }

    fn next_cmd_id(&mut self) -> String {
        self.next_id += 1;
        format!("req-{}", self.next_id)
    }

    /// 唤醒/驱动：发 prompt（idle 后同进程再发 = 同会话续写，上下文在内存，非 session 重放）。
    /// 返回命令 id。
    pub fn prompt(&mut self, message: &str) -> Result<String> {
        let id = self.next_cmd_id();
        let line = CommandRecord::prompt(id.clone(), message).encode()?;
        let stdin = self
            .stdin
            .as_mut()
            .expect("stdin 仅在 kill 后不可用");
        stdin
            .write_all(line.as_bytes())
            .ctx(format!("发送 prompt (id={id})"))?;
        stdin.flush().ctx("flush prompt")?;
        Ok(id)
    }

    /// 等待 agent_settled（pi 不会再自动继续 = 暂停保活点）。期间收集响应/流式文本/事件。
    pub fn wait_settled(&mut self) -> Result<WaitOutcome> {
        let mut out = WaitOutcome::default();
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).ctx("读取 RPC 事件流")?;
            if n == 0 {
                return Err(DevforgeError::RpcProtocol(
                    "子进程 EOF：agent_settled 之前流已结束".into(),
                ));
            }
            match decode_line(line.trim_end())? {
                crate::engine::protocol::Inbound::Response(r) => out.responses.push(r),
                crate::engine::protocol::Inbound::Event(e) => {
                    if let Some(delta) = e.text_delta() {
                        out.text.push_str(delta);
                    }
                    let settled = e.is_agent_settled();
                    out.events.push(e);
                    if settled {
                        return Ok(out);
                    }
                }
            }
        }
    }

    /// 等待 agent_settled，期间遇 extension_ui_request 事件时：
    /// dialog 类（select/confirm/input/editor）回调取答案并回传；
    /// fire-and-forget 类（notify/setStatus/setWidget/setTitle/set_editor_text）只记录不应答
    /// （P8-ISSUE-07 修复：rpc-extension-ui.md 契约——后者不期待 response，误应答属协议噪音）。
    pub fn wait_settled_with_ui(
        &mut self,
        mut on_ui_request: impl FnMut(
            &serde_json::Map<String, serde_json::Value>,
        ) -> Result<String>,
    ) -> Result<WaitOutcome> {
        let mut out = WaitOutcome::default();
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).ctx("读取 RPC 事件流")?;
            if n == 0 {
                return Err(DevforgeError::RpcProtocol(
                    "子进程 EOF：agent_settled 之前流已结束".into(),
                ));
            }
            match decode_line(line.trim_end())? {
                crate::engine::protocol::Inbound::Response(r) => out.responses.push(r),
                crate::engine::protocol::Inbound::Event(e) => {
                    if let Some(delta) = e.text_delta() {
                        out.text.push_str(delta);
                    }
                    if e.r#type == "extension_ui_request" {
                        // P8-ISSUE-07 修复：按 method 分流——dialog 才走应答链路
                        if !is_dialog_method(&e.extra) {
                            out.events.push(e);
                            continue;
                        }
                        // 问用户（经 UI 链路），回传应答（同一 request id）
                        let answer = on_ui_request(&e.extra)?;
                        let req_id = e
                            .extra
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let resp = serde_json::json!({
                            "id": req_id,
                            "type": "extension_ui_response",
                            "value": answer,
                        });
                        let mut l = serde_json::to_string(&resp).ctx("序列化 UI 应答")?;
                        l.push('\n');
                        self.stdin
                            .as_mut()
                            .expect("stdin 仅在 kill 后不可用")
                            .write_all(l.as_bytes())
                            .ctx("发送 extension_ui_response")?;
                            self.stdin.as_mut().expect("stdin").flush().ctx("flush UI 应答")?;
                        out.events.push(e);
                        continue;
                    }
                    let settled = e.is_agent_settled();
                    out.events.push(e);
                    if settled {
                        return Ok(out);
                    }
                }
            }
        }
    }

    /// 校验某 id 的响应成功；失败则转为该角色 Agent 的失败错误（可恢复类）。
    pub fn require_success(&self, outcome: &WaitOutcome, id: &str) -> Result<()> {
        match outcome.responses.iter().find(|r| r.id.as_deref() == Some(id)) {
            Some(r) if r.success => Ok(()),
            Some(r) => Err(DevforgeError::agent(
                &self.spec.role,
                r.error.clone().unwrap_or_else(|| format!("命令 {id} 失败")),
            )),
            None => Err(DevforgeError::RpcProtocol(format!("响应缺失: {id}"))),
        }
    }

    /// 终止：关 stdin（体面请求，rpc.md 语义）→ kill → reap。
    pub fn kill(&mut self) -> Result<()> {
        // 体面：关 stdin 请求 orderly shutdown
        drop(self.stdin.take());
        // 兜底：强杀并收尸，不留 zombie
        self.child.kill().ctx("kill RPC 子进程")?;
        self.child.wait().ctx("reap RPC 子进程")?;
        Ok(())
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        // 尽力收尸，避免 zombie
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
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
        assert!(outcome.responses.len() >= 1);

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
}
