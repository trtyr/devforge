//! 依赖图调度器 + 准入控制（P012 M4，topic 06）。
//!
//! 调度循环（`scheduler_loop`）：周期 + kick 即时地跑 `tick`——
//! ① 超预算熔断（cancel 置位，引擎按失败收尾）；
//! ② Settled 且带契约的任务自动入集成队列（M3 MergeQueue，闭环自动化）；
//! ③ 就绪判定（depends_on 全 Completed）→ 准入（并发上限/独占窗口）→ 派工计划；
//! ④ blocked 语义：capacity / exclusive_window / dependency_failed，恢复联动自动迁出。
//!
//! 派工决策（tick，纯函数可测）与派工执行（`launch`，async：渲染闸门等待）分离。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::domain::run::RunStatus;
use crate::domain::task::TaskStatus;
use crate::domain::now_ms;
use crate::integration::{Candidate, CandidateStatus, MergeQueue};
use crate::server::{run_in_background, SharedState};

/// 准入配置（topic 06 准入表）。
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// 并发 mutation 上限（默认 2：worktree 隔离下两路并行；下游组合验证是瓶颈）。
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

/// 单次调度报告（tick 的纯决策产物；dispatched 的实际 launch 由循环执行）。
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct TickReport {
    /// 派工计划：`project/task`（循环据此 launch）。
    pub dispatched: Vec<String>,
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

/// 单次调度（纯决策：改状态/入队/发事件；launch 由调用方执行）。
pub fn tick(st: &SharedState, cfg: &SchedulerConfig) -> TickReport {
    let mut report = TickReport::default();

    // ① 超预算熔断：Running 且 started_at + budget < now → cancel 置位 + 台账强杀。
    // 复验归因：cancel 是协作式（SOP 轮询间隙检查），pi 子进程工具挂死时无人消费，
    // run 永驻 Running（M3 挂死 14h 的直接教训）——必须台账精确 SIGKILL。
    let to_fuse: Vec<(String, String, String)> = {
        let store = st.store.lock().expect("store 锁");
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
                .collect()
        }
    };
    for (project, task, run_id) in &to_fuse {
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
            .expect("runs_control 锁")
            .get(&format!("{project}/{task}"))
            .cloned();
        if let Some(c) = control {
            c.cancel();
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

    // ② 自动入集成队列：Completed + 带契约 + frozen-commit 落盘 + 队列内无此任务。
    let settled: Vec<(String, String)> = {
        let store = st.store.lock().expect("store 锁");
        store
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed && t.contract.is_some())
            .map(|t| (t.project.clone(), t.id.clone()))
            .collect()
    };
    for (project, task_id) in settled {
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
        let mut queue = match MergeQueue::load(&st.plans_root, &project) {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!(component = "scheduler", error = %e, "队列加载失败，跳过入队");
                continue;
            }
        };
        // 全仓审计 #2：NeedsRebase 不是终态——语义是「目标在集成期间被推进，
        // rebase 到新目标后重验」。旧实现 next_pending 只挑 Pending，一次 CAS 失败
        // 就把任务永久卡死（scheduler 又「任意状态都跳过入队」）。这里把 NeedsRebase
        // 放回 Pending 重验；NeedsRepair 保持终态（修复 = 新候选带新证据 = 新 frozen_sha）。
        let mut rearmed = false;
        for c in queue.candidates.iter_mut() {
            if let CandidateStatus::NeedsRebase { target_moved_to } = &c.status {
                tracing::warn!(
                    component = "scheduler",
                    project = %project,
                    task = %c.task_id,
                    target = %target_moved_to,
                    "NeedsRebase 候选放回 Pending 重验（终态不停滞）"
                );
                c.status = CandidateStatus::Pending;
                rearmed = true;
            }
        }
        if rearmed && let Err(e) = queue.save() {
            tracing::warn!(component = "scheduler", error = %e, "重排候选回写失败");
        }
        // 去重口径（全仓审计 #2）：Pending/Integrated = 本任务已在队或已合入；
        // NeedsRepair 只在 frozen_sha 未变时跳过（同一批失败重投只会再失败一次），
        // 任务重跑产生新 frozen_sha 即视为「新证据」允许入队——不再静默停滞。
        let already = queue.candidates.iter().any(|c| {
            c.task_id == task_id
                && match &c.status {
                    CandidateStatus::Pending | CandidateStatus::Integrated { .. } => true,
                    CandidateStatus::NeedsRebase { .. } => true,
                    CandidateStatus::NeedsRepair { .. } => c.frozen_sha == sha,
                }
        });
        if already {
            continue;
        }
        let contract = {
            let store = st.store.lock().expect("store 锁");
            store
                .get_task(&project, &task_id)
                .and_then(|t| t.contract.clone())
        };
        let Some(contract) = contract else { continue };
        queue.enqueue(Candidate {
            task_id: task_id.clone(),
            frozen_sha: sha,
            base_rev: contract.base_rev.clone().unwrap_or_default(),
            owned_paths: contract.owned_paths.clone(),
            status: CandidateStatus::Pending,
            evidence_paths: vec![],
        });
        if queue.save().is_ok() {
            report.auto_enqueued.push(format!("{project}/{task_id}"));
            st.emit(serde_json::json!({
                "type": "integration.auto_enqueued",
                "project": project,
                "task": task_id,
            }));
        }
    }

    // ③ 就绪判定 + 准入 → 派工计划。
    let snapshot: Vec<(String, String, TaskStatus, Vec<String>, bool)> = {
        let store = st.store.lock().expect("store 锁");
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
                )
            })
            .collect()
    };
    let exclusive_running = {
        let store = st.store.lock().expect("store 锁");
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
    for (project, task_id, status, deps, exclusive) in snapshot {
        if !matches!(status, TaskStatus::Pending | TaskStatus::Blocked) {
            continue;
        }
        // 依赖判定：任一 Failed → blocked{dependency_failed}；任一未完成 → 继续等。
        {
            let store = st.store.lock().expect("store 锁");
            if deps
                .iter()
                .any(|d| dep_status(&store, &project, d) == TaskStatus::Failed)
            {
                drop(store);
                let mut store = st.store.lock().expect("store 锁");
                let _ = store.set_task_status(&project, &task_id, TaskStatus::Blocked);
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
        // 独占窗口：已有独占任务在跑，或本 tick 已派工过独占任务（复验 #3：
        // running 集合在后台线程 begin_run 前不变，不追踪本 tick 派工就会同 tick 双独占并行）。
        if exclusive_running || exclusive_dispatched_this_tick {
            let _ = {
                let mut store = st.store.lock().expect("store 锁");
                store.set_task_status(&project, &task_id, TaskStatus::Blocked)
            };
            report.blocked.push((
                format!("{project}/{task_id}"),
                "exclusive_window".into(),
            ));
            continue;
        }
        // 容量：running + 本 tick 已派工（预留）< min(准入上限, store 硬上限)——
        // begin_run 发生在后台线程，tick 内 running_run_count 不变，必须本地预留。
        let capacity_ok = {
            let store = st.store.lock().expect("store 锁");
            store.running_run_count() + dispatched_this_tick
                < cfg
                    .max_concurrent_mutation
                    .min(store.max_concurrent_runs.max(1))
        };
        if !capacity_ok {
            let _ = {
                let mut store = st.store.lock().expect("store 锁");
                store.set_task_status(&project, &task_id, TaskStatus::Blocked)
            };
            report
                .blocked
                .push((format!("{project}/{task_id}"), "capacity".into()));
            continue;
        }
        // 解除阻塞态 → 派工计划（begin_run 在 run_in_background 内置 Running）。
        if status == TaskStatus::Blocked {
            let _ = {
                let mut store = st.store.lock().expect("store 锁");
                store.set_task_status(&project, &task_id, TaskStatus::Pending)
            };
        }
        report.dispatched.push(format!("{project}/{task_id}"));
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
        let store = st.store.lock().expect("store 锁");
        match store.get_task(project, task_id) {
            Some(t) => t.sop.clone(),
            None => return,
        }
    };
    let required = crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(&sop);
    st.render_gate
        .wait_for_profiles(&st.profiles_root, required)
        .await;
    let engine = (st.engine_factory)();
    // 准入在派发线程之前完成（与 start_task 同口径：202/派发不得早于 begin_run）；
    // 失败只打日志——调度器没有请求方可以回错了。
    let run = {
        let mut store = st.store.lock().expect("store 锁");
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
                return;
            }
        }
    };
    let st2 = st.clone();
    let project = project.to_string();
    let task_id = task_id.to_string();
    std::thread::spawn(move || run_in_background(st2, project, task_id, engine, run));
}

