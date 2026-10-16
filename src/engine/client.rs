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
            .stderr(Stdio::null()) // 诊断走 stderr，不解析为协议（rpc.md）
            .spawn()
            .ctx(format!(
                "spawn {} (role={}, run={})",
                spec.program.display(),
                spec.role,
                spec.run_id
            ))?;
        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");
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

    /// 等待 agent_settled，期间遇 extension_ui_request 事件时回调取答案并回传
    /// （ask-user-question 的 UI 子协议；回答记录格式为联调校准点，见 P001 roadmap）。
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
