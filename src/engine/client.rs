//! RPC 生命周期客户端（decisions/005）：保活 = agent_settled 后 idle；唤醒 = 同进程再 prompt；终止 = 关 stdin + kill。
//!
//! MVP 同步实现（std::process + BufRead）；P003 服务器层经 spawn_blocking 桥接 tokio。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

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
    /// 读取线程推送的行（stdout 已移交该线程）——读方带子进程存活检测，见 next_line
    lines: Receiver<String>,
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
            lines: {
                // stdout 交给专用线程逐行推送：读方就能在子进程已死但孙进程仍持有
                // 管道（永远等不到 EOF）时靠 child 状态判死，而不是无限阻塞。
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                if tx.send(l).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
                rx
            },
            spec,
            next_id: 0,
            started_at_ms: now_ms(),
        })
    }

    /// 读一行事件流（带存活检测）。
    ///
    /// 为什么不能直接阻塞读：孙进程（MCP server 等）继承了 stdout 时，主进程死掉
    /// 也等不到 EOF——2026-10-19 实测审查官进程中途死亡后 run 挂了 50+ 分钟。
    fn next_line(&mut self) -> Result<String> {
        loop {
            match self.lines.recv_timeout(Duration::from_millis(500)) {
                Ok(l) => return Ok(l),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(DevforgeError::RpcProtocol(
                        "子进程 EOF：agent_settled 之前流已结束".into(),
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        return Err(DevforgeError::System(format!(
                            "子进程已退出（{status}）但未发出 agent_settled——按基础设施故障处理，不作为审查/编码结论"
                        )));
                    }
                }
            }
        }
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
        let started_at_ms = now_ms();
        let mut out = WaitOutcome::default();
        loop {
            let line = self.next_line()?;
            match decode_line(line.trim_end())? {
                crate::engine::protocol::Inbound::Response(r) => {
                    // DR-1 修复：失败响应快速失败（此前傻等 agent_settled，无 key 类错误挂死整个 run 而无任何事件）
                    if !r.success {
                        return Err(self.command_failure(&r));
                    }
                    out.responses.push(r)
                }
                crate::engine::protocol::Inbound::Event(e) => {
                    if let Some(delta) = e.text_delta() {
                        out.text.push_str(delta);
                    }
                    let settled = e.is_agent_settled();
                    out.events.push(e);
                    if settled {
                        self.log_turn_end(started_at_ms, &out);
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
        let started_at_ms = now_ms();
        let mut out = WaitOutcome::default();
        loop {
            let line = self.next_line()?;
            match decode_line(line.trim_end())? {
                crate::engine::protocol::Inbound::Response(r) => {
                    // DR-1 修复：同 wait_settled —— 失败响应快速失败，不傻等 settled
                    if !r.success {
                        return Err(self.command_failure(&r));
                    }
                    out.responses.push(r)
                }
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
                        self.log_turn_end(started_at_ms, &out);
                        return Ok(out);
                    }
                }
            }
        }
    }

    /// DR-1 修复：把 pi 的命令失败（success=false，如缺 API key）转成结构化错误。
    /// 此前这类失败只被归档进 responses 继续傻等 settled，run 无声挂死。
    fn command_failure(&self, r: &ResponseRecord) -> DevforgeError {
        let detail = r.error.clone().unwrap_or_else(|| "（无错误详情）".into());
        tracing::warn!(
            component = "engine",
            event = "agent.command_failed",
            role = %self.spec.role,
            command = %r.command,
            error = %detail,
            "agent 命令失败（快速失败，不等 settled）"
        );
        DevforgeError::System(format!(
            "agent[{}] 命令 {} 失败：{}（多半是 profile 配置问题：缺 API key / 模型名错误；见 auth.json 与 models.json）",
            self.spec.role, r.command, detail
        ))
    }

    /// 「agent 轮次结束」里程碑（Q5：唯一落点，覆盖全部 SOP——change 侧不再另落）。
    ///
    /// 字段：角色名（`spec.role`）、本轮耗时（毫秒）、本轮产出字符数。
    /// 作用：交卷 → 下一次开庭之间的空洞由此可算（此前那 15 分钟无任何记录）。
    fn log_turn_end(&self, started_at_ms: u64, out: &WaitOutcome) {
        tracing::info!(
            component = "engine",
            event = "agent.turn_end",
            role = %self.spec.role,
            turn_ms = now_ms().saturating_sub(started_at_ms),
            chars = out.text.chars().count(),
            "agent 轮次结束"
        );
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
    const RSP_FAIL: &str = r#"{"id":"req-2","type":"response","command":"prompt","success":false,"error":"No API key found for the selected model."}"#;

    #[test]
    fn failed_response_fails_fast_without_waiting_for_settled() {
        // DR-1（2026-10-23 真实仓首跑）：无 API key 时 pi 只回 success=false 的 response、
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
}
