//! API 服务器（P003 · 服务器形态，非 CLI）。
//!
//! 协议定案（P003 decisions/001）：REST（编排控制/产物/配置/中继输入）+ SSE（事件推送），
//! 路径前缀 `/api/v1` 从第一天；默认绑 127.0.0.1，token 预留位。
//!
//! 五能力域：编排控制 · 产物管理 · 交互中继 · 可观测 · 配置管理。

pub mod change_engine;
pub mod gate;
pub mod handlers;
pub mod http;
pub mod scheduler;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

pub use gate::RenderGate;
pub use http::{build_router, parse_token_flag, resolve_startup_token, serve, TOKEN_ENV};

use crate::config::installer::PackageInstaller;
use crate::config::ConfigManifest;
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::error::Result;
use crate::observability::events::EventBus;
use crate::observability::watchdog::run_with_watchdog;
use crate::sop::planning::UserLink;


// ---------- UI 中继（交互中继域） ----------

/// 待答请求表的值：(应答通道, 归属 run id)
///
/// 自审 minor（48）：旧口径把这段嵌套泛型直接写在字段上（clippy::type_complexity），
/// 与本文件顶部导入的 `std::sync::*` 风格也不一致。
type PendingAsk = (std::sync::mpsc::Sender<String>, Option<String>);

/// ask-user-question 的 API 中继：请求推 SSE，应答经 POST 端点回填。
pub struct UiRelay {
    next_id: AtomicU64,
    bus: std::sync::Arc<EventBus>,
    pending: Mutex<HashMap<String, PendingAsk>>,
}

impl UiRelay {
    /// 事件通道：ui.request 与 run 事件同流（单一 SSE 通道，Q002 信封设计）。
    pub fn new(bus: std::sync::Arc<EventBus>) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            bus,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 阻塞等待用户应答（在引擎的同步线程里调用）。
    pub fn ask_blocking(&self, question: &str, options: &[&str]) -> Result<String> {
        // 自审 critical：显式墙钟旋钮（0/未设 = 不限，零硬限制哲学）；干预面
        // （cancel 端点 / 墙钟超时 / 失活收割）由 watchdog 的解阻钩子兜底。
        let timeout_ms = std::env::var("DEVFORGE_UI_ASK_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|ms| *ms > 0);
        self.ask_blocking_with_timeout(
            question,
            options,
            timeout_ms.map(std::time::Duration::from_millis),
        )
    }

    /// 内层实现（超时注入便于测试）：见 [`Self::ask_blocking`]。
    pub(crate) fn ask_blocking_with_timeout(
        &self,
        question: &str,
        options: &[&str],
        timeout: Option<std::time::Duration>,
    ) -> Result<String> {
        let id = format!("ui-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = std::sync::mpsc::channel();
        // server #3：记下发起方 run——cancel 只能解阻**自己那个 run** 的待答，
        // 不能把并发 run（max_concurrent_runs > 1）的 dialog 一起打掉。
        let owner = crate::observability::current_run_id();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), (tx, owner));
        // 先登记后广播：应答端点在事件可见后仍能命中 pending
        self.bus.emit(json!({
            "type": "ui.request",
            "id": id,
            "question": question,
            "options": options
        }));
        // 自审 critical：无超时的 recv 会把 run 线程永久挂在无人应答的问题上——
        // run 永不落终态、串行槽占死、后续 start 一律 409。
        let outcome = match timeout {
            Some(d) => rx.recv_timeout(d).map_err(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => {
                    crate::error::DevforgeError::System(format!(
                        "UI 应答超时——问题 {id} 已失效，run 按干预收场"
                    ))
                }
                std::sync::mpsc::RecvTimeoutError::Disconnected => crate::error::DevforgeError::System(
                    "等待 UI 应答: 应答通道已关闭（被中止）".into(),
                ),
            }),
            None => rx
                .recv()
                .map_err(|e| crate::error::DevforgeError::System(format!("等待 UI 应答: {e}"))),
        };
        if outcome.is_err() {
            // 超时路径清残条目：迟到应答拿到干净的「无此待答」，不会悄悄发进虚空
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
        outcome
    }

