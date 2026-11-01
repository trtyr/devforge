//! 依赖图调度器 + 准入控制（P012 M4，topic 06）。
//!
//! 调度循环（`scheduler_loop`）：周期 + kick 即时地跑 `tick`——
//! ① 超预算熔断（cancel 置位，引擎按失败收尾）；
//! ② Settled 且带契约的任务自动入集成队列（M3 MergeQueue，闭环自动化）；
//! ③ 就绪判定（depends_on 全 Completed）→ 准入（并发上限/独占窗口）→ 派工计划；
//! ④ blocked 语义：capacity / exclusive_window / dependency_failed / integration_window，
//!    恢复联动自动迁出。
//!
//! 派工决策（tick，纯函数可测）与派工执行（`launch`，async：渲染闸门等待）分离。
//!
//! 2026-10-31（autonomy-gaps gap-E）：集成队列**有了生产消费方**——`scheduler_loop` 另拉起
//! 一个常驻的集成消费循环（周期 30s，开关每轮重读 → 改文件即热生效）：
//! ⑤ 自动集成消费（settled 候选 → integrate → 验证 → 终态）；集成窗口期内**不派新 run**
//!    （错峰：验证与 run 派发争同一台机，验证被自家并发 run 拖成 flaky 就没人敢开开关）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::config::ConfigManifest;
use crate::domain::run::RunStatus;
use crate::domain::store::Store;
use crate::domain::task::TaskStatus;
use crate::domain::now_ms;
use crate::integration::{Candidate, CandidateStatus, MergeQueue};
use crate::server::{run_in_background, SharedState};

/// 准入配置（topic 06 准入表）。
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// 链末位缺省：并发 mutation 上限（2）。
    ///
    /// **本字段不再是并发上限的来源**：生效值在进程启动期按优先级链解析并回写
    /// `store.max_concurrent_runs`（`DEVFORGE_MAX_CONCURRENT` > 持久值 > 本字段，
    /// 见 `apply_startup_concurrency`）。本字段只剩一个消费点——解析函数的 fallback 实参。
    pub max_concurrent_mutation: usize,
    /// 每 work item 墙钟预算分钟，超限熔断（0 = 不限）。
    pub budget_wall_minutes: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_mutation: 2,
            budget_wall_minutes: 240,
        }
    }
}

/// 并发生效值的来源（优先级链的判据——留痕与事后复盘都靠它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcurrencySource {
    /// 环境变量 `DEVFORGE_MAX_CONCURRENT`（最高优先）。
    Env,
    /// store 的持久值（次优先）。
    Store,
    /// 链末位缺省（`SchedulerConfig::default().max_concurrent_mutation`）。
    Default,
}

impl ConcurrencySource {
    /// 留痕口径（小写字面量：日志与 status 用同一套词）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Store => "store",
            Self::Default => "default",
        }
    }
}

/// 并发解析结果（纯函数产物）。
///
/// `ignored_env` 让「被判非法的 env 原值」成为**可断言的数据**——WARN 的触发条件
/// 因此不必捕获日志输出就能被单测钉住。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyResolution {
    /// 生效并发值（≥ 1）。
    pub value: usize,
    /// 生效值来源。
    pub source: ConcurrencySource,
    /// 被判非法的 env 原值（None = env 未设或合法）。
    pub ignored_env: Option<String>,
    /// 落地前的 store 值（归一化口径：0 视为 1）——与 `value` 相同即「值未变」。
    pub previous: usize,
}

/// 并发优先级链（纯函数）：`DEVFORGE_MAX_CONCURRENT` > store 持久值 > 缺省。
///
/// - `env_raw`：env 原串（**原样**，不 trim）——非法时原值进 `ignored_env` 供留痕。
///   合法口径：trim 后能解析成 ≥ 1 的 usize（空串/纯空白/0/负数/非数字均非法）。
/// - `persisted`：可信的持久值；`None` = 没有可信持久值（首启 / 坏文件已隔离）→ 取缺省。
///   0 按 1 处理（与 `Store::begin_run` 的 cap 逐字同口径）。
/// - `fallback`：链末位缺省。
///
/// 不读进程 env：env 原串由调用方传入（单测因此不与并行用例串味）。
pub fn resolve_max_concurrent(
    env_raw: Option<&str>,
    persisted: Option<usize>,
    fallback: usize,
) -> ConcurrencyResolution {
    let previous = persisted.map(|p| p.max(1)).unwrap_or(1);
    let env_value = env_raw.and_then(|raw| raw.trim().parse::<usize>().ok().filter(|v| *v >= 1));
    match env_value {
        Some(value) => ConcurrencyResolution {
            value,
            source: ConcurrencySource::Env,
            ignored_env: None,
            previous,
        },
        None => {
            let (value, source) = match persisted {
                Some(p) => (p.max(1), ConcurrencySource::Store),
                None => (fallback.max(1), ConcurrencySource::Default),
            };
            ConcurrencyResolution {
                value,
                source,
                ignored_env: env_raw.map(|raw| raw.to_string()),
                previous,
            }
        }
    }
}

/// 启动期并发落地：把优先级链的生效值写回 store（并发的唯一运行时真相源）。
///
/// **必须在 store 移入 `ServerState` 之前调用**——本函数拿不到 `store_path`，
/// 落盘由调用方在生效值变化时显式 `Store::save`（`previous != value` 才写）。
///
/// - `env_raw`：`DEVFORGE_MAX_CONCURRENT` 原串（不读进程 env——由调用方传入）。
/// - `store_trusted`：store 是否可信加载（`false` = 首启 / 坏文件已隔离 → 持久值不可信，
///   按未配置取缺省）。
///
/// 落一条 INFO（`resolved` / `source` / `previous`）；env 非法时另落一条 WARN
/// （`event = "scheduler.max_concurrent_env_invalid"`，带原值与最终采用的 source）。
pub fn apply_startup_concurrency(
    store: &mut Store,
    env_raw: Option<&str>,
    store_trusted: bool,
) -> ConcurrencyResolution {
    let persisted = if store_trusted {
        Some(store.max_concurrent_runs)
    } else {
        None
    };
    let mut resolution = resolve_max_concurrent(
        env_raw,
        persisted,
        SchedulerConfig::default().max_concurrent_mutation,
    );
    // previous 取落地前 store 的归一化值（0 视为 1）——与 value 同口径可比：
    // 两者相等即「生效值没变」，调用方据此不写盘。
    resolution.previous = store.max_concurrent_runs.max(1);
    store.max_concurrent_runs = resolution.value;
    tracing::info!(
        component = "scheduler",
        event = "scheduler.max_concurrent_resolved",
        resolved = resolution.value,
        source = resolution.source.as_str(),
        previous = resolution.previous,
        "并发生效值已解析（DEVFORGE_MAX_CONCURRENT > store 持久值 > 缺省）"
    );
    if let Some(raw) = resolution.ignored_env.as_deref() {
        tracing::warn!(
            component = "scheduler",
            event = "scheduler.max_concurrent_env_invalid",
            invalid_env = raw,
            resolved = resolution.value,
            source = resolution.source.as_str(),
            "DEVFORGE_MAX_CONCURRENT 取值非法（已回退下一优先级）"
        );
    }
    resolution
}

/// 单次调度报告（tick 的纯决策产物；dispatched 的实际 launch 由循环执行）。
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct TickReport {
    /// 派工计划：`project/task`（循环据此 launch）。
    /// 派工计划：**结构化对**（自审 minor 232：旧口径用 `project/task` 复合字符串传递、
    /// 再用 split_once('/') 反解——标识符含 '/' 时解析错位，派工静默落空）。
    pub dispatched: Vec<(String, String)>,
    /// 阻塞：`project/task` + 原因（capacity / exclusive_window / dependency_failed）。
    pub blocked: Vec<(String, String)>,
    /// 超预算熔断。
    pub fused: Vec<String>,
    /// 自动入集成队列。
    pub auto_enqueued: Vec<String>,
}

