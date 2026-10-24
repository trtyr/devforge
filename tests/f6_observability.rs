//! F6 靶的验收对账测试（黑盒口径，plan 010 · change-20261022-c7-1）。
//!
//! 口径：只经**公开面**观察——`devforge::` 的导出面、HTTP 端点、日志条目、落盘文件；
//! 不读私有状态、不改产品代码。每条用例首行写「错实现形态」：把实现改坏时本用例必须变红。
//!
//! 覆盖面 = 变更计划 §5 的验收标准（逐条对账见黑板 `testing/acceptance.md`）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};

use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession};
use devforge::domain::run::Run;
use devforge::domain::runner::{RunControl, RunEngine, RunOutcome};
use devforge::domain::sop::SopRegistry;
use devforge::domain::store::Store;
use devforge::domain::task::Task;
use devforge::engine::client::RpcClient;
use devforge::engine::process::SpawnSpec;
use devforge::engine::retry::{RetryPolicy, run_with_retry};
use devforge::error::{Context as _, DevforgeError, Result};
use devforge::observability as obs;
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

// ============================================================ 日志面捕获

static LOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static ACC: OnceLock<Mutex<String>> = OnceLock::new();
static CAPTURE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn log_lines() -> &'static Mutex<Vec<String>> {
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}
fn acc() -> &'static Mutex<String> {
    ACC.get_or_init(|| Mutex::new(String::new()))
}
fn capture_lock() -> &'static Mutex<()> {
    CAPTURE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 把 tracing 的 JSONL 行收进进程级缓冲（测试进程内的唯一订阅器）。
struct BufferWriter;

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

fn install_logger() {
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
struct Capture {
    _guard: MutexGuard<'static, ()>,
    start: usize,
}

impl Capture {
    fn new() -> Self {
        install_logger();
        let guard = capture_lock().lock().unwrap_or_else(|e| e.into_inner());
        let start = log_lines().lock().expect("缓冲锁").len();
        Capture {
            _guard: guard,
            start,
        }
    }
    /// 采集期间新增的 JSONL 行。
    fn lines(&self) -> Vec<String> {
        log_lines().lock().expect("缓冲锁")[self.start..].to_vec()
    }
}

/// 取含某个片段的日志行。
fn lines_with<'a>(lines: &'a [String], needle: &str) -> Vec<&'a str> {
    lines
        .iter()
        .map(|l| l.as_str())
        .filter(|l| l.contains(needle))
        .collect()
}

// ============================================================ 假 agent / 假审查

/// 不问用户的用户链路（单测里问用户 = 剧本走偏）。
struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 脚本化 agent：按队列吐回复，并记下收到的每条指令。
struct ScriptedAgent {
    replies: VecDeque<String>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ScriptedAgent {
    fn new(replies: &[&str]) -> Self {
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

struct ScriptedReview {
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
struct ScriptedFactory {
    queue: Arc<Mutex<VecDeque<String>>>,
    opened: Arc<Mutex<usize>>,
}

impl ScriptedFactory {
    fn new(replies: &[&str]) -> Self {
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
const PLAN_REVIEW_PASS: &str = "通顺。\n<verdict>PASS</verdict>";
const PLAN_REVIEW_FAIL: &str =
    "有矛盾。\n<verdict>FAIL</verdict>\n<issues>\n- 缺一条边界\n</issues>";
/// MVP coding / testing / change 的审查协议（`<stage-verdict>` + `<next>`）。
const STAGE_REVIEW_PASS_FINISH: &str = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
const STAGE_REVIEW_FAIL: &str =
    "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 缺一条边界\n</issues>";
/// doc-from-zero 的审查协议（`<doc-verdict>` + `<route>` + `<issues>`）。
const DOC_REVIEW_PASS: &str = "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>";
const DOC_REVIEW_FAIL: &str =
    "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>";

// ============================================================ 服务器夹具

/// 假引擎：立刻完成 / 挂起一段时间 / 前 N 次抛基础设施故障。
struct MockEngine {
    calls: Arc<Mutex<Vec<String>>>,
    hang_ms: u64,
    fail_system_times: Arc<Mutex<u32>>,
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

struct ServerFixture {
    router: axum::Router,
    state: Arc<devforge::server::ServerState>,
    calls: Arc<Mutex<Vec<String>>>,
    obs_root: PathBuf,
    _dir: tempfile::TempDir,
}

/// 造一个服务器夹具（观测根钉在夹具的临时目录里 → 不落用户 HOME）。
fn server_fixture(hang_ms: u64, fail_system_times: u32) -> ServerFixture {
    let dir = tempfile::tempdir().expect("临时目录");
    let obs_root = dir.path().join("observability");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let failures = Arc::new(Mutex::new(fail_system_times));
    let engine_factory = Box::new(move || -> Box<dyn RunEngine + Send> {
        Box::new(MockEngine {
            calls: calls2.clone(),
            hang_ms,
            fail_system_times: failures.clone(),
        })
    });
    let bus = Arc::new(devforge::server::EventBus::new());
    let ui = Arc::new(devforge::server::UiRelay::new(bus.clone()));
    let mut state = devforge::server::ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        devforge::config::ConfigManifest::builtin(),
        engine_factory,
        bus,
        ui,
        Arc::new(devforge::config::installer::FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // 单测不真睡：重试退避归零
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .retry_policy = RetryPolicy::fast();
    // 观测根钉在夹具的临时目录
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .obs_root = Some(obs_root.clone());
    let router = devforge::server::build_router(state.clone());
    ServerFixture {
        router,
        state,
        calls,
        obs_root,
        _dir: dir,
    }
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    use tower::ServiceExt as _;
    let builder = axum::http::Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(v.to_string()))
            .expect("请求构造"),
        None => builder.body(axum::body::Body::empty()).expect("请求构造"),
    };
    router.clone().oneshot(req).await.expect("端点调用")
}

async fn body_json(resp: axum::response::Response) -> Value {
    use http_body_util::BodyExt as _;
    let bytes = resp.into_body().collect().await.expect("取响应体").to_bytes();
    serde_json::from_slice(&bytes).expect("响应应为 JSON")
}

/// 建项目 + 建任务（走真实 HTTP 面）。
async fn make_task(f: &ServerFixture, project: &str, task: &str, sop: &str) {
    let local = f
        .state
        .plans_root
        .parent()
        .expect("根")
        .join(format!("ws-{project}"));
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": project, "local_path": local.to_string_lossy()})),
    )
    .await;
    send(
        &f.router,
        "POST",
        &format!("/api/v1/projects/{project}/tasks"),
        Some(json!({"id": task, "sop": sop})),
    )
    .await;
}

/// 等 run 进终态。
async fn wait_settled(f: &ServerFixture, project: &str, task: &str) {
    for _ in 0..300 {
        let body = body_json(
            send(
                &f.router,
                "GET",
                &format!("/api/v1/projects/{project}/tasks/{task}/runs"),
                None,
            )
            .await,
        )
        .await;
        let st = body["runs"]
            .as_array()
            .and_then(|runs| runs.last())
            .and_then(|r| r["status"].as_str())
            .unwrap_or("");
        if st == "Settled" || st == "Failed" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("run 未在上限内进入终态：{project}/{task}");
}

/// 事件总线里的全部事件（回放缓冲）。
fn events(f: &ServerFixture) -> Vec<String> {
    f.state
        .bus
        .replay_after(0)
        .into_iter()
        .map(|(_, data)| data)
        .collect()
}

// ============================================================ 需求 1：原始错误全量可见

#[test]
fn infra_failure_is_logged_as_error_with_the_full_text_and_the_source_chain() {
    // 错实现形态：①非质量类失败落 WARN（应 ERROR）②缺 error_chain（source 链看不见）
    // ③缺 attempt / component / event → 本用例必须变红
    let cap = Capture::new();
    let marker = "f6-infra-marker-1";
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            // 带上下文的包装错误：source 链有两层（上下文层 + 内层错误）
            let inner: Result<()> = Err(DevforgeError::System(format!("上游 500（{marker}）")));
            Err(inner.ctx("拉模型响应").unwrap_err())
        });
        assert_eq!(attempts, 1, "包装过的错误不是裸 System → 不进自动重启分支");
        assert!(result.is_err());
    }
    let text = cap.lines();
    let lines = lines_with(&text, "run.attempt_failed");
    assert_eq!(lines.len(), 1, "每次尝试一条：{text:?}");
    let first = lines[0];
    assert!(
        first.contains("\"level\":\"ERROR\""),
        "非质量类失败落 ERROR：{first}"
    );
    assert!(
        first.contains("\"component\":\"run-retry\""),
        "字段集要含 component：{first}"
    );
    assert!(
        first.contains("\"event\":\"run.attempt_failed\""),
        "字段集要含 event：{first}"
    );
    assert!(first.contains("\"attempt\":1"), "要带 attempt：{first}");
    // 原始错误全文：顶层 Display 逐字 + source 链每一层
    assert!(first.contains(marker), "缺原始错误全文：{first}");
    assert!(first.contains("拉模型响应"), "缺上下文层：{first}");
    assert!(first.contains("→"), "source 链要用 → 连接：{first}");
    assert!(first.contains("error_chain"), "缺 source 链字段：{first}");
    assert!(
        first.contains("\"retryable\":false"),
        "这条失败不重启（不是裸 System）：{first}"
    );
}

#[test]
fn quality_failure_is_logged_as_warn_with_the_same_fields_and_is_not_retried() {
    // 错实现形态：①质量失败落 ERROR（应 WARN）②字段与基础类不同构
    // ③质量失败被当成基础设施故障重启 → 本用例必须变红
    let cap = Capture::new();
    let marker = "f6-quality-marker-1";
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            Err(DevforgeError::Quality(format!(
                "审查打回：缺验收标准（{marker}）"
            )))
        });
        assert_eq!(attempts, 1, "质量失败不得重启");
        assert!(result.is_err());
    }
    let text = cap.lines();
    let lines = lines_with(&text, "run.attempt_failed");
    assert_eq!(lines.len(), 1, "质量失败只落一条：{text:?}");
    let line = lines[0];
    assert!(line.contains("\"level\":\"WARN\""), "质量失败落 WARN：{line}");
    assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
    assert!(line.contains("\"component\":\"run-retry\""), "{line}");
    assert!(line.contains("\"attempt\":1"), "{line}");
    assert!(line.contains("\"retryable\":false"), "质量失败不重启：{line}");
    assert!(line.contains(marker), "缺原始错误全文：{line}");
    assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
    assert!(
        !line.contains("\"level\":\"ERROR\""),
        "质量失败不得占 ERROR 级别：{line}"
    );
}