    /// 回填应答（POST /ui/{id}/answer 调用）。
    pub fn answer(&self, id: &str, value: impl Into<String>) -> Result<()> {
        let (tx, _owner) = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
            .ok_or_else(|| {
                crate::error::DevforgeError::Store(format!("无此待答 UI 请求 `{id}`"))
            })?;
        tx.send(value.into()).map_err(|e| {
            crate::error::DevforgeError::System(format!("回传 UI 应答: {e}"))
        })
    }

    /// 待答请求数（测试用）。
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// 中止全部待答（cancel 端点解阻用）：丢弃 sender 使所有阻塞中的
    /// `ask_blocking` 立刻失败解阻，引擎得以按 cancel 语义收场——
    /// 修复 dialog 挂起态 cancel 半效（杀 pi 不解阻 mpsc recv）。
    pub fn abort_all(&self) -> usize {
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let n = map.len();
        map.clear();
        n
    }

    /// 中止**指定 run** 的待答（cancel 端点用）：只丢这个 run 的 sender，
    /// 并发 run 的 dialog 不受影响。无归属信息的历史条目（不在 run 上下文里发起）
    /// 保持不动——它们不归任何 run 管，宁可不打也不误伤。
    pub fn abort_run(&self, run_id: &str) -> usize {
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let before = map.len();
        map.retain(|_, (_, owner)| owner.as_deref() != Some(run_id));
        before - map.len()
    }
}

/// sop::UserLink 的中继实现（引擎线程内使用）。
pub struct RelayUserLink {
    pub relay: Arc<UiRelay>,
}

impl UserLink for RelayUserLink {
    fn ask(&mut self, question: &str, options: &[&str]) -> Result<String> {
        self.relay.ask_blocking(question, options)
    }
}

// ---------- 服务器状态与路由 ----------

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub store: Mutex<Store>,
    pub registry: SopRegistry,
    pub manifest: Mutex<ConfigManifest>,
    /// 清单真源落盘路径（PUT 时同步写盘；None = 仅内存）。Mutex 以便测试注入。
    pub manifest_path: Mutex<Option<PathBuf>>,
    /// 每 run 引擎工厂（decisions/003：可多实例化的单 run 引擎）。
    pub engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
    /// profile 安装器（生产 = PiInstallBridge；测试注入 Fake——单测不走真实网络）。
    pub installer: Arc<dyn PackageInstaller + Send + Sync>,
    /// 运行中 run 的干预面（P8-ISSUE-01：cancel 端点置位，watchdog 杀进程）。
    pub runs_control: Mutex<HashMap<String, Arc<RunControl>>>,
    /// 默认 run 超时 ms（None = 不限时，零硬限制哲学；可由 env DEVFORGE_RUN_TIMEOUT_MS 开启）。
    pub run_timeout_ms: Option<u64>,
    /// 事件总线：序号 + 环形缓冲 + 广播（SSE 源与 Last-Event-ID 回放）。
    pub bus: std::sync::Arc<EventBus>,
    pub ui: Arc<UiRelay>,
    /// 鉴权预留位：Some(token) 时要求 `x-devforge-token` 头匹配（Q003）。
    pub token: Option<String>,
    pub profiles_root: PathBuf,
    pub plans_root: PathBuf,
    pub sessions_root: PathBuf,
    /// 运行级重试策略（plan 010 T005）：生产默认 10/30/90s；测试注入 fast。
    pub retry_policy: crate::engine::policy::RetryPolicy,
    /// 渲染闸门（用户裁决 A）：`PUT /config/manifest` 在途时，`start` 端点等所需 profile 就绪。
    pub render_gate: RenderGate,
    /// 观测根覆盖（Some = 本实例的 run 把心跳写在这里；None = 用进程级解析结果）。
    /// 单测用它把落点钉在自己的临时目录 → 不落用户 HOME（R13），且**不与其它单测抢全局 root**。
    pub obs_root: Option<PathBuf>,
    /// 调度有效并发上限（自审 major）：tick 准入用 min(cfg, store)，status 只看 store
    /// 会把「capacity 阻塞」误报成可跑。调度循环启动同步后写入；0 = 未同步（回退 store）。
    pub effective_max_concurrent: std::sync::atomic::AtomicUsize,
}