/// 调度踢脚：run 收尾置 true，循环立即 tick（不等周期）。
static KICK: AtomicBool = AtomicBool::new(false);

pub fn kick() {
    KICK.store(true, Ordering::Relaxed);
}

fn load_kick() -> bool {
    KICK.swap(false, Ordering::Relaxed)
}

/// 自动集成消费循环的周期（秒）——开关每轮重读，改文件即热生效。
const INTEGRATION_PERIOD: Duration = Duration::from_secs(30);

/// 集成窗口（进程级）：自动集成验证进行中 → tick 不派新 run（错峰口径）。
///
/// 进程级而非项目级：集成 worktree 名按仓名派生（`_integration` 固定名独占），
/// 同一台机上的集成本来就是全局串行的。
static INTEGRATION_WINDOW: AtomicBool = AtomicBool::new(false);

/// 集成窗口态（tick 与 `status()` 读同一份，单一事实源）。
pub fn integration_window_in_flight() -> bool {
    INTEGRATION_WINDOW.load(Ordering::Relaxed)
}

/// 单次调度（纯决策：改状态/入队/发事件；launch 由调用方执行）。
/// 已熔断过的 run id（自审 minor 227：重复熔断 = 重复 SIGKILL + 重复事件）。
///
/// 进程级集合：run 收场时无需清理（id 唯一，容量与「本进程见过的超预算 run 数」同阶）。
fn fused_runs() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static FUSED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    FUSED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn already_fused(run_id: &str) -> bool {
    fused_runs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(run_id)
}

fn mark_fused(run_id: &str) {
    fused_runs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.to_string());
}

/// 渠道事故去重集（进程级，仿 `fused_runs`）。
///
/// 不去重则每次 tick（30s 周期）都会把到期时间推到 now + TTL——熔断永不过期，
/// 而且每次都重写整份 store.json、事件反复刷屏。去重后同一次失败只处置一次，
/// `expires_at_ms` 也就不随 tick 推进（验收标准：连扫三次到期时刻完全相同）。
///
/// 键带语义前缀（`incident:` / `cooldown:`）：同一个集合服务两种去重语义，互不干扰。
fn channel_seen_runs() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// 该 run 在给定语义下是否已处置过。
fn channel_run_seen(kind: &str, run_id: &str) -> bool {
    channel_seen_runs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&format!("{kind}:{run_id}"))
}

/// 标记该 run 在给定语义下已处置过。
fn mark_channel_run_seen(kind: &str, run_id: &str) {
    channel_seen_runs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(format!("{kind}:{run_id}"));
}

// ---------- A2 通知扫描（ui 待答久等 / 集成 NeedsRepair） ----------

fn ui_notify_seen_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn ui_notify_seen(id: &str) -> bool {
    ui_notify_seen_set()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(id)
}

fn mark_ui_notify_seen(id: &str) {
    ui_notify_seen_set()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string());
}

fn integration_repair_seen_set()
-> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn integration_repair_seen(key: &str) -> bool {
    integration_repair_seen_set()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(key)
}

fn mark_integration_repair_seen(key: &str) {
    integration_repair_seen_set()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.to_string());
}

/// A2：ui 待答久等 → 通知。与 scan_pending_warns 的 WARN 同一阈值旋钮，
/// 但各自独立去重（WARN 是日志面，通知是产品面，互不抢标志）。
/// 通知一次后同一待答不再重复（id 全局唯一，seen 集合按 id 去重）。
/// 年龄取 pending_snapshot 的存活时长（墙钟，saturating——不另传 now）。
pub(crate) fn notify_pending_uis(st: &SharedState) {
    let threshold = crate::server::warn_threshold_ms();
    if threshold == 0 {
        return; // 旋钮关闭：不告警也不通知
    }
    for ask in st.ui.pending_snapshot() {
        if ui_notify_seen(&ask.id) || ask.age_ms <= threshold {
            continue;
        }
        mark_ui_notify_seen(&ask.id);
        crate::server::notify(
            st,
            "ui_pending",
            "warn",
            &format!(
                "agent 提问等待裁决 {}s：{}",
                ask.age_ms / 1000,
                crate::server::question_prefix(&ask.question)
            ),
            ask.run_id.as_deref(),
        );
    }
}

/// A2：集成队列 NeedsRepair → 通知。队列**文件**是生产事实源——不管谁把候选
/// 推进修复队列（将来的自动集成消费者 / 手工 API / 脚本），tick 都看得到；
/// 去重键 = project + task_id + frozen_sha（同一候选修完重入 = 新 frozen_sha = 新通知）。
/// A2：集成队列 NeedsRepair → 通知。队列**文件**是生产事实源——不管谁把候选
/// 推进修复队列（将来的自动集成消费者 / 手工 API / 脚本），tick 都看得到；
/// 去重键 = project + task_id + frozen_sha（同一候选修完重入 = 新 frozen_sha = 新通知）。
pub(crate) fn notify_integration_repairs(st: &SharedState) {
    let projects: Vec<String> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.projects.iter().map(|p| p.name.clone()).collect()
    };
    for project in projects {
        let Ok(queue) = MergeQueue::load(&st.plans_root, &project) else {
            continue;
        };
        for cand in &queue.candidates {
            let crate::integration::CandidateStatus::NeedsRepair { reason } = &cand.status else {
                continue;
            };
            let key = format!("{project}:{}:{}", cand.task_id, cand.frozen_sha);
            if integration_repair_seen(&key) {
                continue;
            }
            mark_integration_repair_seen(&key);
            crate::server::notify(
                st,
                "integration_needs_repair",
                "warn",
                &format!(
                    "{project}/{} 集成转修复队列：{}",
                    cand.task_id,
                    reason_summary(reason)
                ),
                Some(&format!("{project}/{}", cand.task_id)),
            );
        }
    }
}

/// 失败原因摘要（写进熔断条目与事件）：压掉换行、截断到 200 字。
/// 运维要的是一眼看出「为什么这条在冷却」，不是失败全文。
fn reason_summary(reason: &str) -> String {
    const MAX: usize = 200;
    let flat = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        return flat;
    }
    let head: String = flat.chars().take(MAX).collect();
    format!("{head}…")
}

pub fn tick(st: &SharedState, cfg: &SchedulerConfig) -> TickReport {
    // 集成窗口读进程级原子（与 `status()` 同一事实源）。
    tick_with_integration_window(st, cfg, integration_window_in_flight())
}