#[test]
fn four_infra_failures_log_four_attempt_entries_and_one_exhausted_entry() {
    // 错实现形态：①只落一条（把四次尝试合成一条）②缺 run.attempt_exhausted
    // ③终态条目不带总尝试次数 → 本用例必须变红
    let cap = Capture::new();
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
            Err(DevforgeError::System(format!("第 {n} 次挂了")))
        });
        assert_eq!(attempts, 4, "首次 + 3 次重试");
        assert!(result.is_err());
    }
    let text = cap.lines();
    let attempts = lines_with(&text, "run.attempt_failed");
    assert_eq!(attempts.len(), 4, "4 次尝试各一条：{text:?}");
    for (i, line) in attempts.iter().enumerate() {
        assert!(line.contains(&format!("\"attempt\":{}", i + 1)), "{line}");
        assert!(line.contains("\"level\":\"ERROR\""), "基础设施故障落 ERROR：{line}");
    }
    let exhausted = lines_with(&text, "run.attempt_exhausted");
    assert_eq!(exhausted.len(), 1, "应有且仅有一条终态条目：{text:?}");
    assert!(
        exhausted[0].contains("\"attempts\":4"),
        "终态条目要带总尝试次数：{}",
        exhausted[0]
    );
    assert!(
        exhausted[0].contains("\"level\":\"ERROR\""),
        "终态条目与尝试级同级：{}",
        exhausted[0]
    );
}

#[tokio::test]
async fn the_error_entry_is_logged_before_the_restart_warning() {
    // 错实现形态：先公告「自动重启」再记错误（或错误只在终态落）→ 现场只剩「重启了」，
    // 看不出「为什么重启」→ 本用例必须变红
    let cap = Capture::new();
    let f = server_fixture(0, 1);
    make_task(&f, "p", "t", "planning").await;
    let engine: Box<dyn RunEngine + Send> = Box::new(MockEngine {
        calls: Arc::new(Mutex::new(Vec::new())),
        hang_ms: 0,
        fail_system_times: Arc::new(Mutex::new(1)),
    });
    devforge::server::run_in_background(f.state.clone(), "p".into(), "t".into(), engine);
    let text = cap.lines();
    let err_at = text
        .iter()
        .position(|l| l.contains("run.attempt_failed") && l.contains("\"attempt\":1"))
        .unwrap_or_else(|| panic!("第 1 次失败应落一条尝试级条目：{text:?}"));
    let restart_at = text
        .iter()
        .position(|l| l.contains("自动重启 run"))
        .unwrap_or_else(|| panic!("基础设施故障应触发自动重启：{text:?}"));
    assert!(
        err_at < restart_at,
        "错误条目必须先于「自动重启 run」落盘（先记错误、再决定重启）：{text:?}"
    );
    assert!(
        text[err_at].contains("\"level\":\"ERROR\""),
        "非质量类失败落 ERROR：{}",
        text[err_at]
    );
    assert!(
        text[restart_at].contains("\"attempt\":2"),
        "重启的是第 2 次尝试：{}",
        text[restart_at]
    );
}

// ---- R-1：attempt 的写入时机（每次尝试**开始时**写进 run 上下文）----

#[test]
fn the_run_context_carries_the_current_attempt_during_a_later_attempt() {
    // 错实现形态：attempt 只在**失败分支**里写（第 1 次失败之后才写 1）→ 第 2 次尝试期间
    //   上下文仍是上一次的值 → 落点②（`map_intervention` 的质量 WARN）读到陈旧的 attempt
    //   → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let run_id = "run-f6-attempt";
    obs::begin_run_in(dir.path().to_path_buf(), run_id, "coding");
    let mut seen: Vec<u32> = Vec::new();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
        // 每次尝试**开始时**，上下文里就应当是本次尝试号
        // （重试层在 `execute(attempt)` 之前写；`map_intervention` 从同一处读）
        seen.push(obs::attempt());
        if n == 1 {
            // 基础设施故障 → 触发自动重启（第 2 次尝试）
            Err(DevforgeError::System("模拟上游 500（测试注入）".into()))
        } else {
            // 第 2 次尝试期间的质量失败（生产链路上被 `map_intervention` 收成 `Ok(Failed)`）
            Err(DevforgeError::Quality("终审未通过".into()))
        }
    });
    obs::end_run(run_id);
    assert_eq!(
        attempts, 2,
        "第 1 次基础设施故障 → 自动重启一次；第 2 次的质量失败不再重启"
    );
    assert!(result.is_err(), "质量失败以错误收场");
    assert_eq!(
        seen,
        vec![1, 2],
        "每次尝试**开始时** run 上下文就应是本次尝试号（R-1：不是上一次的值）：{seen:?}"
    );
}

// ============================================================ 根因修复：渲染闸门

