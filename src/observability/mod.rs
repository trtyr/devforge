//! 可观测性（plan 010 · F6 靶）：run 上下文 + 观测根解析 + 心跳/墓碑的公共面。
//!
//! 三件交付物共用这一层的词汇：
//! - **run 上下文**（run id / 阶段 / 步骤 / attempt）：当前线程的 run 归属 + 进程级的快照表。
//!   MVP 是串行执行（同一时刻只有一个 run），故线程级归属足够；快照表让**别的线程**
//!   （watchdog 线程、panic hook）也能读到「当前阶段」。
//! - **观测根**（root）：心跳与墓碑都落它下面。解析顺序（Q4）：
//!   **进程内注入**（单测用）> `DEVFORGE_HOME` > `HOME/.devforge`（生产默认）。
//! - **里程碑字段**：由各调用点**内联**写 `tracing::info!`——`tracing` 的 `target` 必须是
//!   发出它的模块，包装函数会让所有里程碑共用同一个 target，「按 target 过滤」这条需求直接作废。
//!
//! **阶段 / 步骤的词汇**（R24：心跳与里程碑必须同一套值，否则事后对不上号）：
//! - `phase` = 阶段。change SOP 用 `ChangePhase` 的变体名（plan/coding/testing/done，由编排层写）；
//!   其余 SOP 用自己的阶段标识（planning=plan、coding=coding、testing=test、doc-from-zero=doc、doc-fix=doc-fix，
//!   由服务端在 run 开始时写）。
//! - `step` = 阶段内更细的一格（`review:<角色名>` / `fix:<种类>:<轮次>`）——只有**知道 run 归属**的调用点写得动
//!   （change 侧的编排层与会话层；MVP/doc 循环拿不到 run id → 它们只落里程碑，不动心跳）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::error::DevforgeError;

/// 一次 run 的当前观测状态（心跳与墓碑的「当时是谁、在哪个阶段」）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSnapshot {
    /// run id（心跳/墓碑的文件名用它；无活动 run 时墓碑用占位值）。
    pub run_id: String,
    /// 阶段（change = plan/coding/testing/done；其他 SOP = sop 名）。
    pub phase: String,
    /// 步骤（阶段内更细的一格：审查角色、修复轮次……）。
    pub step: String,
    /// 当前尝试次数（run 级重试的 attempt；1 = 首次尝试）。
    pub attempt: u32,
}

/// 进程级的 run 快照表（run id → 快照）。
///
/// 为什么要一张表：watchdog 线程与 panic hook 在**别的线程**上跑，
/// 它们要读「这次 run 走到哪个阶段了」——线程级上下文看不到，得从表里取。
static RUNS: OnceLock<Mutex<HashMap<String, RunSnapshot>>> = OnceLock::new();
fn runs() -> &'static Mutex<HashMap<String, RunSnapshot>> {
    RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 当前线程的 run 归属。
#[derive(Debug, Default)]
struct LocalState {
    /// 当前线程正在跑的 run id（无 = 不在这个 run 里）。
    run_id: Option<String>,
    /// 当前线程的尝试次数（重试层与 `map_intervention` 同线程 → 读到的是同一个值）。
    attempt: u32,
    /// 角色内审查轮次计数（key = 角色名）。
    review_rounds: HashMap<String, u32>,
}

thread_local! {
    static LOCAL: RefCell<LocalState> = RefCell::new(LocalState::new());
}

impl LocalState {
    fn new() -> Self {
        Self {
            run_id: None,
            attempt: 0,
            review_rounds: HashMap::new(),
        }
    }
}

/// 置位 run 上下文（run 开始，在 run 线程上调用）：登记快照并清零角色内审查轮次计数。
///
/// 观测根在**此刻捕获**（见 [`begin_run_in`]）→ 本次 run 的证据落点固定，不因进程级注入漂移。
pub fn begin_run(run_id: &str, phase: &str) {
    begin_run_in(root(), run_id, phase);
}

/// 置位 run 上下文并**指定**观测根（本次 run 的心跳落点）。
///
/// 服务端按 run 注入（单测把落点钉在自己的临时目录 → 不落用户 HOME，R13）；
/// 进程级注入（[`set_root`]）只是**默认**来源。
pub fn begin_run_in(root: PathBuf, run_id: &str, phase: &str) {
    let snap = RunSnapshot {
        run_id: run_id.to_string(),
        phase: phase.to_string(),
        step: String::new(),
        attempt: 1,
    };
    runs().lock().expect("observability 快照表锁").insert(run_id.to_string(), snap);
    roots()
        .lock()
        .expect("observability 根表锁")
        .insert(run_id.to_string(), root);
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        l.run_id = Some(run_id.to_string());
        l.attempt = 1;
        l.review_rounds.clear();
    });
    write_heartbeat(run_id);
}