/// 单次调度（**显式注入**集成窗口态）。
///
/// 显式入参的理由：进程级原子在 `cargo test` 并行下会互相污染（一个用例置位、另一个用例
/// 断言派工）——显式入参让「集成窗口期不派工」可确定性断言，不产生变化期断言。
/// 既有 `tick(&state, &cfg)` 调用点零改动。
pub fn tick_with_integration_window(
    st: &SharedState,
    cfg: &SchedulerConfig,
    integration_in_flight: bool,
) -> TickReport {
    let mut report = TickReport::default();

    // ① 超预算熔断：Running 且 started_at + budget < now → cancel 置位 + 台账强杀。
    //    自审 minor（227）：必须有幂等标记——旧口径同一个仍 Running 的超预算 run 每 30s
    //    重复熔断一次（重复 SIGKILL + 重复 budget_fused 事件），且 killed=0 时不做任何
    //    升级处置，run 永久占着并发槽。
    // 复验归因：cancel 是协作式（SOP 轮询间隙检查），pi 子进程工具挂死时无人消费，
    // run 永驻 Running（M3 挂死 14h 的直接教训）——必须台账精确 SIGKILL。
    let to_fuse: Vec<(String, String, String)> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        if cfg.budget_wall_minutes == 0 {
            Vec::new()
        } else {
            let budget_ms = cfg.budget_wall_minutes * 60_000;
            store
                .runs
                .iter()
                .filter(|r| {
                    r.status == RunStatus::Running
                        && now_ms().saturating_sub(r.started_at_ms) > budget_ms
                })
                .map(|r| (r.task_project.clone(), r.task_id.clone(), r.id.0.clone()))
                .filter(|(_, _, run_id)| !already_fused(run_id))
                .collect()
        }
    };
    for (project, task, run_id) in &to_fuse {
        // 自审 major（127）：project/task 是用户可控标识，进路径前必须校验
        if crate::domain::store::Store::validate_path_name(project).is_err()
            || crate::domain::store::Store::validate_path_name(task).is_err()
        {
            tracing::error!(
                component = "scheduler",
                project = %project,
                task = %task,
                "熔断：标识符形状非法（拒绝拼进文件路径）"
            );
            continue;
        }
        // 强杀统一入口（引擎审计修复 #9：台账精确优先，缺失降级扫盘兕底+WARN）
        let ledger_path = st.plans_root.join(project).join("spawn-ledger.json");
        let killed = crate::engine::ledger::kill_run(Some(&ledger_path), run_id);
        tracing::warn!(
            component = "scheduler",
            project,
            task,
            run_id,
            budget_minutes = cfg.budget_wall_minutes,
            killed,
            "budget 熔断：run 超墙钟上限，强杀挂死进程并置取消位"
        );
        let control = st
            .runs_control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&format!("{project}/{task}"))
            .cloned();
        if let Some(c) = control {
            c.cancel();
        }
        mark_fused(run_id);
        // killed=0 且没有在册控制句柄 = 该 run 的线程已经不在了，但 store 里仍是 Running
        // （占着并发槽）。升级为直接落终态，否则槽位永久泄漏。
        if killed == 0 && st.runs_control.lock().unwrap_or_else(|e| e.into_inner()).get(&format!("{project}/{task}")).is_none() {
            let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            let finish_ok = match store.finish_run(
                &crate::domain::run::RunId(run_id.clone()),
                crate::domain::store::FinishOutcome::Failed {
                    reason: format!("超预算 {} 分钟且无存活进程可杀（调度器升级结案）", cfg.budget_wall_minutes),
                },
            ) {
                Ok(()) => {
                    tracing::error!(
                        component = "scheduler",
                        project = %project,
                        task = %task,
                        run_id = %run_id,
                        "熔断升级：无进程可杀且无控制句柄，直接结案释放并发槽"
                    );
                    true
                }
                Err(e) => {
                    tracing::error!(
                        component = "scheduler",
                        run_id = %run_id,
                        error = %e,
                        "熔断升级失败：run 仍占并发槽"
                    );
                    false
                }
            };
            drop(store);
            // P020：issue 联动（熔断失败回炉）——必须在 store 锁作用域外（Mutex 不可重入）
            if finish_ok {
                crate::server::issue_run_outcome_linkage(
                    st,
                    project,
                    task,
                    run_id,
                    false,
                    "超预算熔断升级结案",
                );
            }
        }
        report.fused.push(format!("{project}/{task}"));
        st.emit(serde_json::json!({
            "type": "scheduler.budget_fused",
            "project": project,
            "task": task,
            "run_id": run_id,
            "budget_minutes": cfg.budget_wall_minutes,
            "killed": killed,
        }));
    }

    // ② 渠道事故扫描（gap-B）：失败 run 的原因命中渠道事故签名 → 熔断对应渠道。
    //
    // 触发点选 tick 而不是 `finish_run`：`finish_run` 手里只有 store，拿不到 live 清单
    // （Store 结构体没有 manifest 字段），而归因必须知道「agent → 池条目」；
    // tick 同时握着 store 与 ServerState.manifest。run 收尾会 kick() → 扫描延迟≈0。
    //
    // 锁序（风险 R5）：manifest 快照在 store 锁**之外**取，锁序恒为 manifest → store。
    let channel_index = {
        let manifest = st.manifest.lock().unwrap_or_else(|e| e.into_inner());
        ChannelIndex::build(&manifest, &st.registry.ids())
    };
    let now = now_ms();
    // 过期清理：TTL 到期自然解除（下轮 tick 起不再算冷却）。
    {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store.purge_expired_channel_breakers(now);
    }
    // 窗口内的失败 run（窗口基准 = `ended_at_ms`：失败时刻才是事故时刻；
    // `started_at_ms` 对长 run 可能早得多，用它会把「跑了一小时后 402」的 run 漏掉；
    // 同时挡住「重启后把陈旧失败重放成熔断」（风险 R4））。
    let failed_runs: Vec<(String, String, String, String)> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Failed)
            .filter_map(|r| {
                let reason = r.reason.clone()?;
                let ended = r.ended_at_ms.unwrap_or(now);
                if now.saturating_sub(ended) > CHANNEL_BREAKER_MAX_TTL_MS {
                    return None; // 陈旧失败：不重放熔断
                }
                Some((
                    r.id.0.clone(),
                    r.task_project.clone(),
                    r.task_id.clone(),
                    reason,
                ))
            })
            .collect()
    };
    for (run_id, project, task_id, reason) in failed_runs {
        if channel_run_seen("incident", &run_id) {
            continue; // 同一次失败只处置一次（否则到期时刻被反复推迟）
        }
        // 不是渠道事故（质量失败等）→ 不动熔断表。
        let Some(ttl_ms) = channel_incident_ttl_ms(&reason) else {
            continue;
        };
        let agent = extract_agent_role(&reason);
        let channel = agent
            .as_deref()
            .and_then(|role| channel_index.channel_of(role))
            .map(|c| c.to_string());
        let summary = reason_summary(&reason);
        match channel {
            Some(channel) => {
                let expires_at_ms = now + ttl_ms;
                {
                    let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                    store.record_channel_breaker(&channel, expires_at_ms, &summary);
                }
                mark_channel_run_seen("incident", &run_id);
                st.emit(serde_json::json!({
                    "type": "scheduler.channel_breaker_tripped",
                    "project": project,
                    "task": task_id,
                    "run_id": run_id,
                    "channel": channel,
                    "agent": agent,
                    "ttl_ms": ttl_ms,
                    "expires_at_ms": expires_at_ms,
                    "reason": summary,
                }));
                // A2：熔断命中 → 通知（这是运营者必须马上知道的事，error 级）。
                crate::server::notify(
                    st,
                    "channel_breaker",
                    "error",
                    &format!(
                        "{channel} 进入冷却 {}s（{project}/{task_id}：{summary}）",
                        ttl_ms / 1000
                    ),
                    Some(&format!("{project}/{task_id}")),
                );
            }
            None => {
                // 归因不出渠道 → 不熔断（宁可漏熔，不可错杀健康渠道导致全线饿死），只记 WARN。
                mark_channel_run_seen("incident", &run_id);
                tracing::warn!(
                    component = "scheduler",
                    project = %project,
                    task = %task_id,
                    run_id = %run_id,
                    agent = ?agent,
                    "渠道事故归因不出具体池条目：不熔断，只记 WARN（宁可漏熔，不可错杀）"
                );
                st.emit(serde_json::json!({
                    "type": "scheduler.channel_breaker_unattributed",
                    "project": project,
                    "task": task_id,
                    "run_id": run_id,
                    "agent": agent,
                    "ttl_ms": ttl_ms,
                    "reason": summary,
                }));
            }
        }
    }

    // 撞熔断观测（gap-B · 纯观测不干预）：显式 start 路径绕过闸门（本期不改 handlers.rs），
    // tick 扫 Running run——若它引用的渠道正在冷却，发一次事件让运维看得见。
    // 不拦、不改状态：显式 start 是明确点名的意图通道，本期只补「看得见」。
    let running_runs: Vec<(String, String, String)> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .map(|r| (r.id.0.clone(), r.task_project.clone(), r.task_id.clone()))
            .collect()
    };
    for (run_id, project, task_id) in running_runs {
        if channel_run_seen("cooldown", &run_id) {
            continue; // 同一 run 只报一次（不刷屏）
        }
        let cooled = {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            let sop = store
                .tasks
                .iter()
                .find(|t| t.project == project && t.id == task_id)
                .map(|t| t.sop.clone());
            sop.and_then(|sop| cooled_channel(&channel_index, &sop, &store.channel_breakers, now))
        };
        let Some((channel, remaining_ms)) = cooled else {
            continue;
        };
        mark_channel_run_seen("cooldown", &run_id);
        st.emit(serde_json::json!({
            "type": "channel.running_on_cooldown",
            "project": project,
            "task": task_id,
            "run_id": run_id,
            "channel": channel,
            "remaining_secs": remaining_secs(remaining_ms),
        }));
    }

    // ②′ 通知扫描（A2 · 通知系统）：ui 待答久等 + 集成 NeedsRepair。
    //    渠道熔断的通知在上方 incident 命中处内联发。
    notify_pending_uis(st);
    notify_integration_repairs(st);

    // ③ 自动入集成队列：Completed + 带契约 + frozen-commit 落盘 + 队列内无此任务。
    let settled: Vec<(String, String)> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed && t.contract.is_some())
            .map(|t| (t.project.clone(), t.id.clone()))
            .collect()
    };
    // 自审 minor（229）：NeedsRebase 状态机扫动只在**本 tick 每项目一次**（不是每 settled
    // 任务都扫一遍——旧口径对项目内全部候选重复扫 + 重复 load/save）。
    let mut swept_this_tick: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (project, task_id) in settled {
        // 自审 major（127）：同上，标识符进路径前校验
        if crate::domain::store::Store::validate_path_name(&project).is_err()
            || crate::domain::store::Store::validate_path_name(&task_id).is_err()
        {
            tracing::error!(
                component = "scheduler",
                project = %project,
                task = %task_id,
                "自动入队：标识符形状非法（拒绝拼进文件路径）"
            );
            continue;
        }
        let frozen_path = st
            .plans_root
            .join(&project)
            .join(&task_id)
            .join("frozen-commit.txt");
        let sha = match std::fs::read_to_string(&frozen_path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue, // 无冻结产物（非 worktree 任务）不入队
        };
        if sha.is_empty() {
            continue;
        }
        let contract = {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            store
                .get_task(&project, &task_id)
                .and_then(|t| t.contract.clone())
        };
        let Some(contract) = contract else { continue };
        // 自审 major：入队走锁定读-改-写（rearm + 去重 + enqueue 同一把锁）——
        // 旧口径无锁 load→save，与 API 入队并发时互相覆写（丢候选）。
        let enqueued = MergeQueue::mutate(&st.plans_root, &project, |queue| {
            // 自审 minor（229）：NeedsRebase 扫动（状态机改写）只做**每项目一次**——
            // 旧口径在「每个 settled 任务」上对项目内全部候选重复扫 + 重复 load/save。
            // 放在首个入队机会里执行，并只跑一次（PROJECT_SWEPT 记录）。
            if swept_this_tick.insert(project.clone()) {
                // 全仓审计 #2：NeedsRebase 不是终态——rebase 到新目标后重验；
                // NeedsRepair 保持终态（修复 = 新候选带新证据 = 新 frozen_sha）。
                // 单一事实源：与自动消费方（consumer）共用 rearm，两处口径不许漂移。
                crate::integration::rearm_needs_rebase(&project, queue);
            }
            // 去重口径（全仓审计 #2）：Pending/Integrated = 本任务已在队或已合入；
            // NeedsRepair 只在 frozen_sha 未变时跳过；任务重跑产生新 frozen_sha
            // 即视为「新证据」允许入队——不再静默停滞。
            let already = queue.candidates.iter().any(|c| {
                c.task_id == *task_id
                    && match &c.status {
                        CandidateStatus::Pending | CandidateStatus::Integrated { .. } => true,
                        CandidateStatus::NeedsRebase { .. } => true,
                        CandidateStatus::NeedsRepair { .. } => c.frozen_sha == *sha,
                    }
            });
            if already {
                return Ok(false);
            }
            queue.enqueue(Candidate {
                task_id: task_id.clone(),
                frozen_sha: sha.clone(),
                base_rev: contract.base_rev.clone().unwrap_or_default(),
                owned_paths: contract.owned_paths.clone(),
                status: CandidateStatus::Pending,
                evidence_paths: vec![],
            });
            Ok(true)
        });
        match enqueued {
            Ok(true) => {
                report.auto_enqueued.push(format!("{project}/{task_id}"));
                st.emit(serde_json::json!({
                    "type": "integration.auto_enqueued",
                    "project": project,
                    "task": task_id,
                }));
            }
            Ok(false) => {}
            Err(e) => {
                // 自审 minor（230）：入队落盘失败是「候选静默丢失」级别，不能只 warn；
                // 与相邻回写失败口径一致 → error + 事件。
                tracing::error!(
                    component = "scheduler",
                    project = %project,
                    task = %task_id,
                    error = %e,
                    "自动入队失败（队列读改写/落盘），候选未入队"
                );
                st.emit(serde_json::json!({
                    "type": "integration.auto_enqueue_failed",
                    "project": project,
                    "task": task_id,
                    "error": e.to_string(),
                }));
            }
        }
    }

    // ④ 就绪判定 + 准入 → 派工计划。
    let snapshot: Vec<(String, String, TaskStatus, Vec<String>, bool, String)> = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .tasks
            .iter()
            .map(|t| {
                (
                    t.project.clone(),
                    t.id.clone(),
                    t.status,
                    t.depends_on.clone(),
                    t.exclusive,
                    // 渠道闸门要按 sop 反查 profile → 渠道（sop 不在快照里就没法判冷却）。
                    t.sop.clone(),
                )
            })
            .collect()
    };
    let exclusive_running = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Running && t.exclusive)
    };
    let dep_status = |store: &crate::domain::store::Store,
                      project: &str,
                      dep: &str|
     -> TaskStatus {
        store
            .tasks
            .iter()
            .find(|t| t.project == project && t.id == dep)
            .map(|t| t.status)
            .unwrap_or(TaskStatus::Failed)
    };

    let mut dispatched_this_tick = 0usize;
    let mut exclusive_dispatched_this_tick = false;
    for (project, task_id, status, deps, exclusive, sop) in snapshot {
        if !matches!(status, TaskStatus::Pending | TaskStatus::Blocked) {
            continue;
        }
        // **不改任务状态**（窗口是瞬态的，改状态会造成状态抖动；下次 tick 窗口关了再正常判）。
        if integration_in_flight {
            report
                .blocked
                .push((format!("{project}/{task_id}"), "integration_window".into()));
            continue;
        }
        // 依赖判定：任一 Failed → blocked{dependency_failed}；任一未完成 → 继续等。
        {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            if deps
                .iter()
                .any(|d| dep_status(&store, &project, d) == TaskStatus::Failed)
            {
                drop(store);
                let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                // 自审 minor（231）：状态迁移失败不得静默（否则「报告说已阻塞、store 里仍 Pending」）
                if let Err(e) = store.set_task_status(&project, &task_id, TaskStatus::Blocked) {
                    tracing::error!(
                        component = "scheduler",
                        project = %project,
                        task = %task_id,
                        error = %e,
                        "置 Blocked 失败（dependency_failed）"
                    );
                }
                report
                    .blocked
                    .push((format!("{project}/{task_id}"), "dependency_failed".into()));
                continue;
            }
            if deps
                .iter()
                .any(|d| dep_status(&store, &project, d) != TaskStatus::Completed)
            {
                continue; // 依赖未就绪——静默等（非阻塞态）
            }
        }
        // 渠道闸门（gap-B）：任务引用的渠道在冷却中 → 不发 run、任务保持 Pending。
        // **绝不标 Failed**：熔断是调度决策，不是任务失败（前置裁决：状态保持 Pending，
        // TTL 过后 tick 自然派发；已 Failed 的 run 不因熔断恢复而自动重跑）。
        // 位置：依赖判定之后、独占/容量之前（依赖没就绪的任务与渠道无关，先等依赖）。
        {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((channel, remaining_ms)) =
                cooled_channel(&channel_index, &sop, &store.channel_breakers, now)
            {
                drop(store);
                report.blocked.push((
                    format!("{project}/{task_id}"),
                    format!("channel_cooldown({channel},{}s)", remaining_secs(remaining_ms)),
                ));
                continue; // 不进「解除阻塞 → 派工」分支
            }
        }
        // 独占窗口：已有独占任务在跑，或本 tick 已派工过独占任务（复验 #3：
        // running 集合在后台线程 begin_run 前不变，不追踪本 tick 派工就会同 tick 双独占并行）。
        if exclusive_running || exclusive_dispatched_this_tick {
            {
                let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = store.set_task_status(&project, &task_id, TaskStatus::Blocked) {
                    tracing::error!(
                        component = "scheduler",
                        project = %project,
                        task = %task_id,
                        error = %e,
                        "置 Blocked 失败（exclusive_window）"
                    );
                }
            }
            report.blocked.push((
                format!("{project}/{task_id}"),
                "exclusive_window".into(),
            ));
            continue;
        }
        // 容量：running + 本 tick 已派工（预留）< store 的生效并发值（单一事实源）——
        // begin_run 发生在后台线程，tick 内 running_run_count 不变，必须本地预留。
        // 生效值在进程启动期已按优先级链解析并回写 store（`apply_startup_concurrency`：
        // DEVFORGE_MAX_CONCURRENT > store 持久值 > 缺省 2），本处只读 store。
        let capacity_ok = {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            store.running_run_count() + dispatched_this_tick < store.max_concurrent_runs.max(1)
        };
        if !capacity_ok {
            {
                let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = store.set_task_status(&project, &task_id, TaskStatus::Blocked) {
                    tracing::error!(
                        component = "scheduler",
                        project = %project,
                        task = %task_id,
                        error = %e,
                        "置 Blocked 失败（capacity）"
                    );
                }
            }
            report
                .blocked
                .push((format!("{project}/{task_id}"), "capacity".into()));
            continue;
        }
        // 解除阻塞态 → 派工计划（begin_run 在 run_in_background 内置 Running）。
        if status == TaskStatus::Blocked {
            {
                let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = store.set_task_status(&project, &task_id, TaskStatus::Pending) {
                    tracing::error!(
                        component = "scheduler",
                        project = %project,
                        task = %task_id,
                        error = %e,
                        "解除 Blocked 失败（派工前）"
                    );
                }
            }
        }
        report.dispatched.push((project.clone(), task_id.clone()));
        dispatched_this_tick += 1;
        if exclusive {
            exclusive_dispatched_this_tick = true;
        }
    }
    report
}