#[tokio::test]
async fn start_waits_at_the_render_gate_until_the_required_profile_is_ready() {
    // 错实现形态：闸门不存在（start 立即开跑 → 门卫判「profile 未渲染」→ 盲重启）
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let waiting = tokio::time::timeout(
        Duration::from_millis(200),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await;
    assert!(
        waiting.is_err(),
        "渲染在途且所需 profile 未就绪时，start 不得立即开跑"
    );
    // 所需 profile 就绪 → 闸门放行 → start 202
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
    }
    f.state.render_gate.end_render();
    let resp = tokio::time::timeout(
        Duration::from_secs(2),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("闸门放行后 start 应当返回");
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn start_returns_immediately_when_the_required_profiles_are_ready_even_if_a_render_is_in_flight() {
    // 错实现形态：闸门等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
    }
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let resp = tokio::time::timeout(
        Duration::from_millis(500),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("所需 profile 已就绪时 start 不得等整轮渲染");
    assert_eq!(resp.status(), 202);
    // 渲染仍在途（闸门没有等整轮渲染）
    assert!(f.state.render_gate.is_in_flight());
    f.state.render_gate.end_render();
}

#[tokio::test]
async fn a_ready_run_passes_on_the_first_attempt_without_a_restart_warning() {
    // 错实现形态：profile 就绪也走成「失败 → 自动重启」（attempt=2 的重启 WARN 出现）
    // → 本用例必须变红
    let cap = Capture::new();
    let f = server_fixture(0, 0);
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
    }
    make_task(&f, "p", "t1", "coding").await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t1").await;
    let log = cap.lines();
    assert!(
        lines_with(&log, "自动重启 run").is_empty(),
        "profile 就绪时第一次尝试即通过，不得出现自动重启 WARN：{log:?}"
    );
    let finished = events(&f)
        .into_iter()
        .find(|e| e.contains("\"type\":\"run.finished\""))
        .expect("应有 run.finished 事件");
    assert!(
        finished.contains("\"attempts\":1"),
        "尝试次数应为 1：{finished}"
    );
}

#[test]
fn a_missing_profile_fails_the_run_explicitly_without_a_restart() {
    // 错实现形态：profile 缺失时错误文本不点名缺失的 profile / 路径，或被当成基础设施故障盲重启
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let mut engine = devforge::server::BuiltinRpcEngine {
        profile_root: dir.path().join("profiles"),
        session_root: dir.path().join("sessions"),
        plans_root: dir.path().join("plans"),
        ui: Arc::new(devforge::server::UiRelay::new(Arc::new(
            devforge::server::EventBus::new(),
        ))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let (result, attempts) =
        run_with_retry(&RetryPolicy::fast(), &control, |_| engine.execute(&task, &run, &control));
    assert_eq!(attempts, 1, "profile 未就绪不得触发自动重启（尝试次数恒为 1）");
    let err = result.expect_err("profile 未渲染时 run 必须失败");
    let msg = err.to_string();
    assert!(msg.contains("coding-agent"), "错误要点名缺失的 profile：{msg}");
    assert!(msg.contains("settings.json"), "错误要点名缺失的路径：{msg}");
    assert!(msg.contains("未渲染"), "错误要说明未渲染：{msg}");
}

#[test]
fn a_missing_profile_is_a_config_error_and_is_not_an_infrastructure_failure() {
    // 错实现形态：错误种类仍是 System（会被 is_infra_failure 判成基础设施故障 → 盲重启 3 次）
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let mut engine = devforge::server::BuiltinRpcEngine {
        profile_root: dir.path().join("profiles"),
        session_root: dir.path().join("sessions"),
        plans_root: dir.path().join("plans"),
        ui: Arc::new(devforge::server::UiRelay::new(Arc::new(
            devforge::server::EventBus::new(),
        ))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine
        .execute(&task, &run, &control)
        .expect_err("profile 未渲染时必须失败");
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "profile 未就绪应是配置错误：{err:?}"
    );
    assert!(
        !devforge::engine::retry::is_infra_failure(&Err(err)),
        "配置错误不得被判为基础设施故障（否则会盲重启）"
    );
}

#[tokio::test]
async fn other_endpoints_are_served_while_a_start_waits_at_the_render_gate() {
    // 错实现形态：闸门等待用阻塞式等待占住 tokio worker → 等待期内其它端点的请求被拖住
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let r = f.router.clone();
    let waiter = tokio::spawn(async move {
        send(&r, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let resp = tokio::time::timeout(
        Duration::from_millis(500),
        send(&f.router, "GET", "/api/v1/projects", None),
    )
    .await
    .expect("闸门等待期间其它端点必须能被及时处理");
    assert_eq!(resp.status(), 200);
    f.state.render_gate.end_render();
    let resp = tokio::time::timeout(Duration::from_secs(3), waiter)
        .await
        .expect("闸门放行后 start 应当返回")
        .unwrap();
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
async fn two_concurrent_starts_while_the_gate_waits_run_at_most_one() {
    // 错实现形态：两个并发 start 都开跑（双跑）；或输的那一路静默消失（既没开跑也没 run.error）
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(1500, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let r1 = f.router.clone();
    let r2 = f.router.clone();
    let a = tokio::spawn(async move {
        send(&r1, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    let b = tokio::spawn(async move {
        send(&r2, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
    }
    f.state.render_gate.end_render();
    assert_eq!(a.await.expect("第一个 start").status(), 202);
    assert_eq!(b.await.expect("第二个 start").status(), 202);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let run_errors = loop {
        let n = events(&f)
            .iter()
            .filter(|e| e.contains("\"type\":\"run.error\""))
            .count();
        if n > 0 || std::time::Instant::now() > deadline {
            break n;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        run_errors, 1,
        "并发 start 只能有一个开跑，另一个必须以 run.error 事件明确失败"
    );
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t1/runs", None).await,
    )
    .await;
    assert_eq!(
        body["runs"].as_array().map(|r| r.len()),
        Some(1),
        "只能有一个 run 真正开跑：{body}"
    );
    // 引擎调用发生在**后台 run 线程**里，而输的那一路的 run.error 事件先到
    // → 断言「引擎只被调用一次」前要有界等待它出现，
    // 否则机器一忙就会把「还没调到」误判成「零跑/双跑」（假红）
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let calls = loop {
        let n = f.calls.lock().expect("calls 锁").len();
        if n > 0 || std::time::Instant::now() > deadline {
            break n;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(calls, 1, "引擎只能被调用一次（至多一个 run 真正开跑）");
}

// ---- R-2：放行条件回环重判 / R-3：在途计数的归零 ----

#[tokio::test]
async fn the_gate_does_not_release_while_another_render_is_still_in_flight() {
    // 错实现形态：放行条件是「任意一次渲染结束就放行」（`rx.changed()` 分支直接 return）
    //   → 并发两轮渲染时，先结束的那一轮就把 run 放了出去（所需 profile 还没落盘）
    //   → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    // 两轮渲染在途
    f.state.render_gate.begin_render();
    f.state.render_gate.begin_render();
    let r = f.router.clone();
    let waiter = tokio::spawn(async move {
        send(&r, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "两轮渲染都在途时不得开跑");
    // 其中一轮结束 → **仍有渲染在途** → 仍不得放行（R-2：放行条件要回环重判）
    f.state.render_gate.end_render();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !waiter.is_finished(),
        "仍有渲染在途时不得放行（不能由「任意一次渲染结束」放行）"
    );
    // 所需 profile 全部就绪 → 放行（放行条件的另一半）
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
    }
    let resp = tokio::time::timeout(Duration::from_secs(3), waiter)
        .await
        .expect("所需 profile 就绪后闸门应放行")
        .unwrap();
    assert_eq!(resp.status(), 202);
    f.state.render_gate.end_render();
}

#[tokio::test]
async fn the_render_gate_count_goes_back_to_zero_after_the_render_thread_ends() {
    // 错实现形态：渲染结束后在途计数不归零（归零那一行被删 / drop 守卫没接上）
    //   → 之后的 start 永远在闸门上等 → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    // 走公开面：PUT 清单 → 渲染在后台线程跑（立即 202）
    let manifest = body_json(send(&f.router, "GET", "/api/v1/config/manifest", None).await).await;
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/config/manifest",
        Some(manifest),
    )
    .await;
    assert_eq!(resp.status(), 202, "PUT 清单立即 202（渲染在后台）");
    // 渲染线程结束后，在途计数必须归零
    // （R-3：归零由 drop 守卫保证，不依赖线程体末尾的显式调用）
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while f.state.render_gate.is_in_flight() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !f.state.render_gate.is_in_flight(),
        "渲染结束后闸门的在途计数必须归零（R-3）"
    );
    // 归零的后果：之后的 start 不会无限等待
    make_task(&f, "p", "t1", "coding").await;
    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("渲染结束后 start 不得无限等待（计数已归零）");
    assert_eq!(resp.status(), 202);
}

// ============================================================ 需求 2：里程碑

/// change 编排层的脚本化 session（plan 段打回一次 → 过；testing 段回注 coding）。
struct ScriptedChangeSession {
    plan_calls: usize,
}

impl ChangeSession for ScriptedChangeSession {
    fn run_plan(&mut self, _ui: &mut dyn UserLink, _roundtrip: usize) -> Result<PlanTurn> {
        self.plan_calls += 1;
        if self.plan_calls == 1 {
            Ok(PlanTurn::Fail {
                issues: vec!["影响面漏了调用方".into()],
            })
        } else {
            Ok(PlanTurn::Pass)
        }
    }
    fn amend_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _gaps: &[devforge::change::boundary::ImpactGap],
        _roundtrip: usize,
    ) -> Result<PlanTurn> {
        Ok(PlanTurn::Pass)
    }
    fn fix_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _issues: &[String],
        _round: usize,
    ) -> Result<PlanTurn> {
        Ok(PlanTurn::Pass)
    }
    fn run_coding(
        &mut self,
        _ui: &mut dyn UserLink,
        _roundtrip: usize,
        _last_failure: Option<&devforge::change::state::TestFailureContext>,
    ) -> Result<CodingTurn> {
        Ok(CodingTurn {
            reply: "改完了\n<stage-complete>1</stage-complete>".into(),
            changed: vec![],
            reviews: 1,
        })
    }
    fn run_testing(&mut self, _ui: &mut dyn UserLink) -> Result<TestingTurn> {
        Ok(TestingTurn::RouteCoding {
            failures: vec!["tests/a_test.rs:10 断言失败".into()],
        })
    }
}

#[test]
fn the_change_orchestrator_logs_the_phase_change_and_the_fix_pairs() {
    // 错实现形态：①阶段转换不落 phase.change（或缺 from/to）②修复轮不落 fix.start/fix.end
    // （或缺轮次）→ 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let store = StateStore::new(
        dir.path().join("plans"),
        StateKey::new("p", "t", devforge::change::state::CHANGE_SOP),
    );
    let mut session = ScriptedChangeSession { plan_calls: 0 };
    let run = ChangeRun {
        store: &store,
        repo: &dir.path().join("repo"),
        change_id: "change-f6".into(),
        baseline_sha: "deadbeef".into(),
        criteria: vec![],
        whitelist: vec!["src/a.rs".into()],
        usage: String::new(),
        run_id: "run-f6".into(),
        wall_secs: 1,
    };
    {
        let err = run_change(run, &mut session, &mut NoUser)
            .expect_err("测试回注后本轮以 Quality 结束");
        assert!(err.is_quality_failure(), "测试回注是质量失败：{err}");
    }
    let text = cap.lines();

    // 阶段转换：plan → coding
    let phases = lines_with(&text, "phase.change");
    let to_coding = phases
        .iter()
        .find(|l| l.contains("\"to\":\"coding\"") && l.contains("\"from\":\"plan\""))
        .unwrap_or_else(|| panic!("应有 plan → coding 的阶段转换：{text:?}"));
    assert!(to_coding.contains("\"from\":\"plan\""), "{to_coding}");
    // 阶段转换：testing → coding（回注）
    let back = phases
        .iter()
        .find(|l| l.contains("\"from\":\"testing\""))
        .unwrap_or_else(|| panic!("应有 testing → coding 的阶段转换：{text:?}"));
    assert!(back.contains("\"to\":\"coding\""), "{back}");

    // 修复轮：计划审查打回后的重写 + 测试回注 coding（各一对，含轮次）
    let starts = lines_with(&text, "fix.start");
    let ends = lines_with(&text, "fix.end");
    assert_eq!(starts.len(), 2, "计划修复轮 + 测试回注各一对：{text:?}");
    assert_eq!(ends.len(), 2, "计划修复轮 + 测试回注各一对：{text:?}");
    assert!(
        starts.iter().all(|l| l.contains("\"round\":1")),
        "修复轮要带轮次：{text:?}"
    );
    assert!(
        starts
            .iter()
            .any(|l| l.contains("计划修复轮开始")),
        "应有计划修复轮的 fix.start：{text:?}"
    );
    assert!(
        starts
            .iter()
            .any(|l| l.contains("测试回注 coding 开始")),
        "应有测试回注的 fix.start：{text:?}"
    );
    // 里程碑形状：级别 INFO + 带 component
    for line in lines_with(&text, "\"event\":") {
        assert!(line.contains("\"level\":\"INFO\""), "里程碑必须是 INFO：{line}");
        assert!(line.contains("\"component\":"), "里程碑要带 component：{line}");
    }
}

#[test]
fn each_mvp_and_doc_loop_logs_its_own_fix_pair() {
    // 错实现形态：某个循环的打回/回注不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
    let mut all: Vec<String> = Vec::new();

    // MVP planning：判负回注
    {
        let cap = Capture::new();
        let plan = ScriptedAgent::new(&["好了\n<plan-complete>", "改好了\n<plan-complete>"]);
        let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_FAIL, PLAN_REVIEW_PASS]);
        devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
            .run(&mut NoUser)
            .expect("规划循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "planning 回注：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "planning 回注：{text:?}");
        assert!(
            lines_with(&text, "fix.start")[0].contains("\"round\":1"),
            "planning 回注要带轮次：{text:?}"
        );
        all.extend(text);
    }

    // MVP coding：打回重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&[
            "阶段 1 完成\n<stage-complete>1</stage-complete>",
            "修好了\n<stage-complete>1</stage-complete>",
        ]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("编码循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "coding 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "coding 打回：{text:?}");
        all.extend(text);
    }

    // MVP testing：打回重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&[
            "阶段 1 完成\n<stage-complete>1</stage-complete>",
            "修好了\n<stage-complete>1</stage-complete>",
        ]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::testing::TestingSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("测试循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "testing 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "testing 打回：{text:?}");
        all.extend(text);
    }

    // doc-from-zero：单元审查 FAIL → 回注重写
    {
        let cap = Capture::new();
        let dir = tempfile::tempdir().expect("临时目录");
        let store = StateStore::new(
            dir.path().join("plans"),
            StateKey::new("p", "t", "doc-from-zero"),
        );
        let agent = ScriptedAgent::new(&[
            "单元 1 写完\n<doc-complete>1</doc-complete>",
            "单元 1 改完\n<doc-complete>1</doc-complete>",
        ]);
        let factory = ScriptedFactory::new(&[DOC_REVIEW_FAIL, DOC_REVIEW_PASS]);
        devforge::sop::doc_from_zero::run_pipeline(
            agent,
            std::sync::Arc::new(std::sync::Mutex::new(factory)),
            store,
            &mut NoUser,
            &dir.path().join("repo"),
            &["1".to_string()],
            "none",
            String::new(),
            None,
            None,
        )
        .expect("doc 单元循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "doc 回注：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "doc 回注：{text:?}");
        all.extend(text);
    }

    // doc-fix：审查打回 → 回注重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&["写好了", "改好了"]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::doc_fix::DocFixSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("doc-fix 循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "doc-fix 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "doc-fix 打回：{text:?}");
        all.extend(text);
    }

    // 五个循环各一对
    assert_eq!(
        lines_with(&all, "fix.start").len(),
        5,
        "五个循环各落一条 fix.start：{all:?}"
    );
    assert_eq!(
        lines_with(&all, "fix.end").len(),
        5,
        "五个循环各落一条 fix.end：{all:?}"
    );
}

#[test]
fn the_mvp_planning_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次，首审不是 1）→ 本用例必须变红
    let cap = Capture::new();
    let plan = ScriptedAgent::new(&["好了\n<plan-complete>"]);
    let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_PASS]);
    devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
        .run(&mut NoUser)
        .expect("规划循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"plan-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::planning\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
fn the_mvp_coding_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("编码循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"coding-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::coding\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
fn the_mvp_testing_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::testing::TestingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("测试循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"test-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::testing\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
fn the_doc_from_zero_review_open_round_is_the_bump_review_count() {
    // 错实现形态：doc-from-zero 的开庭轮次不是 bump_review 的 1-based 计数（首审不是 1）
    // → 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let store = StateStore::new(
        dir.path().join("plans"),
        StateKey::new("p", "t", "doc-from-zero"),
    );
    let agent = ScriptedAgent::new(&["单元 1 写完\n<doc-complete>1</doc-complete>"]);
    let factory = ScriptedFactory::new(&[DOC_REVIEW_PASS]);
    devforge::sop::doc_from_zero::run_pipeline(
        agent,
        std::sync::Arc::new(std::sync::Mutex::new(factory)),
        store,
        &mut NoUser,
        &dir.path().join("repo"),
        &["1".to_string()],
        "none",
        String::new(),
        None,
        None,
    )
    .expect("doc 单元循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"doc-review\""), "{opens:?}");
    assert!(
        opens[0].contains("\"round\":1"),
        "轮次 = bump_review 的 1-based 计数（首审 = 1）：{opens:?}"
    );
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc_from_zero\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
fn the_doc_fix_review_open_round_equals_the_review_rounds_counter() {
    // 错实现形态：doc-fix 的开庭轮次写成 rounds + 1（首审记成 2）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["写好了", "改好了"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::doc_fix::DocFixSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("doc-fix 循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 2, "两轮审查各一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"doc-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审 = 1：{opens:?}");
    assert!(opens[1].contains("\"round\":2"), "次审 = 2：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc_fix\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

// ---- change 侧：审查开庭 + 三道机械门 ----

/// change 侧的夹具（目标仓带 git + 黑板证据齐全）。
struct ChangeFixture {
    _dir: tempfile::TempDir,
    ctx: ChangeCtx,
}

fn change_fixture() -> ChangeFixture {
    let dir = tempfile::tempdir().expect("临时目录");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("建仓目录");
    std::fs::write(repo.join("src/a.rs"), "// a\n").expect("写源文件");
    let blackboard = dir.path().join("bb");
    std::fs::create_dir_all(&blackboard).expect("建黑板");
    let change_dir = repo.join("docs/plantree/changes/change-f6");
    std::fs::create_dir_all(&change_dir).expect("建计划目录");
    std::fs::write(
        change_dir.join("change-plan.md"),
        "# 变更计划\n\n## 1 需求复述\n改 a。\n\n## 2 影响面\n- `src/a.rs:1`\n\n\
         ## 3 要改的\n| `src/a.rs` | 需求 |\n\n## 4 白名单\n`src/a.rs`\n\n\
         ## 5 验收标准\n- 给定 x=1 → 返回 2\n\n## 6 风险\n- 无\n",
    )
    .expect("写计划");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "baseline",
        ],
    );
    let baseline = git_out(&repo, &["rev-parse", "HEAD"]);
    // 黑板证据：基线绿 + 自证绿 + 一步实施
    std::fs::create_dir_all(blackboard.join("evidence")).expect("建证据目录");
    std::fs::write(
        blackboard.join("evidence/baseline-test.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .expect("写基线证据");
    std::fs::write(
        blackboard.join("evidence/self-check.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .expect("写自证证据");
    std::fs::write(
        blackboard.join("impl-steps.md"),
        "# 实施步骤\n## 步骤 1 改 a\n",
    )
    .expect("写实施步骤");
    std::fs::write(repo.join("src/a.rs"), "// a changed\n").expect("改源文件");
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "change: change-f6 step 1",
        ],
    );
    ChangeFixture {
        ctx: ChangeCtx {
            repo,
            blackboard,
            change_dir,
            change_id: "change-f6".into(),
            baseline,
            requirement: "把 a 改成 b".into(),
            doc_level: DocLevel::None,
        },
        _dir: dir,
    }
}

fn git(repo: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn the_change_review_open_carries_the_role_and_the_round() {
    // 错实现形态：change 侧审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let f = change_fixture();
    let agent = ScriptedAgent::new(&[
        "基线绿了\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(ScriptedAgent::new(&[])),
        plan_review: Box::new(ScriptedFactory::new(&[])),
        coding_agent: Box::new(agent),
        coding_review: Box::new(factory),
        test_agent: Box::new(ScriptedAgent::new(&[])),
        test_review: Box::new(ScriptedFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    {
        use devforge::change::orchestrator::ChangeSession as _;
        session.run_coding(&mut NoUser, 0, None).expect("coding 段应通过");
    }
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(
        opens[0].contains("\"role\":\"change-coding-review\""),
        "{opens:?}"
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::change::session\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

/// 造一个 change coding 段的 session（脚本化 agent + 脚本化审查门）。
fn coding_session(
    f: &ChangeFixture,
    agent_replies: &[&str],
    review_replies: &[&str],
) -> ProdChangeSession {
    ProdChangeSession {
        plan_agent: Box::new(ScriptedAgent::new(&[])),
        plan_review: Box::new(ScriptedFactory::new(&[])),
        coding_agent: Box::new(ScriptedAgent::new(agent_replies)),
        coding_review: Box::new(ScriptedFactory::new(review_replies)),
        test_agent: Box::new(ScriptedAgent::new(&[])),
        test_review: Box::new(ScriptedFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    }
}

#[test]
fn the_baseline_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①红基线不落门禁里程碑（只落绿路径）②退出码只埋在错误文本里
    //   （按字段过滤看不见「退出码多少」）③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true，退出码按字段带出
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.baseline");
        assert_eq!(gates.len(), 1, "绿基线也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.baseline\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿基线的判定结论要落绿：{line}");
        assert!(
            line.contains("\"exit_code\":0"),
            "绿基线也要按字段带出退出码：{line}"
        );
    }
    // 红：判定结论 passed=false，退出码按字段带出（红时含退出码）
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 基线证据写成红（退出码 1）→ 门卫判红
        std::fs::write(
            f.ctx.blackboard.join("evidence/baseline-test.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .expect("写红基线证据");
        let mut session = coding_session(
            &f,
            &["跑了，红了\n<stage-complete>1</stage-complete>"],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("红基线应拦住 coding 轮");
            assert!(
                err.to_string().contains("基线不是绿的"),
                "红基线的报错应说明基线不绿：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.baseline");
        assert_eq!(gates.len(), 1, "红基线也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.baseline\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
        assert!(
            line.contains("\"exit_code\":1"),
            "红时要把退出码按字段带出来（不能只埋在错误文本里）：{line}"
        );
    }
}

#[test]
fn the_self_check_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①自证门红时不落门禁里程碑（只落绿路径）②红路径缺判定结论
    //   ③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.self_check");
        assert_eq!(gates.len(), 1, "绿自证也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.self_check\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿自证的判定结论要落绿：{line}");
    }
    // 红：判定结论 passed=false
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 自证证据写成红（退出码 1）→ 自证门判红
        std::fs::write(
            f.ctx.blackboard.join("evidence/self-check.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .expect("写红自证证据");
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("红自证应拦住 coding 轮");
            assert!(
                err.to_string().contains("自证不是绿的"),
                "红自证的报错应说明自证没过：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.self_check");
        assert_eq!(gates.len(), 1, "红自证也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.self_check\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
    }
}

#[test]
fn the_step_commits_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①步提交对账不落门禁里程碑（只落绿路径）②缺步骤数
    //   ③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true + 步骤数
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.step_commits");
        assert_eq!(gates.len(), 1, "绿步提交对账也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.step_commits\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿对账的判定结论要落绿：{line}");
        assert!(line.contains("\"steps\":1"), "步提交对账要带步骤数：{line}");
    }
    // 红：判定结论 passed=false + 步骤数
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 实施步骤写两步，但只有一个提交 → 对账判红
        std::fs::write(
            f.ctx.blackboard.join("impl-steps.md"),
            "# 实施步骤\n## 步骤 1 改 a\n## 步骤 2 改 b\n",
        )
        .expect("写两步的实施步骤");
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("步骤与提交对不上应拦住 coding 轮");
            assert!(
                err.to_string().contains("对不上"),
                "对账失败的报错应说明步骤与提交对不上：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.step_commits");
        assert_eq!(gates.len(), 1, "红对账也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.step_commits\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
        assert!(line.contains("\"steps\":2"), "红对账也要带步骤数：{line}");
    }
}

#[test]
fn an_agent_turn_logs_exactly_one_turn_end() {
    // 错实现形态：①交卷不落 agent.turn_end（或缺角色名/耗时）②同一轮落两条
    // （change 侧另落一份）→ 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let script = r#"read line; printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hello"}}'; printf '%s\n' '{"type":"agent_settled"}'; sleep 1"#;
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(script);
    let spec = SpawnSpec::new(
        dir.path().join("profile"),
        "f6-test-agent",
        "run-f6-turn",
        dir.path().join("sessions"),
    );
    let mut client = RpcClient::spawn_with_command(cmd, spec).expect("spawn 假 agent");
    {
        client.prompt("干活").expect("发 prompt");
        let outcome = client.wait_settled().expect("等交卷");
        assert!(outcome.text.contains("hello"), "应收到流式文本");
    }
    let _ = client.kill();
    let text = cap.lines();
    let turns = lines_with(&text, "agent.turn_end");
    assert_eq!(turns.len(), 1, "一轮只落一条交卷事件：{text:?}");
    assert!(
        turns[0].contains("\"role\":\"f6-test-agent\""),
        "要带角色名：{turns:?}"
    );
    assert!(turns[0].contains("turn_ms"), "要带本轮耗时：{turns:?}");
    assert!(turns[0].contains("chars"), "要带产出字符数：{turns:?}");
    // 落点唯一：change 侧不另落交卷事件
    let change_session =
        std::fs::read_to_string(repo_root().join("src/change/session.rs")).expect("读 change 侧源码");
    assert!(
        !change_session.contains("agent.turn_end"),
        "交卷事件落点必须唯一（change 侧不得另落）"
    );
}

#[test]
fn every_milestone_is_info_with_component_event_and_the_emitting_module_target() {
    // 错实现形态：里程碑落成 WARN/ERROR、删掉 component、或所有里程碑共用同一个 target
    // （包装函数）→ 本用例必须变红
    let cap = Capture::new();
    let plan = ScriptedAgent::new(&["好了\n<plan-complete>"]);
    let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_PASS]);
    devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
        .run(&mut NoUser)
        .expect("规划循环应通过");
    let text = cap.lines();
    let milestones = lines_with(&text, "\"event\":");
    assert!(!milestones.is_empty(), "应有里程碑条目：{text:?}");
    for line in milestones {
        assert!(line.contains("\"level\":\"INFO\""), "级别必须是 INFO：{line}");
        assert!(line.contains("\"component\":"), "要有 component：{line}");
        assert!(line.contains("\"event\":"), "要有 event：{line}");
        assert!(line.contains("\"target\":"), "要有 target：{line}");
    }
    // 不同模块的里程碑不共用同一个 target
    let before_coding = cap.lines().len();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("编码循环应通过");
    let coding_text = cap.lines()[before_coding..].to_vec();
    let target_of = |lines: &[&str]| -> String {
        lines[0]
            .split("\"target\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("")
            .to_string()
    };
    let planning_target = target_of(&lines_with(&text, "review.open"));
    let coding_target = target_of(&lines_with(&coding_text, "review.open"));
    assert!(!planning_target.is_empty(), "target 要有值");
    assert!(
        planning_target != coding_target,
        "不同模块的里程碑不共用同一个 target：{planning_target} vs {coding_target}"
    );
    assert!(planning_target.contains("planning"), "{planning_target}");
    assert!(coding_target.contains("coding"), "{coding_target}");
}

// ============================================================ 需求 3：死亡取证（墓碑）

/// panic hook 是进程级全局 → 这组用例串行跑。
fn panic_hook_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn a_panic_leaves_a_tombstone_with_the_text_the_location_and_the_phase() {
    // 错实现形态：panic 后没有墓碑 / 墓碑缺 panic 原文、位置或阶段 → 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-tombstone";
    obs::begin_run_in(root.clone(), run_id, "coding");
    obs::set_step("review:change-coding-review");
    let original = std::panic::take_hook();
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-panic-text");
    });
    handle.restore();
    std::panic::set_hook(original);
    obs::end_run(run_id);
    assert!(result.is_err(), "panic 应被捕获（不 abort）");
    let tombs: Vec<PathBuf> = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(tombs.len(), 1, "应恰好一个墓碑文件：{tombs:?}");
    assert!(
        tombs[0].file_name().unwrap().to_string_lossy().contains(run_id),
        "墓碑文件名要含 run id：{tombs:?}"
    );
    let text = std::fs::read_to_string(&tombs[0]).expect("读墓碑");
    assert!(text.contains("f6-boom-panic-text"), "墓碑要含 panic 原文：{text}");
    assert!(text.contains("location: "), "墓碑要含位置行：{text}");
    assert!(
        text.contains("f6_observability.rs:"),
        "位置要指向 panic 现场（file:line）：{text}"
    );
    assert!(text.contains("phase: coding"), "墓碑要含当时阶段：{text}");
    assert!(
        text.contains("step: review:change-coding-review"),
        "墓碑要含当时步骤：{text}"
    );
}

#[test]
fn a_panic_without_an_active_run_still_leaves_a_tombstone() {
    // 错实现形态：无活动 run 时不写墓碑（证据丢失）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let original = std::panic::take_hook();
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-no-run");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err());
    let tombs: Vec<PathBuf> = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(tombs.len(), 1, "无活动 run 也要留墓碑：{tombs:?}");
    let text = std::fs::read_to_string(&tombs[0]).expect("读墓碑");
    assert!(
        text.contains(&format!("run_id: {}", obs::tombstone::NO_RUN)),
        "无活动 run 时用占位 run id：{text}"
    );
    assert!(text.contains("f6-boom-no-run"), "墓碑要含 panic 原文：{text}");
}

#[test]
fn the_previous_panic_hook_is_still_called() {
    // 错实现形态：装了新 hook 就把上一个 hook 顶掉（默认 stderr 输出丢）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let original = std::panic::take_hook();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    std::panic::set_hook(Box::new(move |_| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    let handle = obs::tombstone::install_hook(root.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-chain");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err());
    assert!(
        called.load(std::sync::atomic::Ordering::SeqCst),
        "上一个 hook 必须被链回调用（链回语义）"
    );
    let tombs = std::fs::read_dir(root.join("tombstones"))
        .expect("墓碑目录应存在")
        .count();
    assert_eq!(tombs, 1, "链回的同时也要写墓碑");
}

#[test]
fn an_unwritable_root_does_not_abort_and_still_chains_the_hook() {
    // 错实现形态：写不进去时在 hook 里 panic（= abort，连 stderr 链回都丢）→ 本用例必须变红
    let _guard = panic_hook_lock().lock().expect("锁");
    let dir = tempfile::tempdir().expect("临时目录");
    let not_a_dir = dir.path().join("not-a-dir");
    std::fs::write(&not_a_dir, b"x").expect("造一个不可写的落点");
    let original = std::panic::take_hook();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    std::panic::set_hook(Box::new(move |_| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    let handle = obs::tombstone::install_hook(not_a_dir.clone());
    let result = std::panic::catch_unwind(|| {
        panic!("f6-boom-unwritable");
    });
    handle.restore();
    std::panic::set_hook(original);
    assert!(result.is_err(), "panic 仍应正常传播（不被 hook 吞掉）");
    assert!(
        called.load(std::sync::atomic::Ordering::SeqCst),
        "写失败也必须链回上一个 hook"
    );
    assert!(
        !not_a_dir.join("tombstones").exists(),
        "写不进去时不应留下半成品目录"
    );
}

// ============================================================ 需求 3：run 心跳

#[test]
fn the_heartbeat_follows_the_phase_and_carries_phase_step_and_timestamp() {
    // 错实现形态：阶段推进不更新心跳（文件内容不变）→ 死后答不出「死在哪个阶段」
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-heartbeat";
    obs::begin_run_in(root.clone(), run_id, "plan");
    let before = obs::heartbeat::read(&root, run_id).expect("心跳应已落盘");
    obs::set_phase("coding");
    obs::set_step("fix:plan:1");
    let after = obs::heartbeat::read(&root, run_id).expect("心跳应仍在");
    obs::end_run(run_id);
    assert_eq!(before.phase, "plan");
    assert_eq!(after.phase, "coding", "阶段推进后心跳内容必须随之变化");
    assert_eq!(after.step, "fix:plan:1", "心跳要带步骤");
    assert!(after.timestamp_ms > 0, "心跳要带时间戳");
    assert_eq!(after.run_id, run_id);
}

#[test]
fn the_heartbeat_is_refreshed_by_the_watchdog_for_every_sop_including_change() {
    // 错实现形态：①低频兜底不刷新心跳（长时间无事件时心跳陈旧 → 「卡死」与「已死」分不清）
    //   ②某个 SOP 的入口不经过 watchdog（长 agent 轮里心跳陈旧）
    //   ③watchdog 线程不刷心跳 → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-touch";
    obs::begin_run_in(root.clone(), run_id, "plan");
    let before = obs::heartbeat::read(&root, run_id).expect("心跳应已落盘");
    std::thread::sleep(Duration::from_millis(1100));
    obs::touch_run(run_id);
    let after = obs::heartbeat::read(&root, run_id).expect("心跳应仍在");
    obs::end_run(run_id);
    assert!(
        after.timestamp_ms > before.timestamp_ms,
        "低频兜底刷新生效：before={} after={}",
        before.timestamp_ms,
        after.timestamp_ms
    );
    assert_eq!(after.phase, "plan", "兜底刷新不改阶段");

    // 兜底刷新要覆盖**六个 SOP**（change 也不例外）
    let server = std::fs::read_to_string(repo_root().join("src/server/mod.rs")).expect("读 server");
    let change =
        std::fs::read_to_string(repo_root().join("src/server/change_engine.rs")).expect("读引擎");
    for (name, text) in [
        ("fn run_planning", server.as_str()),
        ("fn run_coding", server.as_str()),
        ("fn run_doc_from_zero", server.as_str()),
        ("fn run_testing", server.as_str()),
        ("fn run_doc_fix", server.as_str()),
        ("fn run_change", change.as_str()),
    ] {
        let body = fn_body(text, name)
            .unwrap_or_else(|| panic!("源码里应能找到 {name}：心跳兜底要覆盖到它"));
        assert!(
            body.contains("run_with_watchdog("),
            "{name} 必须经过 watchdog（否则长 agent 轮里心跳陈旧）：{body}"
        );
    }
    // watchdog 线程本身就是心跳低频兜底的挂载点
    let watchdog = fn_body(&server, "fn run_with_watchdog")
        .expect("源码里应能找到 run_with_watchdog（watchdog 是兜底刷新的挂载点）");
    assert!(
        watchdog.contains("touch_run("),
        "watchdog 线程要刷心跳（区分「卡死」与「已死」）：{watchdog}"
    );
}

/// 从源码里取某个函数的函数体（按花括号配平）。
fn fn_body(text: &str, name: &str) -> Option<String> {
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(name) {
        let at = from + pos;
        let prev = text[..at].chars().next_back().unwrap_or(' ');
        let prev_ok = !prev.is_alphanumeric() && prev != '_';
        let after = text[at + name.len()..].chars().next();
        if prev_ok && matches!(after, Some('(') | Some('<')) {
            let open = text[at..].find('{')? + at;
            let mut depth = 0i32;
            for (i, c) in text[open..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(text[open..open + i + 1].to_string());
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }
        from = at + name.len();
    }
    None
}

#[test]
fn the_heartbeat_answers_which_phase_the_process_died_in() {
    // 错实现形态：进程在两阶段之间异常死亡后心跳被删 / 不含最后阶段 → 答不出「死在哪」
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-death";
    obs::begin_run_in(root.clone(), run_id, "plan");
    obs::set_phase("coding");
    // 模拟异常死亡：不清位（没有 end_run），也不删文件
    let hb = obs::heartbeat::read(&root, run_id).expect("心跳应能回答死在哪个阶段");
    assert_eq!(hb.phase, "coding", "心跳要停在死亡前最后到达的阶段");
    obs::end_run(run_id);
}

#[test]
fn the_heartbeat_and_tombstone_land_in_fixed_subdirectories_named_by_the_run_id() {
    // 错实现形态：落点目录名不对（不在 heartbeat/ 与 tombstones/ 下）或文件名不含 run id
    // → 本用例必须变红
    let hb = obs::heartbeat::heartbeat_path(Path::new("/root"), "run-9");
    assert_eq!(hb, PathBuf::from("/root/heartbeat/run-9.json"));
    let tomb = obs::tombstone::tombstone_path(Path::new("/root"), 1_700_000_000_000, "run-9");
    assert_eq!(tomb, PathBuf::from("/root/tombstones/1700000000000-run-9.txt"));
}

#[tokio::test]
async fn the_server_run_writes_the_heartbeat_into_the_fixture_dir_not_the_user_home() {
    // 错实现形态：仓内走真实 start 路径的 run 把心跳写进用户 HOME（用户 HOME 被写脏）
    // → 本用例必须变红
    let _cap = Capture::new();
    let home_heartbeat = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".devforge").join("heartbeat"));
    let before = home_heartbeat
        .as_ref()
        .map(|d| std::fs::read_dir(d).map(|it| it.count()).unwrap_or(0))
        .unwrap_or(0);
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "planning").await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t1").await;
    // 心跳落在夹具的临时目录里，且 run 结束后**保留**最后一次状态
    // （F18 后首写异步化：有界轮询等落盘，不假设 Settled 瞬间文件已在）
    let hb_dir = f.obs_root.join("heartbeat");
    let mut files: Vec<PathBuf> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        files = std::fs::read_dir(&hb_dir)
            .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        if !files.is_empty() || std::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(files.len(), 1, "run 结束后心跳应保留（不删）：{files:?}");
    assert!(
        files[0].file_name().unwrap().to_string_lossy().ends_with(".json"),
        "心跳文件名带 run id：{files:?}"
    );
    // 用户 HOME 的 .devforge/heartbeat 不新增文件
    if let Some(dir) = &home_heartbeat {
        let after = std::fs::read_dir(dir).map(|it| it.count()).unwrap_or(0);
        assert_eq!(
            after, before,
            "用户 HOME 的 .devforge/heartbeat/ 不该被仓内单测写脏：{dir:?}"
        );
    }
}

// ============================================================ 需求 4 / 边界口径

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const CHANGE_ID: &str = "change-20261022-c7-1-start-src-server-tests-rs-145-fixtu";
const PLAN_REL: &str = "docs/plantree/changes/change-20261022-c7-1-start-src-server-tests-rs-145-fixtu/change-plan.md";
const TEST_STAGE_FILE: &str = "tests/f6_observability.rs";

/// 计划 §4 白名单（从计划文件里抽反引号列表项 → 数据驱动，不另抄一份）。
fn plan_whitelist() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join(PLAN_REL)).expect("读计划");
    let section = text
        .split("## 4 白名单")
        .nth(1)
        .expect("计划里应有「## 4 白名单」")
        .split("\n## ")
        .next()
        .expect("白名单小节");
    let mut out = Vec::new();
    for line in section.lines() {
        let t = line.trim();
        if !t.starts_with("- `") {
            continue;
        }
        let Some(rest) = t.strip_prefix("- `") else {
            continue;
        };
        let Some(end) = rest.find('`') else { continue };
        out.push(rest[..end].to_string());
    }
    out
}

/// 从测试文件里抽 `const NAME: &[&str] = &[ ... ];` 的条目。
fn const_list(text: &str, name: &str) -> Vec<String> {
    let start = text
        .find(&format!("const {name}:"))
        .unwrap_or_else(|| panic!("文件里应有常量 {name}"));
    let body = &text[start..];
    let open = body
        .find("= &[")
        .map(|i| i + 4)
        .unwrap_or_else(|| panic!("常量 {name} 应是数组"));
    let close = body[open..]
        .find("];")
        .map(|i| i + open)
        .expect("数组有结尾");
    body[open..close]
        .lines()
        // 注释行不是常量条目（守卫常量里允许写注释说明）→ 跳过，否则解析结果会把
        // 注释当成白名单文件，失败信息失真
        .filter(|l| !l.trim().starts_with("//"))
        .map(|l| l.trim().trim_matches(',').trim().trim_matches('"').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// 从测试文件里抽 `const NAME: &str = "..."` 的值。
fn const_str(text: &str, name: &str) -> String {
    let marker = format!("const {name}: &str = \"");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("文件里应有常量 {name}"));
    let rest = &text[start + marker.len()..];
    let end = rest.find('"').expect("字符串有结尾");
    rest[..end].to_string()
}

/// 本场基线（黑板口径：`docs/plantree/changes/<change-id>/baseline.txt`）。
fn baseline_of_record() -> String {
    std::fs::read_to_string(
        repo_root().join(format!("docs/plantree/changes/{CHANGE_ID}/baseline.txt")),
    )
    .expect("读基线记录")
    .trim()
    .to_string()
}

#[test]
fn the_guard_constants_point_at_this_change() {
    // 错实现形态：守卫常量没刷新（BASELINE 还指着上一场 / WHITELIST 少文件或多文件）
    // → 本用例必须变红
    let baseline = baseline_of_record();
    assert!(!baseline.is_empty(), "基线记录不该为空");
    let plan = plan_whitelist();
    let coding: Vec<String> = plan
        .iter()
        .filter(|p| *p != TEST_STAGE_FILE)
        .cloned()
        .collect();
    assert_eq!(plan.len(), 24, "计划 §4 白名单 = 24 个文件：{plan:?}");
    assert_eq!(coding.len(), 23, "coding 步白名单 = 23 个文件：{coding:?}");

    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("读守卫文件");
    let failure = std::fs::read_to_string(repo_root().join("tests/change_failure_context.rs"))
        .expect("读验收文件");
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol.rs"))
        .expect("读验收文件");

    for (name, text) in [
        ("守卫", guard.as_str()),
        ("失败上下文验收", failure.as_str()),
        ("协议验收", protocol.as_str()),
    ] {
        assert_eq!(
            const_str(text, "BASELINE"),
            baseline,
            "{name} 的 BASELINE 必须等于本场基线 sha"
        );
    }
    // 守卫的 WHITELIST = coding 步白名单（23 个，逐个列出）
    let listed = const_list(&guard, "WHITELIST");
    assert_eq!(listed.len(), 23, "守卫 WHITELIST 必须有 23 个文件：{listed:?}");
    for f in &coding {
        assert!(listed.iter().any(|x| x == f), "守卫 WHITELIST 缺 {f}");
    }
    // 三份副本逐字一致
    assert_eq!(
        const_list(&failure, "WHITELIST"),
        listed,
        "两份 WHITELIST 常量必须逐字一致"
    );
    assert_eq!(
        const_list(&protocol, "CODING_WHITELIST"),
        listed,
        "上一场验收文件的 CODING_WHITELIST 必须逐字一致"
    );
}

/// 取某个声明**正上方**的文档注释块（连续的 `///` 行）。
fn doc_comment_above(text: &str, marker: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with(marker) {
            break;
        }
        if line.trim_start().starts_with("///") {
            out.push(line.trim_start().trim_start_matches("///").trim());
        } else if !line.trim().is_empty() {
            out.clear(); // 非注释行 → 注释块断开
        }
    }
    out.join("\n")
}

#[test]
fn the_guard_comments_match_their_constant_values() {
    // 错实现形态：注释里留着陈旧的基线 sha / 白名单个数（读者按注释复核会数出旧值）
    //   → 本用例必须变红（R-4）
    let baseline = baseline_of_record();
    let short = &baseline[..7];
    for rel in [
        "tests/change_ask_ring.rs",
        "tests/change_failure_context.rs",
        "tests/change_review_protocol.rs",
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        for stale in ["18ff755", "22 个", "a0be947"] {
            assert!(
                !text.contains(stale),
                "{rel} 残留陈旧口径「{stale}」（注释要与常量的实际值一致）"
            );
        }
        let baseline_comment = doc_comment_above(&text, "const BASELINE");
        assert!(
            baseline_comment.contains(short),
            "{rel} 的 BASELINE 注释要写本场基线 {short}：{baseline_comment}"
        );
    }
    // 白名单常量的注释要写白名单个数（coding 步 23 个 / 计划 24 个）
    for (rel, marker, needle) in [
        ("tests/change_ask_ring.rs", "const WHITELIST", "23 个"),
        (
            "tests/change_failure_context.rs",
            "const WHITELIST",
            "23 个",
        ),
        (
            "tests/change_review_protocol.rs",
            "const CODING_WHITELIST",
            "23 个",
        ),
        (
            "tests/change_review_protocol.rs",
            "const PLAN_WHITELIST",
            "24 个",
        ),
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        let comment = doc_comment_above(&text, marker);
        assert!(
            comment.contains(needle),
            "{rel} 的 {marker} 注释要写「{needle}」：{comment}"
        );
    }
}

#[test]
fn the_test_stage_file_is_excluded_from_the_coding_step_whitelist() {
    // 错实现形态：测试阶段文件被塞进 coding 步白名单常量（两场之间恒红的来源）
    // → 本用例必须变红
    let plan = plan_whitelist();
    assert!(
        plan.iter().any(|p| p == TEST_STAGE_FILE),
        "计划白名单要含唯一的测试阶段文件"
    );
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("读守卫文件");
    let listed = const_list(&guard, "WHITELIST");
    assert!(
        !listed.iter().any(|x| x == TEST_STAGE_FILE),
        "测试阶段文件不得进 coding 步白名单常量：{listed:?}"
    );
    // a29 的排除项断言写的就是这个测试阶段文件
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol.rs"))
        .expect("读验收文件");
    assert!(
        protocol.contains(TEST_STAGE_FILE),
        "a29 的排除项断言要指向本场测试阶段文件"
    );
}

#[test]
fn the_plan_whitelist_has_twenty_four_files_and_the_coding_whitelist_twenty_three() {
    // 错实现形态：白名单数目写错（22/25）或测试阶段文件缺失 → 本用例必须变红
    // （10-22 操作员补录：§4 扩容 +1（coding_gate.rs）后应为 24/23）
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol.rs"))
        .expect("读验收文件");
    let plan_list = const_list(&protocol, "PLAN_WHITELIST");
    let coding_list = const_list(&protocol, "CODING_WHITELIST");
    assert_eq!(plan_list.len(), 24, "计划白名单 = 24 个文件：{plan_list:?}");
    assert_eq!(coding_list.len(), 23, "coding 步白名单 = 23 个文件：{coding_list:?}");
    assert_eq!(
        plan_list,
        plan_whitelist(),
        "验收文件里的计划白名单必须与计划 §4 一致"
    );
    assert!(
        plan_list.iter().any(|f| f == TEST_STAGE_FILE),
        "计划白名单要含唯一的测试阶段文件"
    );
    assert_eq!(
        plan_list
            .iter()
            .filter(|f| *f != TEST_STAGE_FILE)
            .cloned()
            .collect::<Vec<String>>(),
        coding_list,
        "coding 白名单 = 计划白名单去掉测试阶段文件"
    );
}

#[test]
fn the_guard_keeps_its_ignore_between_two_changes() {
    // 错实现形态：两场之间的守卫被摘掉 ignore（守卫恒红 → 变成噪音）或 ignore 被抹掉
    // → 本用例必须变红
    for rel in [
        "tests/change_ask_ring.rs",
        "tests/change_failure_context.rs",
        "tests/change_review_protocol.rs",
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        assert!(
            text.contains("#[ignore"),
            "{rel} 的守卫应保持 ignore（两场之间无 step 提交）"
        );
    }
}

#[test]
fn the_decision_ledger_has_the_d006_section() {
    // 错实现形态：D006 缺三件交付物 / 缺渲染闸门口径 / 缺分级口径，或 D001–D005 被改动
    // → 本用例必须变红
    let rel = "docs/plantree/plans/010-run-resume/decisions/README.md";
    let current = std::fs::read_to_string(repo_root().join(rel)).expect("读决策台账");
    assert!(current.contains("## D006 ·"), "决策台账要有 D006 一节");
    for needle in [
        "原始错误",
        "里程碑",
        "墓碑",
        "心跳",
        "渲染闸门",
        "ERROR",
        "WARN",
        "配置错误",
    ] {
        assert!(
            current.contains(needle),
            "D006 要写明「{needle}」这一口径"
        );
    }
    // 既有 D001–D005 一字不动（与本变更基线版本逐字比对）
    let before_d006 = current
        .split("## D006 ·")
        .next()
        .expect("应有 D006 之前的部分")
        .trim_end()
        .to_string();
    let baseline_version = git_out(
        &repo_root(),
        &[
            "show",
            &format!(
                "{}:docs/plantree/plans/010-run-resume/decisions/README.md",
                baseline_of_record()
            ),
        ],
    );
    assert_eq!(
        before_d006,
        baseline_version.trim_end(),
        "既有 D001–D005 条目一字不动"
    );
}

#[test]
fn the_plan_audit_has_no_findings() {
    // 错实现形态：计划里出现别的机械门 finding（六块不全 / path:line 不存在 / 白名单外改动 /
    //   「白名单里的文件不存在」）→ 本用例必须变红
    // （计划期预期的 4 条「白名单里的文件不存在」已随四个新增文件落盘消解 → 现状必须是 0 条）
    let plan = std::fs::read_to_string(repo_root().join(PLAN_REL)).expect("读计划");
    let audit = devforge::change::plan_audit::audit(&plan, &repo_root());
    assert!(
        audit.issues.is_empty(),
        "机械门审计的 finding 必须为 0 条：{audit:?}"
    );
    assert!(audit.missing_blocks.is_empty(), "六块必须齐全：{audit:?}");
}

#[test]
#[ignore = "F10：守卫随场次烙死——c7b 已 Settled，基线后移后本守卫仅在下一场 run 内生效（本注释代 ignore 复贴，10-23 收官）"]
fn the_commits_stay_inside_the_plan_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来
    // → 本用例必须变红
    let baseline = baseline_of_record();
    let plan = plan_whitelist();
    let changed = git_out(
        &repo_root(),
        &["diff", "--name-only", &format!("{baseline}..HEAD")],
    );
    for f in changed.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            plan.iter().any(|p| p == f),
            "提交内容越界（计划 §4 白名单外）：{f}"
        );
    }
    for artifact in [
        ".pi/goals/",
        "tests/e2e/results/",
        "docs/plantree/changes/",
    ] {
        let touched = git_out(
            &repo_root(),
            &[
                "log",
                "--format=%H",
                "--name-only",
                &format!("{baseline}..HEAD"),
                "--",
                artifact,
            ],
        );
        assert!(
            touched.trim().is_empty(),
            "未跟踪产物 {artifact} 被提交进来了：{touched}"
        );
    }
}
