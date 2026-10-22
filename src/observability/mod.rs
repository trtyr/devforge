//! 可观测性（plan 010 · F6 靶）：run 上下文 + 观测根解析 + 心跳/墓碑的公共面。
//!
//! 三件交付物共用这一层的词汇：
//! - **run 上下文**（run id / 阶段 / 步骤 / attempt）：进程级单例。MVP 是串行执行
//!   （同一时刻只有一个 run），故这里不做并发隔离（将来并发化时两个 run 的上下文会互相覆盖，
//!   登记为遗留）。
//! - **观测根**（root）：心跳与墓碑都落它下面。解析顺序（Q4）：
//!   **进程内注入**（单测用）> `DEVFORGE_HOME` > `HOME/.devforge`（生产默认）。
//! - **里程碑字段**：由各调用点**内联**写 `tracing::info!`——`tracing` 的 `target` 必须是
//!   发出它的模块，包装函数会让所有里程碑共用同一个 target，「按 target 过滤」这条需求直接作废。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// 进程级状态（run 上下文 + 角色计数 + 注入的观测根）。
#[derive(Default)]
struct ObsState {
    run: Option<RunSnapshot>,
    /// 角色内审查轮次计数（key = 角色名；change 侧拿不到共享门的实例数，Q6/R21）。
    review_rounds: HashMap<String, u32>,
    /// 注入的观测根（None = 按 `DEVFORGE_HOME` / `HOME` 解析）。
    root: Option<PathBuf>,
    /// 上次刷心跳的时刻（低频兜底的节流）。
    last_touch_ms: u64,
}

fn state() -> &'static Mutex<ObsState> {
    static STATE: OnceLock<Mutex<ObsState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ObsState::default()))
}

/// 置位 run 上下文（run 开始）：同时清零角色内审查轮次计数。
pub fn begin_run(run_id: &str, phase: &str) {
    let mut st = state().lock().expect("observability 状态锁");
    st.run = Some(RunSnapshot {
        run_id: run_id.to_string(),
        phase: phase.to_string(),
        step: String::new(),
        attempt: 1,
    });
    st.review_rounds.clear();
    st.last_touch_ms = 0;
    drop(st);
    touch();
}

/// 清位 run 上下文（run 结束）：**心跳文件保留**最后一次状态（不删——那是死亡现场的证据）。
pub fn end_run() {
    let mut st = state().lock().expect("observability 状态锁");
    st.run = None;
    st.review_rounds.clear();
}

/// 更新阶段（编排层阶段转换时调用；顺带刷心跳）。
pub fn set_phase(phase: &str) {
    {
        let mut st = state().lock().expect("observability 状态锁");
        if let Some(run) = st.run.as_mut() {
            run.phase = phase.to_string();
        } else {
            return;
        }
    }
    touch();
}

/// 更新步骤（阶段内更细的一格）。
pub fn set_step(step: &str) {
    {
        let mut st = state().lock().expect("observability 状态锁");
        let Some(run) = st.run.as_mut() else {
            return;
        };
        run.step = step.to_string();
    }
    touch();
}

/// 更新尝试次数（重试层每次尝试更新；`map_intervention` 从同一处读，两处字段才同构）。
pub fn set_attempt(attempt: u32) {
    let mut st = state().lock().expect("observability 状态锁");
    if let Some(run) = st.run.as_mut() {
        run.attempt = attempt;
    }
}

/// 当前 run 快照（无活动 run = `None`）。
pub fn current() -> Option<RunSnapshot> {
    let st = state().lock().expect("observability 状态锁");
    st.run.clone()
}

/// 角色内审查轮次（1-based）：同一角色在本次 run 内第几次开庭。
///
/// change 侧的 `review_once` 恒传 `base_round=0`（共享门返回的实例数是**返回值**，调用前拿不到），
/// 故按 `(run, role)` 计数（R21）。
pub fn next_review_round(role: &str) -> u32 {
    let mut st = state().lock().expect("observability 状态锁");
    let counter = st.review_rounds.entry(role.to_string()).or_insert(0);
    *counter += 1;
    *counter
}

/// 低频兜底刷新的节流间隔（毫秒）：watchdog 每 300ms 一跳，心跳不必跟着每跳写盘。
pub const HEARTBEAT_REFRESH_MS: u64 = 1_000;

/// 刷心跳（无活动 run 时**不写**；低频兜底由 watchdog 调用，节流到
/// [`HEARTBEAT_REFRESH_MS`] 一次——阶段/步骤变化走 `set_phase`/`set_step`，不受节流）。
pub fn touch() {
    let snap = {
        let st = state().lock().expect("observability 状态锁");
        st.run.clone()
    };
    let Some(snap) = snap else { return };
    let now = crate::domain::now_ms();
    {
        let mut st = state().lock().expect("observability 状态锁");
        if now.saturating_sub(st.last_touch_ms) < HEARTBEAT_REFRESH_MS {
            return;
        }
        st.last_touch_ms = now;
    }
    let root = root();
    if let Err(e) = heartbeat::write(&root, &snap) {
        // 刷心跳失败不打断 run（best-effort）：只在 debug 里留痕
        tracing::debug!(component = "observability", error = %e, "心跳写入失败");
    }
}

/// 注入观测根（单测用；生产由 `DEVFORGE_HOME` / `HOME/.devforge` 解析）。
pub fn set_root(root: impl Into<PathBuf>) {
    let mut st = state().lock().expect("observability 状态锁");
    st.root = Some(root.into());
}

/// 解析观测根：进程内注入 > `DEVFORGE_HOME` > `HOME/.devforge`。
pub fn root() -> PathBuf {
    if let Some(r) = state().lock().expect("observability 状态锁").root.clone() {
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
        begin_run("run-1", "coding");
        set_phase("testing");
        set_step("review:change-plan-review");
        set_attempt(3);
        let snap = current().expect("run 上下文应存在");
        assert_eq!(snap.run_id, "run-1");
        assert_eq!(snap.phase, "testing");
        assert_eq!(snap.step, "review:change-plan-review");
        assert_eq!(snap.attempt, 3);
        end_run();
        assert!(current().is_none(), "run 结束后上下文应清位");
    }

    #[test]
    fn review_round_counts_per_role_and_resets_per_run() {
        begin_run("run-1", "plan");
        assert_eq!(next_review_round("change-plan-review"), 1);
        assert_eq!(next_review_round("change-plan-review"), 2);
        assert_eq!(next_review_round("change-coding-review"), 1);
        begin_run("run-2", "plan");
        assert_eq!(next_review_round("change-plan-review"), 1, "新 run 的计数从 1 起");
        end_run();
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
