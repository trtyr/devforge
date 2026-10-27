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
    /// DR-9 L2：**最近一次响应**上报的 usage（input, output）tokens。
    /// 口径澄清（2026-10-27 自审 minor 129）：这是最后一次上报的值（上下文规模快照），
    /// 不是整轮累加；事件字段名用 last_ 前缀，避免被当成「本轮消耗」。
    pub last_usage: Option<(u64, u64)>,
    /// 2026-10-27 自审 major：path-guard 拦截计数（消费方持有事件总线，可上总线）。
    pub boundary_denials: usize,
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
    /// 台账落点与登记时的 pid（子进程收场时据此摘条目；自审 minor 132）。
    ledger_path: Option<std::path::PathBuf>,
    ledger_pid: Option<u32>,
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
            // 自审 minor（138）：上下文带 working_dir / session_dir——ENOENT 才能区分
            // 「可执行程序不存在」与「工作目录不存在」。
            .ctx(crate::engine::process::spawn_failure_context(&spec))?;
    // 运行时台账登记（复验熔断归因修复）：budget fuse 强杀与孤儿对账的事实源。
    // 登记成功后路径与 pid 留在 client 上，收场时据此摘条目（自审 minor 132）。
    // 自审 major：优先用显式指定的台账路径；没有时才退回 working_dir.parent() 涌现
    // 推导（change SOP 的 working_dir 是 plans/<p>/<t>/change，推导会落错层）。
    let registered_ledger_path: Option<std::path::PathBuf> = {
        let ledger_path = spec.ledger_path.clone().unwrap_or_else(|| {
            spec.working_dir
                .parent()
                .map(|p| p.join("spawn-ledger.json"))
                .unwrap_or_else(|| spec.working_dir.join("spawn-ledger.json"))
        });
        // 引擎批次：读-改-写必须跨进程互斥（max_concurrent_mutation 默认 2）
        crate::engine::ledger::with_file_lock(&ledger_path, || {
        let mut ledger = crate::engine::ledger::SpawnLedger::load_for_record(&ledger_path);
        ledger.record(crate::engine::ledger::LedgerEntry {
            pid: child.id(),
            role: spec.role.clone(),
            run_id: spec.run_id.clone(),
            started_at_ms: crate::domain::now_ms(),
        });
        // 引擎审计修复 #4：台账是熔断 SIGKILL 与孤儿对账的事实源（先落盘再打勾铁律）——
        // 写失败不得静默：ERROR 带路径留痕（spawn 继续不回滚：台账缺失只影响未来收割能力，
        // 不影响本 run 正确性；上抛反而把可观测降级放大成 run 失败）。
        match ledger.save(&ledger_path) {
            // 自审 minor（131）：登记这一核心状态迁移此前零事件——日志流无法重建
            // 「哪个 pid 在什么时候、以什么角色进了台账」。
            Ok(()) => {
                tracing::info!(
                    component = "engine-ledger",
                    event = "ledger.registered",
                    path = %ledger_path.display(),
                    pid = child.id(),
                    role = %spec.role,
                    run_id = %spec.run_id,
                    "子进程已登记进 spawn 台账"
                );
                Some(ledger_path.clone())
            }
            Err(e) => {
                tracing::error!(
                    component = "engine-ledger",
                    event = "ledger.register_failed",
                    path = %ledger_path.display(),
                    pid = child.id(),
                    run_id = %spec.run_id,
                    error = %e,
                    "spawn 台账落盘失败——熔断事实源缺口，本 run 挂死时无法精确收割"
                );
                None
            }
        }
        })
    };
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
                            // 2026-10-27 自审 minor（121）：长度上限——stderr 可能整段
                            // 打印超长行（stack/JSON dump），原样进日志会把日志炸大。
                            const MAX_LINE: usize = 2000;
                            let l = if l.chars().count() > MAX_LINE {
                                let cut: String = l.chars().take(MAX_LINE).collect();
                                format!("{cut}…（截断，原长 {} 字符）", l.chars().count())
                            } else {
                                l
                            };
                            tracing::warn!(
                                component = "pi-stderr",
                                role = %role,
                                run_id = %run_id,
                                "{l}"
                            );
                            match std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&stderr_log)
                            {
                                Ok(mut f) => {
                                    use std::io::Write as _;
                                    // 自审 minor（122）：留档失败必须留痕——旧口径
                                    // `let _ =` 使「stderr 落盘」可静默降级。
                                    if let Err(e) = writeln!(f, "{l}") {
                                        tracing::warn!(
                                            component = "pi-stderr",
                                            role = %role,
                                            path = %stderr_log.display(),
                                            error = %e,
                                            "stderr 留档写入失败（此后仅 tracing 有一份）"
                                        );
                                    }
                                }
                                Err(e) => tracing::warn!(
                                    component = "pi-stderr",
                                    role = %role,
                                    path = %stderr_log.display(),
                                    error = %e,
                                    "stderr 留档文件打不开（仅 tracing 一份）"
                                ),
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(
                                component = "pi-stderr",
                                role = %role,
                                error = %e,
                                "读 stderr 失败，伴读线程退出"
                            );
                            break;
                        }
                    }
                }
            });
        }
        let ledger_pid = registered_ledger_path.as_ref().map(|_| child.id());
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
                            // 2026-10-27 自审 minor（123）：读错误不得等同 EOF——
                            // 旧口径一律 break，下游把管道读失败统一报成「子进程 EOF」，
                            // 真实起因丢掉。先留痕再断。
                            Err(e) => {
                                tracing::warn!(
                                    component = "engine",
                                    error = %e,
                                    "读子进程 stdout 失败（下游按 EOF 处理，起因已留痕）"
                                );
                                break;
                            }
                        }
                    }
                });
                rx
            },
            spec,
            next_id: 0,
            started_at_ms: now_ms(),
            ledger_path: registered_ledger_path,
            ledger_pid,
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
                    // 引擎审计修复 #3：子进程流提前结束与 try_wait 发现退出是同一类故障
                    // （「子进程没了」的两种表象）——统一归 System（基础设施，可重启），
                    // 不再一个 RpcProtocol 一个 System 各奔东西。
                    return Err(DevforgeError::System(format!(
                        "子进程 EOF（role={} pid={}）：agent_settled 之前流已结束（基础设施故障）",
                        self.spec.role,
                        self.child.id()
                    )));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        // 2026-10-27 自审 major：判死前先给伴读线程一个排空窗口——
                        // 子进程退出 ≠ 管道读尽，已缓冲的 agent_settled 可能晚于
                        // 500ms 才被伴读线程交付；旧口径晚一点就误判「已退出」，
                        // 整轮工作作废且触发全 run 盲重启。排空到断流或窗口耗尽。
                        let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
                        loop {
                            match self.lines.recv_timeout(Duration::from_millis(200)) {
                                Ok(l) => return Ok(l),
                                Err(RecvTimeoutError::Timeout) => {
                                    if std::time::Instant::now() >= drain_deadline {
                                        break;
                                    }
                                }
                                Err(RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        return Err(DevforgeError::System(format!(
                            "子进程已退出（{status}，role={} pid={}）且管道排空后仍未见 agent_settled——按基础设施故障处理，不作为审查/编码结论",
                            self.spec.role,
                            self.child.id()
                        )));
                    }
                }
            }
        }
    }

    /// 台账条目（用于对外暴露本进程登记事实）。
    ///
    /// 注意（2026-10-27 自审 minor 30）：真正落盘的台账条目由 spawn 时的
    /// `now_ms()` 生成，与此处的 `started_at_ms`（客户端构造时刻）**不是同一个值**；
    /// 本方法只作观测/测试用，不作为台账真源。
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
            // 引擎审计修复 #7：可失败路径不许 expect（kill 已 take stdin，此处 None 会 panic）
            .ok_or_else(|| {
                DevforgeError::RpcProtocol("stdin 已关闭：子进程被杀后无法发送 prompt".into())
            })?;
        stdin
            .write_all(line.as_bytes())
            .ctx(format!("发送 prompt (id={id})"))?;
        stdin.flush().ctx("flush prompt")?;
        // DR-2/DR-3：轮次开始即记 agent 事件——停滞告警与活性聚合的锚点
        crate::observability::note_agent_event_for(&self.spec.run_id);
        tracing::info!(
            component = "engine",
            event = "agent.turn_start",
            role = %self.spec.role,
            // 2026-10-27 自审 minor（125）：与 turn_end 同口径（字符数，非字节）
            chars = message.chars().count(),
            "agent 轮次开始（prompt 已发）"
        );
        Ok(id)
    }

    /// 等待 agent_settled（pi 不会再自动继续 = 暂停保活点）。期间收集响应/流式文本/事件。
    pub fn wait_settled(&mut self) -> Result<WaitOutcome> {
        self.wait_settled_deadline(None)
    }

    /// 带墙钟的等待（第三方 r3 M4）：到点即返回 Quality——
    /// 「流式输出刷 liveness、但再也不收尾」的活死批靠 idle 收割是打不中的，
    /// 只能由批次自己的墙钟叫停。`None` = 旧行为（永不死等）。
    pub fn wait_settled_deadline(
        &mut self,
        deadline: Option<std::time::Instant>,
    ) -> Result<WaitOutcome> {
        let started_at_ms = now_ms();
        let mut out = WaitOutcome::default();
        let mut tool_starts = std::collections::HashMap::new();
        loop {
            // 单层 match 而非嵌两层 if：clippy 会嫌 `if let` 包 `if` 可合，
            // 而 let-chain 在 edition 2021 不可用。
            if let Some(at) = deadline {
                let expired = std::time::Instant::now() >= at;
                if expired {
                    return Err(self.fail_turn(
                        started_at_ms,
                        &out,
                        DevforgeError::Quality(
                            "审查会话超过批次墙钟仍未收尾（流式刷活性但不再推进）——按质量失败放弃该批"
                                .into(),
                        ),
                    ));
                }
            }
            let line = match self.next_line() {
                Ok(l) => l,
                Err(e) => return Err(self.fail_turn(started_at_ms, &out, e)),
            };
            let inbound = match decode_line(line.trim_end()) {
                Ok(i) => i,
                Err(e) => return Err(self.fail_turn(started_at_ms, &out, e)),
            };
            match inbound {
                crate::engine::protocol::Inbound::Response(r) => {
                    // DR-1 修复：失败响应快速失败（此前傻等 agent_settled，无 key 类错误挂死整个 run 而无任何事件）
                    if !r.success {
                        let e = self.command_failure(&r);
                        return Err(self.fail_turn(started_at_ms, &out, e));
                    }
                    out.responses.push(r)
                }
                crate::engine::protocol::Inbound::Event(e) => {
                    // 自审 minor（140）：权威终结文本到达即覆盖 delta 拼接（text_end.content /
                    // message_end.message 才是权威全文；delta 可能漏事件或分块错位）。
                    if let Some(full) = e.authoritative_text() {
                        out.text.clear();
                        out.text.push_str(full);
                        crate::observability::note_agent_event_for(&self.spec.run_id);
                    } else if let Some(delta) = e.text_delta() {
                        out.text.push_str(delta);
                        // 2026-10-27 自审 minor（31）：流式文本无上限累积（长轮次线性吃内存）。
                        // 协议块按全仓口径在**尾部**（D026），故超限保尾丢头，并留痕。
                        const MAX_TEXT_CHARS: usize = 4_000_000;
                        if out.text.chars().count() > MAX_TEXT_CHARS {
                            let keep_from = out.text.len().saturating_sub(MAX_TEXT_CHARS / 2);
                            let mut cut = keep_from;
                            while cut < out.text.len() && !out.text.is_char_boundary(cut) {
                                cut += 1;
                            }
                            out.text.drain(..cut);
                            tracing::warn!(
                                component = "engine",
                                role = %self.spec.role,
                                run_id = %self.spec.run_id,
                                "轮次文本超过上限，已丢弃头部保留尾部（协议块在尾部）"
                            );
                        }
                        // DR-3 精化：流式 delta 也是活性——长流式回合不得误触停滞告警
                        crate::observability::note_agent_event_for(&self.spec.run_id);
                    }
                    self.note_step_telemetry(&e, &mut tool_starts, &mut out);
                    let settled = e.is_agent_settled();
                    out.events.push(e);
                    // 事件只是遥测原料：超窗口丢最旧（避免长轮次无界增长）
                    const MAX_EVENTS: usize = 50_000;
                    if out.events.len() > MAX_EVENTS {
                        let drop_n = out.events.len() - MAX_EVENTS;
                        out.events.drain(..drop_n);
                    }
                    if settled {
                        self.log_turn_end(started_at_ms, &out);
                        return Ok(out);
                    }
                }
            }
        }
    }

    /// 2026-10-27 自审 minor（126/127）：等待循环的错误出口统一处理——
    /// ① 轮次异常终止也落 agent.turn_end（旧口径只有正常 settled 才落，turn_start 无配对，
    ///    「交卷→开庭空洞」与轮次耗时统计失真）；② 已产出的文本/事件数落 ERROR 日志
    ///    （旧口径三条错误出口把 out.text/out.events 整只丢掉，排障最值钱的上下文没了）。
    fn fail_turn(&self, started_at_ms: u64, out: &WaitOutcome, e: DevforgeError) -> DevforgeError {
        tracing::error!(
            component = "engine",
            event = "agent.turn_aborted",
            role = %self.spec.role,
            run_id = %self.spec.run_id,
            turn_ms = now_ms().saturating_sub(started_at_ms),
            chars = out.text.chars().count(),
            events = out.events.len(),
            responses = out.responses.len(),
            error = %e,
            "轮次异常终止（已收集的上下文见上）"
        );
        self.log_turn_end_with(started_at_ms, out, "aborted");
        e
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
        let mut tool_starts = std::collections::HashMap::new();
        loop {
            // 2026-10-27 自审 major：Plan 侧等待也要墙钟——「流式刷活性但不再收尾」
            // 的活死批 idle 收割打不中，只能靠墙钟叫停（与 wait_settled_deadline 同口径）。
            // review_deadline() 为 None = 显式不限时（零硬限制），行为同旧。
            if let Some(at) = crate::sop::review_gate::review_deadline() {
                if std::time::Instant::now() >= at {
                    return Err(DevforgeError::Quality(
                        "会话超过墙钟仍未收尾（流式刷活性但不再推进）——按质量失败放弃"
                            .into(),
                    ));
                }
            }
            let line = match self.next_line() {
                Ok(l) => l,
                Err(e) => return Err(self.fail_turn(started_at_ms, &out, e)),
            };
            let inbound = match decode_line(line.trim_end()) {
                Ok(i) => i,
                Err(e) => return Err(self.fail_turn(started_at_ms, &out, e)),
            };
            match inbound {
                crate::engine::protocol::Inbound::Response(r) => {
                    // DR-1 修复：同 wait_settled —— 失败响应快速失败，不傻等 settled
                    if !r.success {
                        return Err(self.command_failure(&r));
                    }
                    out.responses.push(r)
                }
                crate::engine::protocol::Inbound::Event(e) => {
                    // 自审 minor（140）：权威终结文本到达即覆盖 delta 拼接（同 wait_settled）
                    if let Some(full) = e.authoritative_text() {
                        out.text.clear();
                        out.text.push_str(full);
                        crate::observability::note_agent_event_for(&self.spec.run_id);
                    } else if let Some(delta) = e.text_delta() {
                        out.text.push_str(delta);
                        // 2026-10-27 自审 minor（31）：流式文本无上限累积（长轮次线性吃内存）。
                        // 协议块按全仓口径在**尾部**（D026），故超限保尾丢头，并留痕。
                        const MAX_TEXT_CHARS: usize = 4_000_000;
                        if out.text.chars().count() > MAX_TEXT_CHARS {
                            let keep_from = out.text.len().saturating_sub(MAX_TEXT_CHARS / 2);
                            let mut cut = keep_from;
                            while cut < out.text.len() && !out.text.is_char_boundary(cut) {
                                cut += 1;
                            }
                            out.text.drain(..cut);
                            tracing::warn!(
                                component = "engine",
                                role = %self.spec.role,
                                run_id = %self.spec.run_id,
                                "轮次文本超过上限，已丢弃头部保留尾部（协议块在尾部）"
                            );
                        }
                        // DR-3 精化：同 wait_settled —— 流式即活性
                        crate::observability::note_agent_event_for(&self.spec.run_id);
                    }
                    self.note_step_telemetry(&e, &mut tool_starts, &mut out);
                    if e.r#type == "extension_ui_request" {
                        // P8-ISSUE-07 修复：按 method 分流——已知 dialog 走应答链路
                        if is_dialog_method(&e.extra) {
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
                            self.send_ui_response(resp, "UI 应答")?;
                            out.events.push(e);
                            continue;
                        }
                        let method =
                            e.extra.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                        const FIRE_AND_FORGET: [&str; 5] =
                            ["notify", "setStatus", "setWidget", "setTitle", "set_editor_text"];
                        if FIRE_AND_FORGET.contains(&method) {
                            // 已知 fire-and-forget：不期待 response，误应答属协议噪音 → 只记录
                            out.events.push(e);
                            continue;
                        }
                        // 2026-10-27 自审 major：未知 method 若恰好是新增 dialog 类，
                        // 沉默不应答 = pi 阻塞等答整轮永挂。回显式错误应答兜底（
                        // fire-and-forget 类收到多余 response 无害；dialog 类拿到错误能收场）。
                        tracing::warn!(
                            component = "engine",
                            event = "extension_ui.unknown_method",
                            role = %self.spec.role,
                            method = method,
                            "未知 extension_ui method——回错误应答防子进程阻塞等答"
                        );
                        let req_id = e
                            .extra
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let resp = serde_json::json!({
                            "id": req_id,
                            "type": "extension_ui_response",
                            "error": format!("client 不支持该 UI method：{method}"),
                        });
                        self.send_ui_response(resp, "未知 method 错误应答")?;
                        out.events.push(e);
                        continue;
                    }
                    let settled = e.is_agent_settled();
                    out.events.push(e);
                    // 事件只是遥测原料：超窗口丢最旧（避免长轮次无界增长）
                    const MAX_EVENTS: usize = 50_000;
                    if out.events.len() > MAX_EVENTS {
                        let drop_n = out.events.len() - MAX_EVENTS;
                        out.events.drain(..drop_n);
                    }
                    if settled {
                        self.log_turn_end(started_at_ms, &out);
                        return Ok(out);
                    }
                }
            }
        }
    }

    /// UI 应答写 stdin（引擎审计修复 #7：kill 后 stdin 关闭不再 panic）。
    fn send_ui_response(&mut self, resp: serde_json::Value, what: &str) -> Result<()> {
        let mut l = serde_json::to_string(&resp).ctx("序列化 UI 应答")?;
        l.push('\n');
        self.stdin
            .as_mut()
            .ok_or_else(|| {
                DevforgeError::RpcProtocol("stdin 已关闭：子进程被杀后无法发送 UI 应答".into())
            })?
            .write_all(l.as_bytes())
            .ctx(format!("发送 {what}"))?;
        self.stdin
            .as_mut()
            .ok_or_else(|| {
                DevforgeError::RpcProtocol("stdin 已关闭：子进程被杀后无法 flush UI 应答".into())
            })?
            .flush()
            .ctx(format!("flush {what}"))?;
        Ok(())
    }

    /// DR-1 修复：把 pi 的命令失败（success=false，如缺 API key）转成结构化错误。
    /// 此前这类失败只被归档进 responses 继续傻等 settled，run 无声挂死。
    fn command_failure(&self, r: &ResponseRecord) -> DevforgeError {
        crate::observability::note_agent_event_for(&self.spec.run_id);
        let detail = r
            .safe_error()
            .unwrap_or_else(|| "（无错误详情）".into());
        tracing::warn!(
            component = "engine",
            event = "agent.command_failed",
            run_id = %self.spec.run_id,
            role = %self.spec.role,
            command = %r.command,
            error = %detail,
            "agent 命令失败（快速失败，不等 settled）"
        );
        Self::classify_command_failure(&self.spec.role, &r.command, &detail)
    }

    /// 引擎 #7 分流 + 2026-10-27 自审 major：抽成自由函数钉单测——旧口径内联在
    /// command_failure 里，测试够不看（变异成一律 Config 后 16 条测试仍全绿）。
    /// 瞬态上游/限流/连接中断归 System（可重启重试）；认不出的归 Config（不盲重启）。
    fn classify_command_failure(role: &str, command: &str, detail: &str) -> DevforgeError {
        if Self::transient_failure(detail) {
            return DevforgeError::System(format!(
                "agent[{role}] 命令 {command} 失败：{detail}（瞬态上游/限流故障，按基础设施错误重试）"
            ));
        }
        // 审计修复 #2：配置类命令失败用 Config（非 System）——is_infra_failure 只认裸 System，
        // 用 System 会盲重启 4 次（退避 10s/30s/90s）；配置问题重启永远不会变好，
        // require_profile 的「配置问题用 Config」口径在此对齐。
        DevforgeError::Config(format!(
            "agent[{role}] 命令 {command} 失败：{detail}（配置问题：缺 API key / 模型名错误；见 auth.json 与 models.json，修好配置后重新启动 run）"
        ))
    }

