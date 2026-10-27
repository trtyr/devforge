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
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::error::DevforgeError;

/// 最近一次 agent 事件（turn_start / turn_end / 命令失败 / 流式 delta）的时间戳（ms），
/// **按 run 分槽**——地基批次 #2：旧实现是进程级单槽，上一 run 的旧戳会算到下一 run
/// 头上（前一 run 已静默超阈值时，下一 run 的 watchdog 首跳即误报 agent.stall）。
static AGENT_EVENTS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

fn agent_events() -> &'static Mutex<HashMap<String, u64>> {
    AGENT_EVENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
    /// 开跑时刻（ms；DR-3 停滞告警用：agent 从未开轮时按此刻起算）。
    pub started_at_ms: u64,
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
    begin_run_in_ctx(root, run_id, phase);
    write_heartbeat(run_id);
}

/// [`begin_run_in`] 的延迟盘写变体：只做内存置位（快照表 / 根表 / 线程本地），
/// **不写盘**——心跳首刷由 watchdog 周期 tick（≤300ms）承担，终态由
/// `run_with_watchdog` 收尾刷盘定格。
///
/// F18（10-23 resume6f 实测）：心跳首写在 run 线程上同步落盘、甚至后台线程写盘，
/// 都落在「开跑→执行→释放串行槽」关键路径上，拉长第一个 run 的占槽窗口 →
/// 并发 start 的第二个请求间歇吃 409（既有用例 14 连跑红 2 / spawn 变体 20 连跑红 3；
/// 基线 worktree 120 连跑 0 红）。纯内存置位 + 周期刷新：取证语义无损
/// （死亡现场的心跳由阶段转换事件与 watchdog 持续刷新，墓碑归属不走盘写）。
pub fn begin_run_in_deferred(root: PathBuf, run_id: &str, phase: &str) {
    begin_run_in_ctx(root, run_id, phase);
}

/// 内存置位（快照表 / 根表 / 线程本地），不落盘。
fn begin_run_in_ctx(root: PathBuf, run_id: &str, phase: &str) {
    let snap = RunSnapshot {
        run_id: run_id.to_string(),
        phase: phase.to_string(),
        step: String::new(),
        attempt: 1,
        started_at_ms: crate::domain::now_ms(),
    };
    runs().lock().unwrap_or_else(|e| e.into_inner()).insert(run_id.to_string(), snap);
    // 地基批次 #2：新 run 开跑前清掉同 id 的旧 agent 事件戳（按 run 分槽后每槽归本 run）
    agent_events().lock().unwrap_or_else(|e| e.into_inner()).remove(run_id);
    phase_logs().lock().unwrap_or_else(|e| e.into_inner()).insert(run_id.to_string(), Vec::new());
    roots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.to_string(), root);
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        l.run_id = Some(run_id.to_string());
        l.attempt = 1;
        l.review_rounds.clear();
    });
}

/// 各 run 的阶段转换历史（P012 M0 · T0.3 串行分数测量）：begin 时清空，set_phase 时追加。
/// 独立于快照表：end_run 不摘除（run 结束后仍可查四段边界戳；下个同 id run 开始时重置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhaseSpan {
    /// 转换发生时间（ms）。
    pub at_ms: u64,
    /// 来源阶段（首个 span 的 from = run 的初始阶段）。
    pub from: String,
    /// 目标阶段。
    pub to: String,
}

static PHASE_LOGS: OnceLock<Mutex<HashMap<String, Vec<PhaseSpan>>>> = OnceLock::new();

fn phase_logs() -> &'static Mutex<HashMap<String, Vec<PhaseSpan>>> {
    PHASE_LOGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 某 run 的阶段转换历史（无记录 = 空；串行分数 = 相邻 span 的 at_ms 差值）。
pub fn phase_log(run_id: &str) -> Vec<PhaseSpan> {
    phase_logs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(run_id)
        .cloned()
        .unwrap_or_default()
}

/// 各 run 的观测根（run 开始时捕获；键 = run id）。
static ROOTS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

fn roots() -> &'static Mutex<HashMap<String, PathBuf>> {
    ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 某 run 的观测根（未登记 = 该 run 不存在）。
fn run_root(run_id: &str) -> Option<PathBuf> {
    roots().lock().unwrap_or_else(|e| e.into_inner()).get(run_id).cloned()
}

/// 清位 run 上下文（run 结束）：**心跳文件保留**最后一次状态（不删——那是死亡现场的证据）。
pub fn end_run(run_id: &str) {
    // F18：终态心跳先落盘再摘快照（摘完 write_heartbeat 就找不到 run 了）；
    // 尾部写不触发开跑竞态（resume6f 实验已证）→ 短 run 也有心跳在案
    write_heartbeat(run_id);
    runs().lock().unwrap_or_else(|e| e.into_inner()).remove(run_id);
    roots().lock().unwrap_or_else(|e| e.into_inner()).remove(run_id);
    // 地基批次 #2：run 结束后摘掉本 run 的 agent 事件槽（下一 run 不得继承旧戳）
    agent_events().lock().unwrap_or_else(|e| e.into_inner()).remove(run_id);
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
        let mut table = runs().lock().unwrap_or_else(|e| e.into_inner());
        let Some(snap) = table.get_mut(&run_id) else {
            return;
        };
        let from = snap.phase.clone();
        snap.phase = phase.to_string();
        phase_logs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(run_id.clone())
            .or_default()
            .push(PhaseSpan {
                at_ms: crate::domain::now_ms(),
                from,
                to: phase.to_string(),
            });
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
        let mut table = runs().lock().unwrap_or_else(|e| e.into_inner());
        let Some(snap) = table.get_mut(&run_id) else {
            return;
        };
        snap.step = step.to_string();
    }
    write_heartbeat(&run_id);
}