impl ServerState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        registry: SopRegistry,
        manifest: ConfigManifest,
        engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
        bus: std::sync::Arc<EventBus>,
        ui: Arc<UiRelay>,
        installer: Arc<dyn PackageInstaller + Send + Sync>,
        profiles_root: PathBuf,
        plans_root: PathBuf,
        sessions_root: PathBuf,
    ) -> SharedState {
        Arc::new(ServerState {
            store: Mutex::new(store),
            registry,
            manifest: Mutex::new(manifest),
            manifest_path: Mutex::new(None),
            engine_factory,
            bus,
            ui,
            installer,
            runs_control: Mutex::new(HashMap::new()),
            run_timeout_ms: None,
            token: None,
            profiles_root,
            plans_root,
            sessions_root,
            retry_policy: crate::engine::policy::RetryPolicy::default(),
            render_gate: RenderGate::new(),
            obs_root: None,
            effective_max_concurrent: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn emit(&self, value: serde_json::Value) {
        self.bus.emit(value);
    }
}

// ---------- 后台执行（编排控制的 start 端点用） ----------

/// 台账条目直通（server 侧记录用）。
pub type EngineBox = Box<dyn RunEngine + Send>;

/// 在独立线程跑一个 run（不持锁执行引擎——Store 锁只在 begin/finish 短暂持有）。
/// 准入（`begin_run`）已在 handler 里**同步完成**（server #1：202 不能早于准入），
/// 这里拿到已建好的 `Run`，只负责取快照与开跑。
pub fn run_in_background(
    state: SharedState,
    project: String,
    task_id: String,
    mut engine: EngineBox,
    run: crate::domain::run::Run,
) {
    let task_snapshot = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.get_task(&project, &task_id).cloned() {
            Some(t) => t,
            None => {
                // 自审 minor（222）：旧口径直接 return——run 永久停在 Running 占死串行槽，
                // 而且这件事只有 SSE 事件、日志里查不到。现在落终态 + 双面留痕。
                let reason = "任务不存在（准入后消失）".to_string();
                {
                    let mut store = state.store.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = store.finish_run(
                        &run.id,
                        crate::domain::store::FinishOutcome::Failed {
                            reason: reason.clone(),
                        },
                    ) {
                        tracing::error!(
                            component = "run",
                            event = "run.finish_failed",
                            run_id = %run.id.0,
                            error = %e,
                            "任务快照缺失且结案失败：该 run 仍为 Running，需人工核对 store"
                        );
                    }
                }
                tracing::error!(
                    component = "run",
                    event = "run.no_task",
                    project = %project,
                    task = %task_id,
                    run_id = %run.id.0,
                    "准入后任务消失，run 已按失败结案"
                );
                state.emit(json!({"type": "run.error", "project": project, "task": task_id, "error": reason}));
                crate::observability::end_run(&run.id.0);
                crate::server::scheduler::kick();
                return;
            }
        }
    };
    state.emit(json!({"type": "run.started", "project": project, "task": task_id, "run_id": run.id.0}));

    // 观测上下文置位：run 归属本线程（引擎与重试层同线程 → attempt 字段同源），
    // 心跳随之落盘（阶段 = 该 SOP 的阶段词汇，R24）。
    let run_phase = run_phase_for_sop(task_snapshot.sop.as_str());
    // 观测根：实例覆盖（单测）优先，否则用进程级解析结果；**每个 run 捕获一次** → 落点固定
    let obs_root = state.obs_root.clone().unwrap_or_else(crate::observability::root);
    // F18：心跳首写不落关键路径（deferred 变体，盘写后台线程）
    crate::observability::begin_run_in_deferred(obs_root, &run.id.0, run_phase);
    tracing::info!(
        component = "run",
        event = "phase.change",
        from = "start",
        to = run_phase,
        "run 开始"
    );

    // 干预面登记（P8-ISSUE-01）：cancel 端点置位 / 可选超时，watchdog 杀自家 pi
    let mut control = RunControl::new();
    if let Some(timeout) = state.run_timeout_ms {
        control = control.with_deadline(crate::domain::now_ms() + timeout);
    }
    let control = Arc::new(control);
    state
        .runs_control
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(format!("{project}/{task_id}"), control.clone());

    // 基础设施故障自动重启（plan 010 T005/D004）：只重 System 类，质量失败不重试
    let policy = state.retry_policy.clone();
    let sop_for_state = task_snapshot.sop.clone();
    let state_for_retry = state.clone();
    let (project_for_retry, task_for_retry, run_id_for_retry) =
        (project.clone(), task_id.clone(), run.id.0.clone());
    let (outcome, attempts) = crate::engine::policy::run_with_retry(&policy, &control, |attempt| {
        if attempt > 1 {
            // 重启记账（attempt 落盘 + WARN + run.retry 事件）已收敛至 policy（ARC-R5）
            crate::engine::policy::run_attempt_bookkeeping(
                &state_for_retry.plans_root,
                &project_for_retry,
                &task_for_retry,
                &sop_for_state,
                &run_id_for_retry,
                attempt,
                |v| state_for_retry.emit(v),
            );
        }
        // 全仓审计 #1：引擎 panic 兜底——SOP 里任何一次 panic 都会展开 run 线程，
        // 而 Running 终态只由下方 finish_run 出清（准入看 running_run_count）。
        // 裸展开 = 串行槽位与任务状态永久卡死，只能重启进程。
        // 自审 minor（124）：契约终校验的 System 类错误必须发生在**重试作用域之内**——
        // 旧口径在 run_with_retry 返回之后才做（注释宣称的「基础设施故障可重启」在该路径上
        // 永不成立，且没有重试记录）。现在每次尝试的产出立即过契约门。
        let exec = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let out = engine.execute(&task_snapshot, &run, &control);
            contract_boundary_check(&state_for_retry, &project_for_retry, &task_for_retry, out)
        }));
        match exec {
            Ok(r) => r,
            Err(payload) => {
                let msg = panic_text(&payload);
                tracing::error!(
                    component = "run",
                    event = "run.panic",
                    run_id = %run.id.0,
                    panic = %msg,
                    // 自审 minor（224）：旧文案写「按 System 失败收尾」，代码实际归类 Quality——
                    // 日志与真实分类矛盾，事后按级别筛选会误判。
                    "引擎 panic 已兜底：按**质量失败**收尾（引擎 bug 不自动重启，不裸展开 run 线程）"
                );
                state_for_retry.emit(json!({
                    "type": "run.panic",
                    "project": project_for_retry,
                    "task": task_for_retry,
                    "run_id": run_id_for_retry,
                    "panic": msg,
                }));
                // 按质量失败归类（不自动重启）：panic 是引擎自身 bug，重启只会复现；
                // 且 panic 可能已毒化共享锁，盲重试只会把 4 次尝试全烧光。
                Err(crate::error::DevforgeError::Quality(format!(
                    "引擎 panic（bug，重启只会复现）：{msg}"
                )))
            }
        }
    });

    state
        .runs_control
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&format!("{project}/{task_id}"));

    // 契约终校验已在每次尝试内完成（见上，124）——这里不再二次校验（避免两次 diff）。

    let finish = match &outcome {
        Ok(RunOutcome::Completed) => crate::domain::store::FinishOutcome::Completed,
        Ok(RunOutcome::Failed { reason }) => crate::domain::store::FinishOutcome::Failed {
            reason: reason.clone(),
        },
        Err(e) => crate::domain::store::FinishOutcome::Failed {
            reason: e.to_string(),
        },
    };
    let ok = {
        let mut store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        match store.finish_run(&run.id, finish) {
            Ok(()) => true,
            Err(e) => {
                // server #4：收尾失败不能只吞成一个 ok=false——run 会停在 Running
                // 继续占着串行槽（准入看 running_run_count），下一个 start 会莫名 409。
                tracing::error!(
                    component = "run",
                    event = "run.finish_failed",
                    project,
                    task = task_id,
                    run_id = %run.id.0,
                    error = %e,
                    "run 终态落账失败：该 run 仍为 Running，串行槽被占死，需人工核对 store"
                );
                false
            }
        }
    };
    let completed = ok && matches!(outcome, Ok(RunOutcome::Completed));
    // 自审 minor（225）：run 终态此前只活在事件总线的内存环形缓冲里，日志侧没有记录
    // （boundary.ok 也只有事件没有日志）。日志必须能独立回溯「这次 run 怎么收的场」。
    tracing::info!(
        component = "run",
        event = "run.finished",
        project = %project,
        task = %task_id,
        run_id = %run.id.0,
        attempts,
        ok = completed,
        store_finished = ok,
        reason = ?outcome.as_ref().err().map(|e| e.to_string()),
        "run 收尾"
    );
    // 自审 minor（226）：结案失败时 store 里该 run 仍是 Running——事件流不得宣称已终态
    // （两个事实源互相矛盾）。用 store_finished 显式区分，消费方据此决定是否重查。
    state.emit(json!({
        "type": "run.finished",
        "project": project,
        "task": task_id,
        "run_id": run.id.0,
        "attempts": attempts,
        "ok": completed,
        "store_finished": ok,
    }));
    // 观测上下文清位：心跳文件**保留**最后一次状态（死亡现场的证据，不删）
    crate::observability::end_run(&run.id.0);
    // P012 M4：run 收尾踢调度器——依赖本任务的后继立即就绪判定 + Settled 自动入队
    crate::server::scheduler::kick();
}