/// 各 run 的观测根（run 开始时捕获；键 = run id）。
static ROOTS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

fn roots() -> &'static Mutex<HashMap<String, PathBuf>> {
    ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 某 run 的观测根（未登记 = 该 run 不存在）。
fn run_root(run_id: &str) -> Option<PathBuf> {
    roots().lock().expect("observability 根表锁").get(run_id).cloned()
}

/// 清位 run 上下文（run 结束）：**心跳文件保留**最后一次状态（不删——那是死亡现场的证据）。
pub fn end_run(run_id: &str) {
    runs().lock().expect("observability 快照表锁").remove(run_id);
    roots().lock().expect("observability 根表锁").remove(run_id);
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        if l.run_id.as_deref() == Some(run_id) {
            l.run_id = None;
        }
    });
}

/// 更新阶段（编排层阶段转换时调用；顺带刷心跳）。
///
/// **线程级归属**：调用点在 run 线程上（与 [`begin_run`] 同一线程）→ 不用传 run id；
/// 跨线程读（watchdog 线程 / panic hook）走进程级快照表。
pub fn set_phase(phase: &str) {
    let Some(run_id) = current_run_id() else {
        return;
    };
    {
        let mut table = runs().lock().expect("observability 快照表锁");
        let Some(snap) = table.get_mut(&run_id) else {
            return;
        };
        snap.phase = phase.to_string();
    }
    write_heartbeat(&run_id);
}

/// 更新步骤（阶段内更细的一格：审查角色、修复轮次……）。
///
/// 线程级归属（同 [`set_phase`]）：调用点与 run 同线程 → 从线程的 run 归属取快照。
pub fn set_step(step: &str) {
    let Some(run_id) = current_run_id() else {
        return;
    };
    {
        let mut table = runs().lock().expect("observability 快照表锁");
        let Some(snap) = table.get_mut(&run_id) else {
            return;
        };
        snap.step = step.to_string();
    }
    write_heartbeat(&run_id);
}

/// 当前线程的 run id（无 = 不在这个 run 里）。
fn current_run_id() -> Option<String> {
    LOCAL.with(|l| l.borrow().run_id.clone())
}

/// 更新尝试次数（重试层每次尝试更新；`map_intervention` 从同一线程读同一个值 → 两处字段同构）。
pub fn set_attempt(attempt: u32) {
    LOCAL.with(|l| l.borrow_mut().attempt = attempt);
    let run_id = LOCAL.with(|l| l.borrow().run_id.clone());
    if let Some(id) = run_id {
        let mut table = runs().lock().expect("observability 快照表锁");
        if let Some(snap) = table.get_mut(&id) {
            snap.attempt = attempt;
        }
    }
}

/// 当前线程的尝试次数（无 run 上下文 = 0）。
pub fn attempt() -> u32 {
    LOCAL.with(|l| l.borrow().attempt)
}

/// 当前 run 快照（无活动 run = `None`）。
pub fn current() -> Option<RunSnapshot> {
    let id = LOCAL.with(|l| l.borrow().run_id.clone())?;
    runs().lock().expect("observability 快照表锁").get(&id).cloned()
}

/// 角色内审查轮次（1-based）：同一角色在本次 run 内第几次开庭。
///
/// change 侧的 `review_once` 恒传 `base_round=0`（共享门返回的实例数是**返回值**，调用前拿不到），
/// 故按角色计数（R21）。
pub fn next_review_round(role: &str) -> u32 {
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        let counter = l.review_rounds.entry(role.to_string()).or_insert(0);
        *counter += 1;
        *counter
    })
}

/// 低频兜底刷新的节流间隔（毫秒）：watchdog 每 300ms 一跳，心跳不必跟着每跳写盘。
pub const HEARTBEAT_REFRESH_MS: u64 = 1_000;

/// 各 run 的上次刷心跳时刻（节流用）。
static LAST_TOUCH: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

fn last_touch() -> &'static Mutex<HashMap<String, u64>> {
    LAST_TOUCH.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 写心跳（无该 run 的快照时不写）。