/// 派工执行（async：渲染闸门等待；run 本体仍在独立线程）。
pub async fn launch(st: &SharedState, project: &str, task_id: &str) {
    let sop = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.get_task(project, task_id) {
            Some(t) => t.sop.clone(),
            None => return,
        }
    };
    let required = crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(&sop);
    st.render_gate
        .wait_for_profiles(&st.profiles_root, required)
        .await;
    let mut engine = (st.engine_factory)();
    // P020-k5：agent 上报面注入 + issue 快照写黑板（调度器派发与 start 端点同口径）
    engine.set_issue_sink(Some(std::sync::Arc::new({
        let st_sink = st.clone();
        move |rep| crate::server::submit_agent_issue(&st_sink, rep)
    })));
    crate::server::write_issue_snapshot(st, project, task_id);
    // 准入在派发线程之前完成（与 start_task 同口径：202/派发不得早于 begin_run）；
    // 失败只打日志——调度器没有请求方可以回错了。
    let run = {
        let mut store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.begin_run(project, task_id) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    component = "scheduler",
                    project,
                    task = task_id,
                    error = %e,
                    "调度派工被准入拦下（任务状态/并发上限）"
                );
                // 自审 minor（233）：拦下要落事件（旧口径只有一条 warn，事后无法按字段重建）
                st.emit(serde_json::json!({
                    "type": "scheduler.dispatch_rejected",
                    "project": project,
                    "task": task_id,
                    "reason": e.to_string(),
                }));
                return;
            }
        }
    };
    let st2 = st.clone();
    // 自审 minor（223）：干预面在派发线程之前同步登记（与 start 端点同口径）。
    let control = st.register_control(project, task_id);
    let project = project.to_string();
    let task_id = task_id.to_string();
    std::thread::spawn(move || run_in_background(st2, project, task_id, engine, run, control));
}