/// 调度循环：1s 心跳看 kick，30s 周期全量 tick（serve 时 spawn；测试可直接调 tick）。
pub async fn scheduler_loop(st: SharedState, cfg: SchedulerConfig) {
    // 启动同步（复验 #3）：store 硬上限 ≥ 准入上限，否则 min() 恒等准入上限被 store 压平，
    // 并发永不开闸（默认 1×1）。取 max：用户手动调大的配置不被压回。
    {
        let mut store = st.store.lock().expect("store 锁");
        store.max_concurrent_runs = store.max_concurrent_runs.max(cfg.max_concurrent_mutation);
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
        let report = tokio::task::spawn_blocking(move || tick(&st2, &cfg2))
            .await
            .unwrap_or_default();
        for plan in &report.dispatched {
            let (project, task) = plan.split_once('/').unwrap_or((plan.as_str(), ""));
            launch(&st, project, task).await;
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

/// M5 · T5.1：调度态只读快照（GET /scheduler/status）——
/// 不登机器纯 API 回答「现在几个 worktree 在飞、各自卡在哪、队列多长、冲突了没有」。
/// 判定逻辑与 tick 同款但零副作用（不置状态不派工）。
pub fn status(st: &SharedState) -> serde_json::Value {
    let store = st.store.lock().expect("store 锁");

    // 在飞：Running runs + 墙钟
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
    let capacity_full = store.running_run_count() >= store.max_concurrent_runs.max(1);

    let mut runnable: Vec<String> = Vec::new();
    let mut blocked: Vec<serde_json::Value> = Vec::new();
    for t in &store.tasks {
        let key = format!("{}/{}", t.project, t.id);
        match t.status {
            TaskStatus::Blocked => {
                // 理由只读重算（与 tick 同款判定序）
                let reason = if deps_failed(&store, t) {
                    "dependency_failed"
                } else if exclusive_running {
                    "exclusive_window"
                } else if capacity_full {
                    "capacity"
                } else {
                    "recovering" // Blocked 但条件已解——下轮 tick 恢复
                };
                blocked.push(serde_json::json!({"task": key, "reason": reason}));
            }
            TaskStatus::Pending => {
                if deps_failed(&store, t) {
                    blocked.push(serde_json::json!({
                        "task": key,
                        "reason": "dependency_failed",
                    }));
                } else if deps_ready(&store, t) {
                    if exclusive_running {
                        blocked.push(serde_json::json!({
                            "task": key,
                            "reason": "exclusive_window",
                        }));
                    } else if capacity_full {
                        blocked.push(serde_json::json!({"task": key, "reason": "capacity"}));
                    } else {
                        runnable.push(key);
                    }
                }
                // 依赖未就绪且未失败：等待中——不入 blocked（与 tick 的静默等待一致）
            }
            _ => {}
        }
    }

    // 集成队列：每项目的 Pending/Integrated/NeedsRebase/NeedsRepair 计数（NeedsRepair = 冲突面）
    let queue: Vec<serde_json::Value> = store
        .projects
        .iter()
        .filter_map(|p| {
            let q = MergeQueue::load(&st.plans_root, &p.name).ok()?;
            let count = |f: &dyn Fn(&CandidateStatus) -> bool| {
                q.candidates.iter().filter(|c| f(&c.status)).count()
            };
            Some(serde_json::json!({
                "project": p.name,
                "total": q.candidates.len(),
                "pending": count(&|s| matches!(s, CandidateStatus::Pending)),
                "integrated": count(&|s| matches!(s, CandidateStatus::Integrated { .. })),
                "needs_rebase": count(&|s| matches!(s, CandidateStatus::NeedsRebase { .. })),
                "needs_repair": count(&|s| matches!(s, CandidateStatus::NeedsRepair { .. })),
            }))
        })
        .collect();

    serde_json::json!({
        "running": running,
        "runnable": runnable,
        "blocked": blocked,
        "queue": queue,
        "max_concurrent_runs": store.max_concurrent_runs,
    })
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