pub fn write_heartbeat(run_id: &str) {
    let Some(snap) = runs()
        .lock()
        .expect("observability 快照表锁")
        .get(run_id)
        .cloned()
    else {
        return;
    };
    // 落点 = 本 run 开始时捕获的观测根（不随进程级注入漂移）
    let Some(root) = run_root(run_id) else {
        return;
    };
    if let Err(e) = heartbeat::write(&root, &snap) {
        // 刷心跳失败不打断 run（best-effort）：只在 debug 里留痕
        tracing::debug!(component = "observability", error = %e, "心跳写入失败");
    }
}

/// 低频兜底刷新（watchdog 调用）：节流到 [`HEARTBEAT_REFRESH_MS`] 一次
/// ——阶段/步骤变化走 `set_phase`/`set_step`，不受节流。
pub fn touch_run(run_id: &str) {
    let now = crate::domain::now_ms();
    {
        let mut table = last_touch().lock().expect("observability 节流锁");
        let last = table.get(run_id).copied().unwrap_or(0);
        if now.saturating_sub(last) < HEARTBEAT_REFRESH_MS {
            return;
        }
        table.insert(run_id.to_string(), now);
    }
    write_heartbeat(run_id);
}

/// 注入观测根（单测用；生产由 `DEVFORGE_HOME` / `HOME/.devforge` 解析）。
pub fn set_root(root: impl Into<PathBuf>) {
    let mut slot = root_slot().lock().expect("observability 根锁");
    *slot = Some(root.into());
}

fn root_slot() -> &'static Mutex<Option<PathBuf>> {
    static SLOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 解析观测根：进程内注入 > `DEVFORGE_HOME` > `HOME/.devforge`。