/// 调度循环：1s 心跳看 kick，30s 周期全量 tick（serve 时 spawn；测试可直接调 tick）。
pub async fn scheduler_loop(st: SharedState, cfg: SchedulerConfig) {
    // 并发上限在**进程启动期**已解析并回写 store（`apply_startup_concurrency`：
    // DEVFORGE_MAX_CONCURRENT > store 持久值 > 缺省 2）。本循环只读 store，
    // 不再做「只抬不压」的启动同步，也不持有第二份并发事实源。
    // 自动集成消费循环（gap-E）：独立常驻任务——不能塞进 tick（tick 是纯决策且被
    // spawn_blocking 包着跑，全量 cargo test 是分钟级，塞进去会把超预算熔断一起冻住）。
    {
        let st_int = st.clone();
        tokio::spawn(async move { integration_loop(st_int).await });
    }
    let mut last = std::time::Instant::now() - Duration::from_secs(60);
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if !load_kick() && last.elapsed() < Duration::from_secs(30) {
            continue;
        }
        last = std::time::Instant::now();
        let st2 = st.clone();
        let cfg2 = cfg.clone();
        // 自审 major：tick panic 不再被 unwrap_or_default 静默吞成空报告——
        // 调度器看起来健康实则已死。panic 必须 ERROR 留痕，本轮跳过派工。
        let report = match tokio::task::spawn_blocking(move || tick(&st2, &cfg2)).await {
            Ok(report) => report,
            Err(err) => {
                if err.is_panic() {
                    tracing::error!(
                        component = "scheduler",
                        panic = %err,
                        "调度 tick panic——本轮跳过派工（调度循环存活）"
                    );
                } else {
                    tracing::error!(
                        component = "scheduler",
                        error = %err,
                        "调度 tick 任务异常结束——本轮跳过派工"
                    );
                }
                continue;
            }
        };
        // 自审 minor（234）：逐个 await launch 会让一个卡在渲染闸门的派工冻结整个调度循环
        // （连超预算熔断这条安全兜底一起冻住）。改成并发派发，循环不被单个派工拖住。
        for (project, task) in &report.dispatched {
            let st3 = st.clone();
            let project = project.clone();
            let task = task.clone();
            tokio::spawn(async move { launch(&st3, &project, &task).await });
        }
        if !report.dispatched.is_empty() || !report.blocked.is_empty() {
            tracing::info!(
                component = "scheduler",
                dispatched = report.dispatched.len(),
                blocked = report.blocked.len(),
                fused = report.fused.len(),
                enqueued = report.auto_enqueued.len(),
                "调度 tick 完成"
            );
        }
    }
}

/// status() 的一次性快照（锁内取、锁外用）。
/// 元组给别名：七元组直接写在签名里会被 clippy 判 `type_complexity`。
type StatusSnapshot = (
    Vec<serde_json::Value>, // running
    Vec<String>,            // runnable
    Vec<serde_json::Value>, // blocked
    Vec<String>,            // project_names
    usize,                  // effective_cap（生效的准入上限）
    Vec<serde_json::Value>, // channel_breakers（渠道熔断段）
);

