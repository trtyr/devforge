//! change run 的检查点（P009 T011b，落点按 plan 010 D001/D022a：引擎写、住黑板）。
//!
//! 状态本身走 `runstate::StateStore`（`plans/<project>/<task>/change/state.json`），
//! 这里只定义 change 自己的扩展段结构——**引擎不解释它，SOP 自己读写**。
//!
//! 续跑语义（010 D002/D003）：`start` 默认从断点继续；`roundtrips` 跨会话保留，
//! 不能因为重启就把「已经往返过几次」清零。

use serde::{Deserialize, Serialize};

use crate::error::{DevforgeError, Result};
use crate::runstate::{Loaded, RunState, StateStore};

/// change SOP 的注册名（同时是状态目录名）。
pub const CHANGE_SOP: &str = "change";

/// 阶段（字符串形式落盘，便于人读与向前兼容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangePhase {
    Plan,
    Coding,
    Testing,
    Done,
}

impl ChangePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Coding => "coding",
            Self::Testing => "testing",
            Self::Done => "done",
        }
    }
}

/// 上一轮测试失败的上下文（回注给 coding agent 的送达面内容）。
///
/// `issues` = test agent 给出的失败明细**逐条原文**（含 `path:line`，不剥不改写）；
/// `round` = 第几轮测试→coding 回注（与 `ChangeState::test_reroutes` 同值）。
/// 住在断点里：崩溃/重启后续跑照样在同一轮送达同一份（幂等）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestFailureContext {
    /// 第几轮回注（首次回注 = 1）。
    pub round: usize,
    /// 失败明细逐条原文（含 `path:line`）。
    pub issues: Vec<String>,
}

/// change run 的断点内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeState {
    pub phase: ChangePhase,
    /// 门 0 记下的基线 sha（全程不动，回滚与 diff 都锚它）。
    pub baseline_sha: String,
    /// 已发生的 plan↔coding 往返次数（上限 3）。
    #[serde(default)]
    pub roundtrips: usize,
    /// 已对账通过的验收项。
    #[serde(default)]
    pub acceptance_done: Vec<String>,
    /// 累计审查轮次。
    #[serde(default)]
    pub review_rounds: usize,
    /// 测试→coding 回注次数（跳会话累计，防止无休止互推）。
    #[serde(default)]
    pub test_reroutes: usize,
    /// change-id（跨会话核对同一件事）。
    #[serde(default)]
    pub change_id: String,
    /// 上一轮测试失败的上下文（`None` = 无上下文）。
    ///
    /// 缺键安全：serde 对**缺键的 `Option<T>` 返回 `None`**（不报 `missing_field`），
    /// 旧断点文件没有这个键时照样加载成功（计数不丢）。
    #[serde(default)]
    pub last_test_failure: Option<TestFailureContext>,
    /// testing→plan 回退携带的计划问题（引擎审计修复 #5：issues 入断点——
    /// resume 后 plan 循环读它走 fix_plan 而非重新 kickoff；旧断点缺键 = 空）。
    #[serde(default)]
    pub last_plan_issues: Vec<String>,
    /// 计划审查已用的修复轮数（上限 MAX_PLAN_FIXES）——复验 #9：失败路径也落盘，
    /// 重启后不得重新获得修复额度。
    #[serde(default)]
    pub plan_fix_rounds: usize,
    /// 编码审查已用的回注轮数（上限 MAX_CODING_FIXES）——change #4：编码审查 FAIL
    /// 不再直接判 Quality，而是回注修复；同样落盘，重启不重置额度。
    #[serde(default)]
    pub coding_fix_rounds: usize,
    /// 已经判定终态的原因（自审 minor 74）：四个上限里此前只有往返是「先比较后落盘」，
    /// 其余三个撞上限时不写断点——续跑会先重跑一整段 coding、再在同一个上限上失败。
    /// 有值 = 本次变更已判死，续跑立即失败（要重跑得带 fresh）。
    #[serde(default)]
    pub terminal_reason: Option<String>,
}

impl ChangeState {
    pub fn new(change_id: impl Into<String>, baseline_sha: impl Into<String>) -> Self {
        Self {
            phase: ChangePhase::Plan,
            baseline_sha: baseline_sha.into(),
            roundtrips: 0,
            acceptance_done: Vec::new(),
            review_rounds: 0,
            test_reroutes: 0,
            change_id: change_id.into(),
            last_test_failure: None,
            last_plan_issues: Vec::new(),
            plan_fix_rounds: 0,
            coding_fix_rounds: 0,
            terminal_reason: None,
        }
    }