/// 瞬态故障判定（引擎 #7）：只看错误文本里**明确的**瞬态标记，认不出的仍归配置问题
/// （保守方向：宁可按配置失败让人看一眼，也不把真配置问题拖进 4 次盲重启）。
fn transient_failure(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    // 状态码按「独立数字」匹配，避开 500000 tokens 这类误命中
    const CODES: [&str; 5] = ["429", "500", "502", "503", "504"];
    if d
        .split(|c: char| !c.is_ascii_digit())
        .any(|t| CODES.contains(&t))
    {
        return true;
    }
    const MARKS: [&str; 12] = [
        "rate limit",
        "ratelimit",
        "too many requests",
        "overloaded",
        "timed out",
        "timeout",
        "econnreset",
        "econnrefused",
        "etimedout",
        "connection reset",
        "service unavailable",
        "temporarily unavailable",
    ];
    MARKS.iter().any(|m| d.contains(m))
}

/// P012 · path-guard 否决判定：RPC 事件的 tool_execution_end 是否为插件拦截。
/// 约定：isError=true 且 result 文本含 `[path-guard] ` 前缀（插件 block reason 的固定前缀）。
fn is_path_guard_denial(e: &EventRecord) -> bool {
    if !e.extra.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }
    e.extra
        .get("result")
        .map(|v| v.to_string())
        .unwrap_or_default()
        .contains("[path-guard] ")
}

    /// DR-9 L2：步进遥测——工具调用起止 + usage 采集（probe4 实证事件形态：
    /// tool_execution_start/end 带 toolName+toolCallId；message_update 带 usage）。
    fn note_step_telemetry(
        &self,
        e: &EventRecord,
        tool_starts: &mut std::collections::HashMap<String, std::time::Instant>,
        out: &mut WaitOutcome,
    ) {
        match e.r#type.as_str() {
            "tool_execution_start" => {
                let tool = e
                    .extra
                    .get("toolName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                if let Some(id) = e.extra.get("toolCallId").and_then(|v| v.as_str()) {
                    tool_starts.insert(id.to_string(), std::time::Instant::now());
                }
                crate::observability::note_agent_event_for(&self.spec.run_id);
                tracing::info!(
                    component = "engine",
                    event = "agent.step",
                    run_id = %self.spec.run_id,
                    role = %self.spec.role,
                    tool = %tool,
                    phase = "start",
                    "工具调用开始"
                );
            }
            "tool_execution_end" => {
                let tool = e
                    .extra
                    .get("toolName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let ms = e
                    .extra
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .and_then(|id| tool_starts.remove(id))
                    .map(|t| t.elapsed().as_millis() as u64);
                crate::observability::note_agent_event_for(&self.spec.run_id);
                tracing::info!(
                    component = "engine",
                    event = "agent.step",
                    run_id = %self.spec.run_id,
                    role = %self.spec.role,
                    tool = %tool,
                    phase = "end",
                    ms = ms,
                    "工具调用结束"
                );
                // P012 · path-guard 事件回流：被插件拦截的调用（isError + reason 前缀约定）
                // 转 boundary.violation —— 与 M0 引擎终校验共用同一事件名，过滤时对得上。
                if Self::is_path_guard_denial(e) {
                    // 2026-10-27 自审 major：除日志外计入结构化计数——WaitOutcome 消费方
                    // （编排层，持有事件总线）可据此把拦截事实发上总线；旧口径只落
                    // tracing，与 server 侧同名事件的「进总线」口径对不上。
                    out.boundary_denials += 1;
                    tracing::warn!(
                        component = "engine",
                        event = "boundary.violation",
                        run_id = %self.spec.run_id,
                        role = %self.spec.role,
                        tool = %tool,
                        "path-guard 拦截了越界工具调用"
                    );
                }
            }
            _ => {}
        }
        if let Some(u) = e.extra.get("usage") {
            let inp = u.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
            let outp = u.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
            if inp > 0 || outp > 0 {
                out.last_usage = Some((inp, outp));
            }
        }
    }

    /// 「agent 轮次结束」里程碑（Q5：唯一落点，覆盖全部 SOP——change 侧不再另落）。
    ///
    /// 字段：角色名（`spec.role`）、本轮耗时（毫秒）、本轮产出字符数。
    /// 作用：交卷 → 下一次开庭之间的空洞由此可算（此前那 15 分钟无任何记录）。
    fn log_turn_end(&self, started_at_ms: u64, out: &WaitOutcome) {
        self.log_turn_end_with(started_at_ms, out, "settled");
    }

    /// 带结局的轮次结束记账（`settled` / `aborted`）。
    fn log_turn_end_with(&self, started_at_ms: u64, out: &WaitOutcome, outcome: &str) {
        crate::observability::note_agent_event_for(&self.spec.run_id);
        tracing::info!(
            component = "engine",
            event = "agent.turn_end",
            run_id = %self.spec.run_id,
            role = %self.spec.role,
            turn_ms = now_ms().saturating_sub(started_at_ms),
            chars = out.text.chars().count(),
            last_tokens_in = out.last_usage.map(|u| u.0),
            last_tokens_out = out.last_usage.map(|u| u.1),
            outcome,
            "agent 轮次结束"
        );
    }

    /// 校验某 id 的响应成功；失败则转为该角色 Agent 的失败错误（可恢复类）。
    ///
    /// 2026-10-27 自审 minor（130）：本方法是**第三种命令失败分类**（Agent 变体），
    /// 与 command_failure 的 System/Config 分流不同口径——现明确分化：
    /// await 路径上已由 command_failure 快速失败，这里的 Agent 变体只覆盖
    /// 「响应存在但 success=false 且调用方辞端判定」的场景（不盲重启，交编排器决策）。
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
        self.deregister_from_ledger();
        Ok(())
    }

    /// 收场摘除台账条目（自审 minor 132）。幂等：摘过一次后 self.pid 置 None。
    fn deregister_from_ledger(&mut self) {
        if let (Some(path), Some(pid)) = (self.ledger_path.take(), self.ledger_pid.take()) {
            crate::engine::ledger::deregister(&path, pid);
        }
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        // 尽力收尸，避免 zombie
        let _ = self.child.kill();
        let _ = self.child.wait();
        // 自审 minor（132）：正常退出也要摘台账条目（旧口径只在崩溃路径靠对账清账）
        self.deregister_from_ledger();
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;

#[cfg(test)]
#[path = "client_unit_tests.rs"]
mod tests;