/// 当前线程的 run id（无 = 不在这个 run 里）——引擎构造子会话工厂时用它给
/// `DEVFORGE_MANAGED` 定父 run（否则审查子进程杀不掉，引擎批次 #1）。
pub fn current_run_id() -> Option<String> {
    LOCAL.with(|l| l.borrow().run_id.clone())
}

/// 更新尝试次数（重试层每次尝试更新；`map_intervention` 从同一线程读同一个值 → 两处字段同构）。
pub fn set_attempt(attempt: u32) {
    LOCAL.with(|l| l.borrow_mut().attempt = attempt);
    let run_id = LOCAL.with(|l| l.borrow().run_id.clone());
    if let Some(id) = run_id {
        let mut table = runs().lock().unwrap_or_else(|e| e.into_inner());
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
    runs().lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
}

/// DR-2/DR-3：记录一次 agent 事件（turn_start / turn_end / 流式 delta / 命令失败），
/// 供活性与停滞告警判定。**按 run 分槽**（地基批次 #2）；无 run 上下文（单测/非 run 路径）
/// 记在空键下，不污染任何真 run 的判定。
pub fn note_agent_event() {
    let key = current_run_id().unwrap_or_default();
    note_agent_event_for(&key);
}

/// 指定 run 键的活性打点（2026-10-27 自审 major）：RPC 客户端跑在 worker 线程上，
/// thread-local run 上下文为空 → 旧口径活性戳全落空键，该 run 的停滞检测退化成
/// 「按开跑时刻起算」——长跑真审计误报 agent.stall，真停滞反而看不见。客户端
/// 持有自己的 run-id，按它记键。
pub fn note_agent_event_for(run_id: &str) {
    agent_events()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.to_string(), crate::domain::now_ms());
}

/// 某个 run 距最近一次 agent 事件的毫秒数（该 run 从未有过事件 = `None`）。
fn agent_event_age_for(run_id: &str) -> Option<u64> {
    let t = *agent_events()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(run_id)?;
    (t != 0).then(|| crate::domain::now_ms().saturating_sub(t))
}

/// 距最近一次 agent 事件的毫秒数（**当前线程所在 run**；无 run 上下文 = 空键口径）。
pub fn last_agent_event_age_ms() -> Option<u64> {
    let key = current_run_id().unwrap_or_default();
    agent_event_age_for(&key)
}

/// DR-3：停滞判定——返回「该 run 距最近 agent 事件的毫秒数」（超阈值与否由调用方比）。
/// 该 run 从未有 agent 事件时按 run 开跑时刻起算（DR-1 型「根本没开轮」也要能报警）。
pub fn stall_check(run_id: &str) -> Option<u64> {
    match agent_event_age_for(run_id) {
        Some(age) => Some(age),
        None => run_started_at(run_id).map(|t0| crate::domain::now_ms().saturating_sub(t0)),
    }
}

/// run 的开跑时刻（进程级快照表；watchdog 线程用）。
pub fn run_started_at(run_id: &str) -> Option<u64> {
    runs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(run_id)
        .map(|s| s.started_at_ms)
}

/// 停滞告警阈值（ms）；`DEVFORGE_STALL_WARN_MS` 可覆盖（测试用小值），默认 15 分钟。
pub fn stall_warn_ms() -> u64 {
    std::env::var("DEVFORGE_STALL_WARN_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(900_000)
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
        .unwrap_or_else(|e| e.into_inner())
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
        let mut table = last_touch().lock().unwrap_or_else(|e| e.into_inner());
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
    let mut slot = root_slot().lock().unwrap_or_else(|e| e.into_inner());
    *slot = Some(root.into());
}

fn root_slot() -> &'static Mutex<Option<PathBuf>> {
    static SLOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 解析观测根：进程内注入 > `DEVFORGE_HOME` > `HOME/.devforge`。
pub fn root() -> PathBuf {
    if let Some(r) = root_slot().lock().unwrap_or_else(|e| e.into_inner()).clone() {
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
/// 事件总线（SSE 单源序号/留档/广播；ARC-R2 自 server 迁入）。
pub mod events;
/// 死亡取证（panic 墓碑）。
pub mod tombstone;
/// 失活收割看门狗（会话指纹活性监测；ARC-R2 自 server 迁入）。
pub mod watchdog;

/// 测试支撑：把一段闭包里的 tracing 日志收成 JSONL 全文（行为断言核对「日志面」用）。
///
/// **为什么用全局订阅器 + 线程级缓冲**：`tracing` 的 callsite 兴趣是**全局缓存**的
/// （第一次求值时定型，之后不再变）。用线程级 `with_default` 捕获时，别的用例先跑过
/// 同一个 callsite 就会把它缓存成「永不记录」→ 捕获器随机收不到（并行测试下必现）。
/// 故：全局装一个订阅器（所有 callsite 恒为 interested），日志按**线程**分流进各自的缓冲，
/// 捕获时只取本线程的缓冲 → 既不受别的用例干扰，也不串味。
#[cfg(test)]
#[path = "test_support.rs"]
pub mod test_support;

#[cfg(test)]
mod tests;