/// 自动集成消费循环（gap-E）：周期 30s，**开关每轮重读**（改文件即热生效、无需重启）。
///
/// 口径：
/// - 开关关（缺省）→ 本轮零动作（连队列都不读）；
/// - 开关开 → 逐项目消费（每项目每轮最多 1 个候选），集成窗口期不派新 run；
/// - git / cargo 全在**锁外**跑（锁内只做快照——与 `status()` 的既有纪律一致）。
///
/// 集成在 `spawn_blocking` 里跑：全量 `cargo test` 是分钟级，不能占住 async 运行线程。
async fn integration_loop(st: SharedState) {
    loop {
        tokio::time::sleep(INTEGRATION_PERIOD).await;
        // 每轮重读开关（fail-closed：读不到 / 读坏了 = 全关）。
        let settings = crate::integration::settings::load(&crate::integration::settings::settings_path(
            &st.plans_root,
        ));
        if !settings.auto_integrate_enabled {
            continue;
        }
        // 项目列表（锁内快照，锁外消费）。
        let projects: Vec<String> = {
            let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
            store.projects.iter().map(|p| p.name.clone()).collect()
        };
        for project in projects {
            // 仓库路径：任务工作区 = 项目登记路径（锁内快照）。
            let repos: HashMap<String, PathBuf> = {
                let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
                store
                    .tasks
                    .iter()
                    .filter(|t| t.project == project)
                    .map(|t| (t.id.clone(), t.workspace.clone()))
                    .collect()
            };
            let plans_root = st.plans_root.clone();
            let project_for_gate = project.clone();
            // 窗口置位：集成期间不派新 run（错峰）。
            INTEGRATION_WINDOW.store(true, Ordering::Relaxed);
            let result = tokio::task::spawn_blocking(move || {
                let target_dir = plans_root
                    .join("integrations")
                    .join(&project_for_gate)
                    .join("cargo-target");
                crate::integration::consumer::drain_project(
                    &settings,
                    &project_for_gate,
                    &repos,
                    &plans_root,
                    |iw: &std::path::Path| {
                        // `IntegrationSettings` 是 Copy：内层闭包按值捕获一份（不借用外层状态）。
                        let gate_settings = settings;
                        let spec = crate::integration::gate::GateSpec::production(&gate_settings);
                        spec.run(iw, Some(target_dir.as_path())).map(|_report| ())
                    },
                )
            })
            .await;
            // 窗口清零（panic / 出错也必须清——否则 tick 永久停派）。
            INTEGRATION_WINDOW.store(false, Ordering::Relaxed);
            match result {
                Ok(Ok(report)) => emit_integration_events(&st, &project, report),
                Ok(Err(e)) => {
                    tracing::error!(
                        component = "scheduler",
                        event = "integration.drain.failed",
                        project = %project,
                        error = %e,
                        "自动集成消费失败（队列状态未动）"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        component = "scheduler",
                        event = "integration.drain.panic",
                        project = %project,
                        panic = %err,
                        "自动集成消费任务异常结束（队列状态未动）"
                    );
                }
            }
        }
    }
}

/// 自动集成事件（事件名落在既有 `integration.*` 族里——前端按前缀过滤不许漏）。
fn emit_integration_events(
    st: &SharedState,
    project: &str,
    report: crate::integration::consumer::DrainReport,
) {
    let Some(consumed) = report.consumed else {
        return;
    };
    use crate::integration::consumer::Consumed;
    match consumed {
        Consumed::Integrated {
            task_id,
            merged_sha,
        } => st.emit(serde_json::json!({
            "type": "integration.integrated",
            "project": project,
            "task": task_id,
            "merged_sha": merged_sha,
        })),
        Consumed::AlreadyIntegrated {
            task_id,
            integrated_sha,
        } => st.emit(serde_json::json!({
            "type": "integration.already_integrated",
            "project": project,
            "task": task_id,
            "integrated_sha": integrated_sha,
        })),
        Consumed::NeedsRepair {
            task_id,
            reason,
            artifact,
        } => st.emit(serde_json::json!({
            "type": "integration.needs_repair",
            "project": project,
            "task": task_id,
            "reason": reason,
            "artifact": artifact.to_string_lossy(),
        })),
        Consumed::InfraRetry {
            task_id,
            reason,
            infra_failures,
        } => st.emit(serde_json::json!({
            "type": "integration.infra_retry",
            "project": project,
            "task": task_id,
            "reason": reason,
            "infra_failures": infra_failures,
        })),
        Consumed::NeedsRebase {
            task_id,
            target_moved_to,
        } => st.emit(serde_json::json!({
            "type": "integration.needs_rebase",
            "project": project,
            "task": task_id,
            "target_moved_to": target_moved_to,
        })),
    }
}

/// M5 · T5.1：调度态只读快照（GET /scheduler/status）——
/// 不登机器纯 API 回答「现在几个 worktree 在飞、各自卡在哪、队列多长、冲突了没有」。
/// 判定逻辑与 tick 同款但零副作用（不置状态不派工）。
///
/// 并发字段：`max_concurrent_runs` 与 `effective_max_concurrent` **同报生效值**
/// （同一把锁内读出的同一个数：store 的 `max_concurrent_runs`，0 视为 1）。
/// `ServerState.effective_max_concurrent` 已废弃——调度器不再读写它。
pub fn status(st: &SharedState) -> serde_json::Value {
    // 集成窗口态（进程级原子——与 tick 同一事实源）。
    let integration_in_flight = integration_window_in_flight();
    // 自动集成开关（读配置面——与消费循环同一份、同一重读口径）。
    let integration_settings = crate::integration::settings::load(&crate::integration::settings::settings_path(
        &st.plans_root,
    ));
    // P020：issues 计数（纯内存短锁——open 总数 + 按严重度分布，Dashboard 面板源）。
    let issue_stats = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let open = store
            .issues
            .iter()
            .filter(|i| i.status == crate::domain::IssueStatus::Open)
            .collect::<Vec<_>>();
        let by = |s: crate::domain::IssueSeverity| {
            open.iter().filter(|i| i.severity == s).count()
        };
        serde_json::json!({
            "open": open.len(),
            "by_severity": {
                "info": by(crate::domain::IssueSeverity::Info),
                "minor": by(crate::domain::IssueSeverity::Minor),
                "major": by(crate::domain::IssueSeverity::Major),
                "critical": by(crate::domain::IssueSeverity::Critical),
            },
        })
    };
    // 渠道派生映射（清单 → sop/agent → 渠道）在进 store 锁**之前**算好：
    // 锁内再取 manifest 就是「store → manifest」反向嵌套，与 PUT /config/manifest
    // （先锁 manifest）构成锁序环（风险 R5）。锁内只查算好的快照。
    let channel_index = {
        let manifest = st.manifest.lock().unwrap_or_else(|e| e.into_inner());
        ChannelIndex::build(&manifest, &st.registry.ids())
    };
    let now = now_ms();
    // 自审 minor（236）：旧口径全程持 store 互斥锁、并跨越逐项目队列文件 I/O——
    // 把全部 store 操作（所有 API handler）堵在磁盘读后面。现在锁内只做快照，
    // 放开锁之后再做文件 I/O（与 tick 的「先放锁再做 I/O」纪律一致）。
    let (
        running,
        runnable,
        blocked,
        project_names,
        effective_cap,
        channel_breakers,
    ): StatusSnapshot = {
        let store = st.store.lock().unwrap_or_else(|e| e.into_inner());
        let running: Vec<serde_json::Value> = store
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .map(|r| {
                serde_json::json!({
                    "project": r.task_project,
                    "task": r.task_id,
                    "run_id": r.id.0,
                    "elapsed_ms": now_ms().saturating_sub(r.started_at_ms),
                })
            })
            .collect();
        let exclusive_running = store
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Running && t.exclusive);
        // 生效并发值：store 是并发的唯一事实源（启动期已按优先级链解析并回写），
        // 0 视为 1（与 `Store::begin_run` 的 cap 同口径）。
        let effective_cap = store.max_concurrent_runs.max(1);
        let capacity_full = store.running_run_count() >= effective_cap;

        let mut runnable: Vec<String> = Vec::new();
        let mut blocked: Vec<serde_json::Value> = Vec::new();
        for t in &store.tasks {
            let key = format!("{}/{}", t.project, t.id);
            let reason = block_reason(
                &store,
                t,
                exclusive_running,
                capacity_full,
                integration_in_flight,
                &channel_index,
                now,
            );
            match (t.status, reason) {
                (TaskStatus::Blocked, Some(r)) => {
                    blocked.push(serde_json::json!({"task": key, "reason": r}))
                }
                // Blocked 但条件已解 → 下轮 tick 恢复（与 tick 的判定同源）
                (TaskStatus::Blocked, None) => blocked
                    .push(serde_json::json!({"task": key, "reason": "recovering"})),
                (TaskStatus::Pending, Some(r)) => {
                    blocked.push(serde_json::json!({"task": key, "reason": r}))
                }
                (TaskStatus::Pending, None) => runnable.push(key),
                _ => {}
            }
        }
        let project_names: Vec<String> = store.projects.iter().map(|p| p.name.clone()).collect();
        // 渠道熔断段（运维不进机器也能看见「哪条渠道在冷却、还有多久、为什么」）。
        let channel_breakers: Vec<serde_json::Value> = store
            .active_channel_breakers(now)
            .into_iter()
            .map(|b| {
                serde_json::json!({
                    "channel": b.channel,
                    "remaining_secs": remaining_secs(b.expires_at_ms.saturating_sub(now)),
                    "expires_at_ms": b.expires_at_ms,
                    "last_seen_reason": b.last_seen_reason,
                })
            })
            .collect();
        (
            running,
            runnable,
            blocked,
            project_names,
            effective_cap,
            channel_breakers,
        )
    };

    // 集成队列：每项目的 Pending/Integrated/NeedsRebase/NeedsRepair 计数（锁已释放）
    let mut queue: Vec<serde_json::Value> = Vec::new();
    for name in &project_names {
        let q = match MergeQueue::load(&st.plans_root, name) {
            Ok(q) => q,
            Err(e) => {
                // 自审 minor（237）：旧口径 `.ok()?` 吞掉加载失败——损坏队列在 API 里表现为
                // 「该项目没有队列」。现在留痕并在响应里显式标注。
                tracing::warn!(
                    component = "scheduler",
                    project = %name,
                    error = %e,
                    "调度状态：集成队列不可读（按 error 标注，不静默当成空队）"
                );
                queue.push(serde_json::json!({
                    "project": name,
                    "error": e.to_string(),
                }));
                continue;
            }
        };
        let count = |f: &dyn Fn(&CandidateStatus) -> bool| {
            q.candidates.iter().filter(|c| f(&c.status)).count()
        };
        queue.push(serde_json::json!({
            "project": name,
            "total": q.candidates.len(),
            "pending": count(&|s| matches!(s, CandidateStatus::Pending)),
            "integrated": count(&|s| matches!(s, CandidateStatus::Integrated { .. })),
            "needs_rebase": count(&|s| matches!(s, CandidateStatus::NeedsRebase { .. })),
            "needs_repair": count(&|s| matches!(s, CandidateStatus::NeedsRepair { .. })),
        }));
    }

    serde_json::json!({
        "running": running,
        "runnable": runnable,
        "blocked": blocked,
        "queue": queue,
        "max_concurrent_runs": effective_cap,
        "effective_max_concurrent": effective_cap,
        "channel_breakers": channel_breakers,
        // P020：issues 面板源（open 总数 + 按严重度分布）。
        "issues": issue_stats,
        // 自动集成面（gap-E）：开关态 + 窗口态——运营面要能回答「开关开了吗 / 现在在集成吗」。
        "integration": {
            "auto_integrate_enabled": integration_settings.auto_integrate_enabled,
            "window_in_flight": integration_in_flight,
            "target_ref": crate::integration::settings::TARGET_REF,
            "gates": {
                "clippy": integration_settings.gates.clippy,
                "doc_lint": integration_settings.gates.doc_lint,
            },
        },
    })
}