    /// 从 `RunState.data` 抠出来（没有就按 change_id/baseline 新建）；
    /// 内层 change 段存在但解析失败时记 WARN（复验 #5：不能静默重置计数）。
    pub fn from_run(state: &RunState, change_id: &str, baseline: &str) -> Result<Self> {
        match state.data.get("change") {
            None => Ok(Self::new(change_id, baseline)),
            Some(v) => {
                let mut out: Self = serde_json::from_value(v.clone()).map_err(|e| {
                tracing::error!(
                    component = "change-orchestrator",
                    change_id,
                    error = %e,
                    "断点内层 change 段解析失败——拒绝冷启动（计数会被归零、额度被重置）"
                );
                DevforgeError::Config(format!(
                    "change 断点损坏（{e}）；为避免计数被重置、额度被绕过，拒绝冷启动重跑——确需重跑请带 fresh:true"
                ))
                })?;
                // 自审 minor（19）：断点里持久化了 change_id，但旧口径从不与本次 run 核对——
                // 陈旧断点（换了 change id 但复用同一黑板）会被静默采纳。
                if !out.change_id.is_empty() && out.change_id != change_id {
                    tracing::error!(
                        component = "change-orchestrator",
                        expected = %change_id,
                        found = %out.change_id,
                        "断点属于另一个 change-id——拒绝采纳（陈旧断点）"
                    );
                    return Err(DevforgeError::Config(format!(
                        "断点 change_id 不符（文件 `{}` / 请求 `{change_id}`）：陈旧断点不得复用，确需重跑请带 fresh:true",
                        out.change_id
                    )));
                }
                if out.change_id.is_empty() {
                    out.change_id = change_id.to_string();
                }
                if out.baseline_sha.is_empty() {
                    out.baseline_sha = baseline.to_string();
                }
                Ok(out)
            }
        }
    }

    /// 写回 `RunState.data`。
    pub fn write_into(&self, state: &mut RunState) {
        if !state.data.is_object() {
            state.data = serde_json::json!({});
        }
        if let Some(obj) = state.data.as_object_mut() {
            obj.insert(
                "change".into(),
                serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
            );
        }
    }

    // 引擎审计修复 #10：mark_step_done 已删——impl_steps_done 是从未被编排器写入的
    // 死字段（活体步对账走 git log vs impl-steps.md），旧断点文件里的该键由 serde
    // 忽略未知字段语义安全兼容。

    pub fn mark_acceptance_done(&mut self, criterion: &str) {
        if !self.acceptance_done.iter().any(|c| c == criterion) {
            self.acceptance_done.push(criterion.to_string());
        }
    }

    pub fn bump_review(&mut self) {
        self.review_rounds += 1;
    }

    pub fn bump_roundtrip(&mut self) {
        self.roundtrips += 1;
    }

    pub fn bump_test_reroute(&mut self) {
        self.test_reroutes += 1;
    }

    /// 写入失败上下文（**覆盖**语义：新一次回注的 issues 覆盖旧的，不追加）。
    ///
    /// 覆盖而不是追加的理由：test agent 每轮重跑全套，报出来的就是当前还红的东西，
    /// 追加只会把已修好的旧失败继续喂给 agent。
    pub fn set_test_failure(&mut self, round: usize, issues: Vec<String>) {
        self.last_test_failure = Some(TestFailureContext { round, issues });
    }

    /// 清除失败上下文（消费即清 / 终态清）。
    pub fn clear_test_failure(&mut self) {
        self.last_test_failure = None;
    }
}

/// 读断点（**仅测试/展示路径**）：损坏时按冷启动重置。生产入口一律走
/// [`load_strict`]——change 批次 #3：计数器被重置就等于修复额度被重置。
pub fn load(store: &StateStore, change_id: &str, baseline: &str) -> ChangeState {
    match load_strict(store, change_id, baseline) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                component = "change-orchestrator",
                change_id,
                error = %e,
                "断点不可用，按冷启动重置（仅测试/展示路径；生产入口走 load_strict）"
            );
            ChangeState::new(change_id, baseline)
        }
    }
}

/// 生产入口读断点。change 批次 #3：**损坏不再按冷启动**——旧实现内层解析失败 /
/// Unusable 一律 `new(...)`，`roundtrips`/`test_reroutes`/`plan_fix_rounds` 全部归零，
/// 三个上限就此被重置（上限挂在内存计数器上，一次断点损坏就重新获得额度）。现在：
/// 文件缺失 = 冷启动（合法）；存在但不可用 = 拒绝继续，要重跑就显式带 `fresh:true`。
pub fn load_strict(store: &StateStore, change_id: &str, baseline: &str) -> Result<ChangeState> {
    match store.load() {
        Loaded::Found(state) => ChangeState::from_run(&state, change_id, baseline),
        Loaded::Unusable(reason) => {
            tracing::error!(
                component = "change-orchestrator",
                change_id,
                %reason,
                "断点不可用——拒绝冷启动（计数会被归零、修复额度被重置）"
            );
            Err(DevforgeError::Config(format!(
                "change 断点不可用（{reason}）；为避免计数被重置、额度被绕过，拒绝冷启动重跑——确需重跑请带 fresh:true"
            )))
        }
        _ => Ok(ChangeState::new(change_id, baseline)),
    }
}

/// 原子写回断点（走 `StateStore`：临时文件 + rename）。
pub fn save(store: &StateStore, state: &ChangeState) -> Result<()> {
    let mut run = match store.load() {
        Loaded::Found(s) => s,
        _ => store.fresh_state(),
    };
    state.write_into(&mut run);
    store.save(&mut run)?;
    Ok(())
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