/// base_rev 是否是可安全交给 git 的纯 commit sha（自审 major 125：注入面收口）。
fn is_plain_git_sha(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 引擎 panic 兜底（全仓审计 #1）：从 panic payload 取人读文本。
///
/// `pub` 是给审计批次 worker 复用的（同一个「panic 也要留人读原因」口径）。
pub fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "非字符串 panic payload".to_string()
    }
}

/// P012 M0 · T0.2：契约边界终校验（引擎层，SOP 无关）。
///
/// run 成功且任务带契约时：`git diff --name-only {base_rev}..HEAD` 的实际改动集
/// 必须 ⊆ 契约 owned_paths，越界 = 改判 Failed（boundary.violation 事件）。
/// 无契约 / run 未成功 / 契约无 base_rev（M0：无基准无法机械 diff）→ 原样放行。
fn contract_boundary_check(
    state: &ServerState,
    project: &str,
    task_id: &str,
    outcome: Result<RunOutcome>,
) -> Result<RunOutcome> {
    let outcome = outcome?;
    if !matches!(outcome, RunOutcome::Completed) {
        return Ok(outcome);
    }
    let task = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        store.get_task(project, task_id).cloned()
    };
    let Some(task) = task else {
        return Ok(outcome);
    };
    let isolation_worktree = matches!(
        task.contract.as_ref().map(|c| &c.isolation),
        Some(crate::domain::contract::Isolation::Worktree)
    );
    let Some(contract) = task.contract else {
        return Ok(outcome);
    };
    let Some(base) = contract.base_rev.as_deref() else {
        return Ok(outcome);
    };
    // 自审 major（125）：base_rev 直接进 git 参数——旧口径零校验，`--output=/x` 这类选项
    // 注入能让 diff 结果落盘到指定路径并让 stdout 变空，从而强制边界门放行（fail-open）。
    if !is_plain_git_sha(base) {
        return Err(crate::error::DevforgeError::Config(format!(
            "契约 base_rev 形状非法：`{base}`（必须是 7..=40 位十六进制 commit，不能含选项前缀）"
        )));
    }
    // 自审 critical：worktree 隔离下主仓 HEAD 纹丝不动（agent 的改动全部在 task/{id}
    // 分支上，worktree 已在引擎收尾时删除）——对 HEAD 求 diff 恒为空集，越界 run
    // 被判 boundary.ok（门 fail-open，正是本函数注释声称已修掉的那类假绿）。改对
    // 候选分支尖求 diff；分支不存在 = 收尾异常，按系统错误拒放行（门不能 fail-open）。
    let diff_target = if isolation_worktree {
        let branch = format!("task/{task_id}");
        let probe = std::process::Command::new("git")
            .args([
                "-C",
                &task.workspace.to_string_lossy(),
                "rev-parse",
                "--verify",
                &branch,
            ])
            .output();
        match probe {
            Ok(o) if o.status.success() => branch,
            _ => {
                return Err(crate::error::DevforgeError::System(format!(
                    "契约终校验找不到候选分支 {branch}（worktree 隔离下对主仓 HEAD 求 diff 恒为空集）——不能放行为 Completed"
                )));
            }
        }
    } else {
        "HEAD".to_string()
    };
    let out = std::process::Command::new("git")
        .args([
            "-C",
            &task.workspace.to_string_lossy(),
            "diff",
            "--name-only",
            &format!("{base}..{diff_target}"),
        ])
        .output();
    let committed: Vec<String> = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        Ok(o) => {
            // 引擎批次 #5：**门不能 fail-open**——旧实现在 git diff 失败时 warn 后放行，
            // 而调用方看到的是 Completed（成功），owned_paths/forbidden_paths 两个契约门
            // 静默失效。改为系统错误（基础设施故障 → 可重启，而不是伪装成「审过了」。
            return Err(crate::error::DevforgeError::System(format!(
                "契约终校验无法取 diff（git 非零退出）：{}——不能放行为 Completed",
                String::from_utf8_lossy(&o.stderr).trim()
            )));
        }
        Err(e) => {
            return Err(crate::error::DevforgeError::System(format!(
                "契约终校验无法调用 git：{e}——不能放行为 Completed"
            )));
        }
    };
    // 自审 minor（126）：改动集口径必须含**未提交改动与未跟踪文件**——仓内 coding gate
    // 已明确点过这个盲区（只 diff 已提交会让「改完不提交」的越界完全隐身）。
    let mut changed: Vec<String> = committed;
    let worktree_status = std::process::Command::new("git")
        .args([
            "-C",
            &task.workspace.to_string_lossy(),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ])
        .output();
    match worktree_status {
        Ok(o) if o.status.success() => {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                // porcelain 格式：XY <path>（rename 时形如 "old -> new"，两边都算改动）
                let path = line.get(3..).unwrap_or("").trim();
                if path.is_empty() {
                    continue;
                }
                if let Some((a, b)) = path.split_once(" -> ") {
                    changed.push(a.trim().to_string());
                    changed.push(b.trim().to_string());
                } else {
                    changed.push(path.to_string());
                }
            }
            changed.sort();
            changed.dedup();
        }
        Ok(o) => {
            return Err(crate::error::DevforgeError::System(format!(
                "契约终校验无法取工作区状态（git status 非零退出）：{}——不能放行为 Completed",
                String::from_utf8_lossy(&o.stderr).trim()
            )));
        }
        Err(e) => {
            return Err(crate::error::DevforgeError::System(format!(
                "契约终校验无法调用 git status：{e}——不能放行为 Completed"
            )));
        }
    }
    // ① forbidden_paths 绝对禁令（P012 挂账补全）：命中即越界，优先于 owned 覆盖判定——
    //    即便文件在 owned 内，禁改条目仍然生效（如 assets/prompts/** 等系统资产）。
    let forbidden_hits: Vec<&String> = changed
        .iter()
        .filter(|f| contract.hits_forbidden(f))
        .collect();
    if !forbidden_hits.is_empty() {
        let reason = format!(
            "触碰禁改路径：{} 个文件命中 contract.forbidden_paths（base {base}）：{}",
            forbidden_hits.len(),
            forbidden_hits.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
        tracing::warn!(component = "boundary", event = "boundary.violation", forbidden = true, "{reason}");
        state.emit(json!({
            "type": "boundary.violation",
            "project": project,
            "task": task_id,
            "base_rev": base,
            "forbidden": true,
            "violations": forbidden_hits,
        }));
        return Ok(RunOutcome::Failed { reason });
    }
    // ② owned_paths 覆盖判定（M0 原有）。
    let violations: Vec<&String> = changed.iter().filter(|f| !contract.covers(f)).collect();
    if violations.is_empty() {
        state.emit(json!({
            "type": "boundary.ok",
            "project": project,
            "task": task_id,
            "files": changed.len(),
        }));
        return Ok(outcome);
    }
    let reason = format!(
        "越界改动：{} 个文件超出契约 owned_paths（base {base}）：{}",
        violations.len(),
        violations.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
    );
    tracing::warn!(component = "boundary", event = "boundary.violation", "{reason}");
    state.emit(json!({
        "type": "boundary.violation",
        "project": project,
        "task": task_id,
        "base_rev": base,
        "violations": violations,
    }));
    Ok(RunOutcome::Failed { reason })
}

/// 每个 SOP 的阶段词汇（心跳的阶段字段 = 阶段名；R24：与里程碑同一套值）。
///
/// change SOP 的首个阶段是 `ChangePhase::Plan`（编排层随后推进到 coding/testing/done）；
/// 其余 SOP 用自己的阶段标识。
fn run_phase_for_sop(sop: &str) -> &str {
    match sop {
        "change" => crate::change::state::ChangePhase::Plan.as_str(),
        "planning" => "plan",
        "coding" => "coding",
        "testing" => "test",
        "doc-from-zero" => "doc",
        "doc-fix" => "doc-fix",
        other => other,
    }
}

// 自审 minor（49）：旧口径在这里再导出 SpawnLedgerEntry，注释称「spawn 侧调用方记录用」——
// 全仓零引用。死再导出已删除（需要时直接用 crate::engine::ledger::LedgerEntry）。