// ---------- 渠道熔断（gap-B · 派发前渠道熔断） ----------

/// 渠道事故签名 → 冷却 TTL 分档（毫秒）。
///
/// 三档取自前置裁决：账户/凭证类 10 分钟硬熔断、限流类 60 秒短档、连接错误类 10 分钟硬熔断。
/// 同一条 reason 同时命中多档时取**最长**档——保守方向：宁冷却久一点，不重复烧一轮。
pub(crate) const CHANNEL_BREAKER_TTL_CREDENTIAL_MS: u64 = 600_000;
pub(crate) const CHANNEL_BREAKER_TTL_RATE_LIMIT_MS: u64 = 60_000;
pub(crate) const CHANNEL_BREAKER_TTL_CONNECTION_MS: u64 = 600_000;

/// 扫描窗口基准 = 三档里的**最大** TTL。
///
/// 取最大档（10 分钟）而不是最小档：取 60 秒会把 402 这类长档失败整批漏掉；
/// 上限约束是「重启后不要把陈旧失败重放成新熔断」（风险 R4）。
pub(crate) const CHANNEL_BREAKER_MAX_TTL_MS: u64 = 600_000;

/// 显式连接错误标记（只收这些，**不收裸 timeout / timed out**）。
///
/// 仓内 `timeout` 既表示「上游连不上」也表示「等 run 超时 / 批次墙钟到点」
/// （`src/engine/client.rs` 的瞬态标记表里就混着），后者不是渠道死亡——
/// 收进来会把健康的慢渠道按 10 分钟熔断掉。
const CONNECTION_ERROR_MARKS: [&str; 7] = [
    "econnreset",
    "econnrefused",
    "etimedout",
    "connection reset",
    "connection refused",
    "connection closed",
    "broken pipe",
];

/// 渠道事故签名 → 命中的冷却 TTL 档（None = 未命中任何签名 = 不是渠道事故）。
///
/// 与 `src/engine/client.rs` 的 `transient_failure` **故意不同口径**：
/// - 那边按「独立数字 token」判瞬态，代价是内部错误分类（漏判无伤）；
/// - 这边是熔断，代价是**错杀健康渠道 10 分钟**（误伤面比漏判大一个量级），
///   所以额外排除「路径 + 行号」「版本号」形态（`src/a.rs:402` / `v1.402` /
///   `src/a.rs 402`）——状态码必须独立成词才算数。
pub(crate) fn channel_incident_ttl_ms(reason: &str) -> Option<u64> {
    let mut ttl: Option<u64> = None;
    // ① 账户/凭证类：402 / 401 / 403 / Insufficient Balance / No API key found。
    if has_status_code(reason, &["402", "401", "403"])
        || contains_ignore_case(reason, "insufficient balance")
        || contains_ignore_case(reason, "no api key found")
    {
        ttl = longer_ttl(ttl, CHANNEL_BREAKER_TTL_CREDENTIAL_MS);
    }
    // ② 限流类：429（瞬时故障，短冷却自动恢复）。
    if has_status_code(reason, &["429"]) {
        ttl = longer_ttl(ttl, CHANNEL_BREAKER_TTL_RATE_LIMIT_MS);
    }
    // ③ 连接错误类：只收显式连接标记（不收裸 timeout——见 CONNECTION_ERROR_MARKS）。
    if CONNECTION_ERROR_MARKS
        .iter()
        .any(|m| contains_ignore_case(reason, m))
    {
        ttl = longer_ttl(ttl, CHANNEL_BREAKER_TTL_CONNECTION_MS);
    }
    ttl
}

/// 两个 TTL 档取较长者（None 视作 0）。
fn longer_ttl(a: Option<u64>, b: u64) -> Option<u64> {
    Some(a.map_or(b, |cur| cur.max(b)))
}

/// reason 里是否出现「独立成词」的状态码。
fn has_status_code(reason: &str, codes: &[&str]) -> bool {
    let chars: Vec<char> = reason.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();
        if codes.contains(&token.as_str()) && !is_path_position(&chars, start) {
            return true;
        }
    }
    false
}