pub fn root() -> PathBuf {
    if let Some(r) = root_slot().lock().expect("observability 根锁").clone() {
        return r;
    }
    resolve_root(
        std::env::var("DEVFORGE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// 观测根的解析（纯函数，便于单测；空串视作未设）。
fn resolve_root(devforge_home: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(d) = devforge_home.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(h) = home.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(h).join(".devforge");
    }
    PathBuf::from(".devforge")
}

/// 错误链：顶层 + source 链每一层，`→` 连接。
///
/// 质量失败（`DevforgeError::Quality`）没有 `#[source]` → 链里只有顶层一段
/// （字段存在、值不为空，Q2）。
pub fn error_chain(e: &DevforgeError) -> String {
    let mut parts = vec![e.to_string()];
    let mut cur: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(src) = cur {
        parts.push(src.to_string());
        cur = src.source();
    }
    parts.join(" → ")
}

/// 心跳文件（run 活性证据）。
pub mod heartbeat;
/// 死亡取证（panic 墓碑）。
pub mod tombstone;

/// 测试支撑：把一段闭包里的 tracing 日志收成 JSONL 全文（行为断言核对「日志面」用）。
///
/// **为什么用全局订阅器 + 线程级缓冲**：`tracing` 的 callsite 兴趣是**全局缓存**的
/// （第一次求值时定型，之后不再变）。用线程级 `with_default` 捕获时，别的用例先跑过
/// 同一个 callsite 就会把它缓存成「永不记录」→ 捕获器随机收不到（并行测试下必现）。
/// 故：全局装一个订阅器（所有 callsite 恒为 interested），日志按**线程**分流进各自的缓冲，
/// 捕获时只取本线程的缓冲 → 既不受别的用例干扰，也不串味。
#[cfg(test)]
pub mod test_support {
    use std::cell::RefCell;
    use std::sync::Once;

    thread_local! {
        /// 本线程的日志缓冲（JSONL 文本）。
        static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    }

    /// 写进本线程缓冲的 writer（全局订阅器的落盘口）。
    struct ThreadBufferWriter;

    impl std::io::Write for ThreadBufferWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            BUFFER.with(|b| b.borrow_mut().push_str(&String::from_utf8_lossy(buf)));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadBufferWriter {
        type Writer = ThreadBufferWriter;
        fn make_writer(&'a self) -> Self::Writer {
            ThreadBufferWriter
        }
    }

    /// 装全局订阅器（进程内一次；callsite 恒为 interested → 不会因别的用例先跑而变死）。
    pub fn install() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_writer(ThreadBufferWriter)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
    }

    /// 跑一段闭包并返回**本线程**的 JSONL 日志全文。
    pub fn capture_jsonl(f: impl FnOnce()) -> String {
        install();
        BUFFER.with(|b| b.borrow_mut().clear());
        f();
        BUFFER.with(|b| b.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Context as _;

    #[test]
    fn root_prefers_the_injected_value() {
        let dir = tempfile::tempdir().unwrap();
        set_root(dir.path());
        assert_eq!(root(), dir.path());
    }

    #[test]
    fn resolve_root_prefers_devforge_home_then_home() {
        // 纯函数口径：DEVFORGE_HOME > HOME/.devforge；空串视作未设
        assert_eq!(
            resolve_root(Some("/x/devforge"), Some("/home/u")),
            PathBuf::from("/x/devforge")
        );
        assert_eq!(
            resolve_root(None, Some("/home/u")),
            PathBuf::from("/home/u/.devforge")
        );
        assert_eq!(
            resolve_root(Some("  "), Some("/home/u")),
            PathBuf::from("/home/u/.devforge")
        );
        assert_eq!(resolve_root(None, None), PathBuf::from(".devforge"));
    }

    #[test]
    fn run_context_carries_phase_step_and_attempt() {
        let id = "run-ctx-1";
        let dir = tempfile::tempdir().unwrap();
        begin_run_in(dir.path().to_path_buf(), id, "coding");
        set_phase("testing");
        set_step("review:change-plan-review");
        set_attempt(3);
        let snap = current().expect("run 上下文应存在");
        assert_eq!(snap.run_id, id);
        assert_eq!(snap.phase, "testing");
        assert_eq!(snap.step, "review:change-plan-review");
        assert_eq!(snap.attempt, 3);
        assert_eq!(attempt(), 3, "attempt 从同一处读");
        end_run(id);
        assert!(current().is_none(), "run 结束后上下文应清位");
    }

    #[test]
    fn the_step_of_the_current_run_is_written_into_the_heartbeat() {
        // 错实现形态：步骤不进心跳（只有阶段进）→ 死后只知道阶段，不知道卡在哪个环节
        let dir = tempfile::tempdir().unwrap();
        let id = "run-ctx-step";
        // 观测根**按 run 注入**（不抢进程级全局 → 与并行单测不互相干扰）
        begin_run_in(dir.path().to_path_buf(), id, "plan");
        set_step("review:change-coding-review");
        let hb = heartbeat::read(dir.path(), id).expect("心跳应已落盘");
        assert_eq!(hb.phase, "plan");
        assert_eq!(hb.step, "review:change-coding-review");
        end_run(id);
    }

    #[test]
    fn run_context_is_scoped_to_the_current_thread() {
        // 错实现形态：run 上下文是进程级单例（别的线程/别的 run 会串扰）→ 本用例必须变红
        let id = "run-ctx-2";
        let dir = tempfile::tempdir().unwrap();
        begin_run_in(dir.path().to_path_buf(), id, "plan");
        let other = std::thread::spawn(|| current().is_none()).join().unwrap();
        assert!(other, "别的线程不该看到本线程的 run 上下文");
        end_run(id);
    }

    #[test]
    fn review_round_counts_per_role_and_resets_per_run() {
        let id = "run-ctx-3";
        let dir = tempfile::tempdir().unwrap();
        begin_run_in(dir.path().to_path_buf(), id, "plan");
        assert_eq!(next_review_round("change-plan-review"), 1);
        assert_eq!(next_review_round("change-plan-review"), 2);
        assert_eq!(next_review_round("change-coding-review"), 1);
        begin_run_in(dir.path().to_path_buf(), id, "plan");
        assert_eq!(next_review_round("change-plan-review"), 1, "新 run 的计数从 1 起");
        end_run(id);
    }

    #[test]
    fn error_chain_keeps_every_source_layer() {
        let inner: crate::error::Result<()> = Err(DevforgeError::Quality("判负".into()));
        let wrapped = inner.ctx("审查轮").unwrap_err();
        let chain = error_chain(&wrapped);
        assert!(chain.contains("审查轮"), "缺上下文层：{chain}");
        assert!(chain.contains("判负"), "缺源头层：{chain}");
        assert!(chain.contains('→'), "层与层之间要有连接符：{chain}");
    }

    #[test]
    fn quality_error_chain_has_the_top_level_text() {
        let e = DevforgeError::Quality("协议违例达上限".into());
        let chain = error_chain(&e);
        assert!(chain.contains("协议违例达上限"), "{chain}");
        assert!(!chain.is_empty());
    }
}
