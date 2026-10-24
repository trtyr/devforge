//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, fixture::*, guards::*, milestone::*, panic::*, runlog::*};

pub(super) fn log_lines() -> &'static Mutex<Vec<String>> {
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}
pub(super) fn acc() -> &'static Mutex<String> {
    ACC.get_or_init(|| Mutex::new(String::new()))
}
pub(super) fn capture_lock() -> &'static Mutex<()> {
    CAPTURE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 把 tracing 的 JSONL 行收进进程级缓冲（测试进程内的唯一订阅器）。
pub(super) struct BufferWriter;

impl std::io::Write for BufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut a = acc().lock().expect("缓冲锁");
        a.push_str(&String::from_utf8_lossy(buf));
        while let Some(pos) = a.find('\n') {
            let rest = a.split_off(pos + 1);
            let line = std::mem::replace(&mut *a, rest);
            if !line.trim().is_empty() {
                log_lines()
                    .lock()
                    .expect("缓冲锁")
                    .push(line.trim_end().to_string());
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferWriter {
    type Writer = BufferWriter;
    fn make_writer(&'a self) -> Self::Writer {
        BufferWriter
    }
}

pub(super) fn install_logger() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let sub = tracing_subscriber::fmt()
            .json()
            .with_writer(BufferWriter)
            .finish();
        let _ = tracing::subscriber::set_global_default(sub);
    });
}

/// 日志采集会话：持锁（产日志的用例串行跑 → 彼此的日志不串味），期间产生的行可取。
pub(super) struct Capture {
    _guard: MutexGuard<'static, ()>,
    start: usize,
}

impl Capture {
    pub(super) fn new() -> Self {
        install_logger();
        let guard = capture_lock().lock().unwrap_or_else(|e| e.into_inner());
        let start = log_lines().lock().expect("缓冲锁").len();
        Capture {
            _guard: guard,
            start,
        }
    }
    /// 采集期间新增的 JSONL 行。
    pub(super) fn lines(&self) -> Vec<String> {
        log_lines().lock().expect("缓冲锁")[self.start..].to_vec()
    }
}

/// 取含某个片段的日志行。
pub(super) fn lines_with<'a>(lines: &'a [String], needle: &str) -> Vec<&'a str> {
    lines
        .iter()
        .map(|l| l.as_str())
        .filter(|l| l.contains(needle))
        .collect()
}

// ============================================================ 假 agent / 假审查

/// 不问用户的用户链路（单测里问用户 = 剧本走偏）。
pub(super) struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 脚本化 agent：按队列吐回复，并记下收到的每条指令。
pub(super) struct ScriptedAgent {
    replies: VecDeque<String>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ScriptedAgent {
    pub(super) fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|s| s.to_string()).collect(),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl PlanAgentSession for ScriptedAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.seen.borrow_mut().push(message.to_string());
        Ok(self.replies
            .pop_front()
            .expect("agent 脚本耗尽（少给了一轮回复）"))
    }
}

pub(super) struct ScriptedReview {
    queue: Arc<Mutex<VecDeque<String>>>,
}

impl ReviewSession for ScriptedReview {
    fn review(&mut self, _instruction: &str) -> Result<String> {
        Ok(self
            .queue
            .lock()
            .expect("review 队列锁")
            .pop_front()
            .expect("review 脚本耗尽（少给了一条回复）"))
    }
}

/// 脚本化审查工厂：每轮开一个全新实例。
/// 内部用 Arc<Mutex>（ARC-R1：run_pipeline 的工厂要跨审查线程，需 Send）。
pub(super) struct ScriptedFactory {
    queue: Arc<Mutex<VecDeque<String>>>,
    opened: Arc<Mutex<usize>>,
}

impl ScriptedFactory {
    pub(super) fn new(replies: &[&str]) -> Self {
        Self {
            queue: Arc::new(Mutex::new(
                replies.iter().map(|s| s.to_string()).collect(),
            )),
            opened: Arc::new(Mutex::new(0)),
        }
    }
}

impl ReviewSessionFactory for ScriptedFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        *self.opened.lock().expect("opened 锁") += 1;
        Ok(Box::new(ScriptedReview {
            queue: self.queue.clone(),
        }))
    }
}

/// MVP planning 的审查协议（`<verdict>` 块）。
pub(super) const PLAN_REVIEW_PASS: &str = "通顺。\n<verdict>PASS</verdict>";
pub(super) const PLAN_REVIEW_FAIL: &str =
    "有矛盾。\n<verdict>FAIL</verdict>\n<issues>\n- 缺一条边界\n</issues>";
/// MVP coding / testing / change 的审查协议（`<stage-verdict>` + `<next>`）。
pub(super) const STAGE_REVIEW_PASS_FINISH: &str = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
pub(super) const STAGE_REVIEW_FAIL: &str =
    "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 缺一条边界\n</issues>";
/// doc-from-zero 的审查协议（`<doc-verdict>` + `<route>` + `<issues>`）。
pub(super) const DOC_REVIEW_PASS: &str = "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>";
pub(super) const DOC_REVIEW_FAIL: &str =
    "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>";

// ============================================================ 服务器夹具

/// 假引擎：立刻完成 / 挂起一段时间 / 前 N 次抛基础设施故障。
pub(super) struct MockEngine {
    pub(super) calls: Arc<Mutex<Vec<String>>>,
    pub(super) hang_ms: u64,
    pub(super) fail_system_times: Arc<Mutex<u32>>,
}

impl RunEngine for MockEngine {
    fn execute(&mut self, task: &Task, _run: &Run, control: &RunControl) -> Result<RunOutcome> {
        self.calls
            .lock()
            .expect("calls 锁")
            .push(format!("{}/{}", task.project, task.id));
        {
            let mut left = self.fail_system_times.lock().expect("failures 锁");
            if *left > 0 {
                *left -= 1;
                return Err(DevforgeError::System("模拟上游 500（测试注入）".into()));
            }
        }
        if self.hang_ms > 0 {
            let deadline = std::time::Instant::now() + Duration::from_millis(self.hang_ms);
            while std::time::Instant::now() < deadline {
                if control.is_cancelled() {
                    return Ok(RunOutcome::Failed {
                        reason: "已取消（cancel 端点）".into(),
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(RunOutcome::Completed)
    }
}

pub(super) struct ServerFixture {
    pub(super) router: axum::Router,
    pub(super) state: Arc<devforge::server::ServerState>,
    pub(super) calls: Arc<Mutex<Vec<String>>>,
    pub(super) obs_root: PathBuf,
    pub(super) _dir: tempfile::TempDir,
}