/// 数字 token 是不是「路径 + 行号 / 版本号」的一部分（这种形态不是状态码）。
///
/// 两种排除：
/// ① 紧跟在 `:` `.` `/` `-` 之后（`src/a.rs:402` / `v1.402` / `a/402` / `x-402`）；
/// ② 紧跟在一个「像文件名」的词之后（`src/a.rs 402`）——文件名带字母扩展名
///    （`.rs` / `.py`）。`HTTP/1.1` 这种小数不算文件名（否则 `HTTP/1.1 402` 会被误排除）。
fn is_path_position(chars: &[char], start: usize) -> bool {
    if start == 0 {
        return false;
    }
    if matches!(chars[start - 1], ':' | '.' | '/' | '-') {
        return true;
    }
    // ② 向前取**紧邻的上一个词**（跳过空白），看它是不是文件名。
    let mut end = start;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    if end == start {
        return false; // 数字前面没有词（行首或纯标点）
    }
    let mut begin = end;
    while begin > 0 && !chars[begin - 1].is_whitespace() {
        begin -= 1;
    }
    let word: String = chars[begin..end].iter().collect();
    match word.rsplit_once('.') {
        Some((_, ext)) => !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic()),
        None => false,
    }
}

/// 大小写无关的子串包含（needle 必须已小写）。
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(needle)
}

/// 从失败原因里抽取 agent 身份（归因锚点，两种文案形态）：
/// - `agent[<role>] 命令 … 失败：…`（`src/engine/client.rs` 的命令失败文案）；
/// - `<role> 连续 N 次未输出 …`（`src/change/session.rs` 的质量失败文案）。
///
/// 抽不出 → None（调用方按「不可归因」处置：只记 WARN，不熔断——宁可漏熔，
/// 不可错杀健康渠道导致全线饿死）。
pub(crate) fn extract_agent_role(reason: &str) -> Option<String> {
    if let Some(open) = reason.find("agent[") {
        let rest = &reason[open + "agent[".len()..];
        if let Some(close) = rest.find(']') {
            let role = rest[..close].trim();
            if !role.is_empty() {
                return Some(role.to_string());
            }
        }
    }
    // 段首形态：`<role> 连续 N 次未输出`
    let tokens: Vec<&str> = reason.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[1] == "连续" && looks_like_agent_name(pair[0]) {
            return Some(pair[0].to_string());
        }
    }
    None
}

/// 像个 agent 名（kebab-case 标识符；排除中文词与普通英文单词）。
fn looks_like_agent_name(s: &str) -> bool {
    !s.is_empty()
        && s.contains('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !s.starts_with('-')
}

/// 渠道派生索引（live 清单的只读派生）：agent 名 → 渠道（model 池条目 id）、
/// sop → 该 sop 引用的渠道列表（清单顺序）。
///
/// 事实源：`AgentManifest.model` 就是 model 池条目 id（`src/config/manifest.rs`），
/// `BuiltinRpcEngine::profiles_for_sop` 是 sop → 角色清单的唯一事实源。
/// 本索引**只在 store 锁之外构造**（锁序：manifest → store 单向，见风险 R5）。
pub(crate) struct ChannelIndex {
    agent_to_channel: HashMap<String, String>,
    sop_to_channels: HashMap<String, Vec<String>>,
}

impl ChannelIndex {
    pub(crate) fn build(manifest: &ConfigManifest, sops: &[&str]) -> Self {
        let mut agent_to_channel = HashMap::new();
        for agent in &manifest.agents {
            // 池条目必须真实存在：清单里指向不存在条目的 agent 不算「有渠道」
            // （否则归因会指向一条根本不存在的渠道）。
            if manifest.model_pool.iter().any(|m| m.id == agent.model) {
                agent_to_channel.insert(agent.name.clone(), agent.model.clone());
            }
        }
        let mut sop_to_channels = HashMap::new();
        for sop in sops {
            let profiles = crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(sop);
            if profiles.is_empty() {
                continue;
            }
            let mut channels: Vec<String> = Vec::new();
            for agent in &manifest.agents {
                // 清单顺序：多个渠道同时在冷却时，报清单顺序第一个（对账口径固定）。
                if profiles.contains(&agent.name.as_str())
                    && let Some(ch) = agent_to_channel.get(&agent.name)
                    && !channels.contains(ch)
                {
                    channels.push(ch.clone());
                }
            }
            sop_to_channels.insert((*sop).to_string(), channels);
        }
        Self {
            agent_to_channel,
            sop_to_channels,
        }
    }

    /// agent 名 → 渠道（None = 清单里没这个 agent，或它没指向有效池条目）。
    pub(crate) fn channel_of(&self, agent: &str) -> Option<&str> {
        self.agent_to_channel.get(agent).map(|s| s.as_str())
    }

    /// 该 sop 引用的渠道列表（清单顺序；去重）。
    pub(crate) fn channels_for_sop(&self, sop: &str) -> &[String] {
        self.sop_to_channels
            .get(sop)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// 任务引用的渠道里第一个处于冷却中的（清单顺序第一个）→ (渠道, 剩余毫秒)。
///
/// 「清单顺序第一个」是固定口径：多个渠道同时在冷却时，报哪一个不许漂（对账面）。
/// 剩余毫秒 0 = 已到期（不再算冷却）。
pub(crate) fn cooled_channel(
    index: &ChannelIndex,
    sop: &str,
    breakers: &[crate::domain::store::ChannelBreaker],
    now_ms: u64,
) -> Option<(String, u64)> {
    for channel in index.channels_for_sop(sop) {
        if let Some(b) = breakers.iter().find(|b| &b.channel == channel) {
            let remaining = b.expires_at_ms.saturating_sub(now_ms);
            if remaining > 0 {
                return Some((channel.clone(), remaining));
            }
        }
    }
    None
}

/// 剩余毫秒 → 剩余秒数（向上取整，最小 1）——理由串与事件里给运维看的口径。
pub(crate) fn remaining_secs(remaining_ms: u64) -> u64 {
    remaining_ms.saturating_add(999) / 1000
}

/// 阻塞理由判定（**单一事实源**，自审 minor 131/235）：tick 与 status 都调它，
/// 两套语义不许漂移。返回 None = 条件已解（可派工 / 待恢复）。
///
/// 返回 `Option<String>`（原 `Option<&'static str>`）：渠道冷却的理由串带参数
/// （渠道 id + 剩余秒数），静态 str 装不下。其余四条理由串**原文不动**
/// （`dependency_failed` / `dependency_pending` / `exclusive_window` / `capacity`
/// 已被 status 与 UI 当词表用）。
///
/// 判定顺序与 tick 的闸门顺序同源：集成窗口 → 依赖 → 渠道冷却 → 独占 → 容量。
///
/// 集成窗口优先级最高（与 tick 的窗口旁路同位置）：窗口期一律不派工，
/// 任务状态不变（瞬态现象不写进 store）——窗口关掉后下轮 tick 恢复细粒度理由。
fn block_reason(
    store: &crate::domain::store::Store,
    t: &crate::domain::task::Task,
    exclusive_running: bool,
    capacity_full: bool,
    integration_in_flight: bool,
    channels: &ChannelIndex,
    now_ms: u64,
) -> Option<String> {
    if integration_in_flight {
        Some("integration_window".into())
    } else if deps_failed(store, t) {
        Some("dependency_failed".into())
    } else if !deps_ready(store, t) {
        // 缺依赖但未失败：等待中（tick 静默等；status 也要显式报出来，不许隐形）
        Some("dependency_pending".into())
    } else if let Some((channel, remaining_ms)) =
        cooled_channel(channels, &t.sop, &store.channel_breakers, now_ms)
    {
        Some(format!(
            "channel_cooldown({channel},{}s)",
            remaining_secs(remaining_ms)
        ))
    } else if exclusive_running {
        Some("exclusive_window".into())
    } else if capacity_full {
        Some("capacity".into())
    } else {
        None
    }
}

fn deps_failed(store: &crate::domain::store::Store, t: &crate::domain::task::Task) -> bool {
    t.depends_on.iter().any(|d| {
        store
            .tasks
            .iter()
            .find(|x| x.project == t.project && x.id == *d)
            .map(|x| x.status == TaskStatus::Failed)
            .unwrap_or(false)
    })
}

fn deps_ready(store: &crate::domain::store::Store, t: &crate::domain::task::Task) -> bool {
    t.depends_on.iter().all(|d| {
        store
            .tasks
            .iter()
            .find(|x| x.project == t.project && x.id == *d)
            .map(|x| x.status == TaskStatus::Completed)
            .unwrap_or(false)
    })
}
