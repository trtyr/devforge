//! change run 的检查点（P009 T011b，落点按 plan 010 D001/D022a：引擎写、住黑板）。
//!
//! 状态本身走 `runstate::StateStore`（`plans/<project>/<task>/change/state.json`），
//! 这里只定义 change 自己的扩展段结构——**引擎不解释它，SOP 自己读写**。
//!
//! 续跑语义（010 D002/D003）：`start` 默认从断点继续；`roundtrips` 跨会话保留，
//! 不能因为重启就把「已经往返过几次」清零。

use serde::{Deserialize, Serialize};

use crate::error::Result;
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
    /// 已完成的实施步骤号。
    #[serde(default)]
    pub impl_steps_done: Vec<u32>,
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
}

impl ChangeState {
    pub fn new(change_id: impl Into<String>, baseline_sha: impl Into<String>) -> Self {
        Self {
            phase: ChangePhase::Plan,
            baseline_sha: baseline_sha.into(),
            roundtrips: 0,
            impl_steps_done: Vec::new(),
            acceptance_done: Vec::new(),
            review_rounds: 0,
            test_reroutes: 0,
            change_id: change_id.into(),
            last_test_failure: None,
            last_plan_issues: Vec::new(),
            plan_fix_rounds: 0,
        }
    }

    /// 从 `RunState.data` 抠出来（没有就按 change_id/baseline 新建）；
    /// 内层 change 段存在但解析失败时记 WARN（复验 #5：不能静默重置计数）。
    pub fn from_run(state: &RunState, change_id: &str, baseline: &str) -> Self {
        match state.data.get("change") {
            None => Self::new(change_id, baseline),
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(parsed) => parsed,
                Err(e) => {
                    tracing::warn!(
                        component = "change-orchestrator",
                        change_id,
                        error = %e,
                        "断点内层 change 段解析失败，按冷启动重置（计数将回零）"
                    );
                    Self::new(change_id, baseline)
                }
            },
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

    pub fn mark_step_done(&mut self, step: u32) {
        if !self.impl_steps_done.contains(&step) {
            self.impl_steps_done.push(step);
            self.impl_steps_done.sort_unstable();
        }
    }

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

/// 读断点（异常按冷启动，与 010 D003 同口径）；Unusable 记 WARN（复验 #5：
/// 静默重置会让断点损坏无从察觉——冷启动是设计，丢弃原因是缺陷）。
pub fn load(store: &StateStore, change_id: &str, baseline: &str) -> ChangeState {
    match store.load() {
        Loaded::Found(state) => ChangeState::from_run(&state, change_id, baseline),
        Loaded::Unusable(reason) => {
            tracing::warn!(
                component = "change-orchestrator",
                change_id,
                %reason,
                "断点不可用，按冷启动重置（原因见日志）"
            );
            ChangeState::new(change_id, baseline)
        }
        _ => ChangeState::new(change_id, baseline),
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
mod tests {
    use super::*;
    use crate::runstate::StateKey;

    fn store(root: &std::path::Path) -> StateStore {
        StateStore::new(
            root,
            StateKey::new("proj", "task", CHANGE_SOP),
        )
    }

    #[test]
    fn roundtrip_persists_phase_baseline_and_counters() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let mut st = ChangeState::new("change-20261020-x", "abc123");
        st.phase = ChangePhase::Coding;
        st.bump_roundtrip();
        st.mark_step_done(2);
        st.mark_step_done(1);
        st.mark_acceptance_done("x=1 → 2");
        st.bump_review();
        save(&s, &st).unwrap();

        let got = load(&s, "change-20261020-x", "abc123");
        assert_eq!(got, st);
        assert_eq!(got.impl_steps_done, vec![1, 2], "步骤顺序稳定");
    }

    #[test]
    fn checkpoint_lives_on_blackboard_path() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let st = ChangeState::new("c1", "sha1");
        save(&s, &st).unwrap();
        let path = s.path();
        assert!(
            path.ends_with("proj/task/change/state.json"),
            "断点应落黑板：{}",
            path.display()
        );
        assert!(path.is_file());
    }

    #[test]
    fn missing_checkpoint_means_cold_start() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let st = load(&s, "c1", "sha1");
        assert_eq!(st.phase, ChangePhase::Plan);
        assert_eq!(st.roundtrips, 0);
        assert_eq!(st.baseline_sha, "sha1");
    }

    #[test]
    fn resume_keeps_roundtrips_across_sessions() {
        // 模拟「testing 中途进程退出」：同一黑板再打开，往返计数不丢
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let mut st = ChangeState::new("c1", "sha1");
        st.bump_roundtrip();
        st.bump_roundtrip();
        st.phase = ChangePhase::Testing;
        save(&s, &st).unwrap();

        // 新会话（新 StateStore，同路径）续跑
        let s2 = StateStore::new(
            tmp.path(),
            StateKey::new("proj", "task", CHANGE_SOP),
        );
        let resumed = load(&s2, "c1", "sha1");
        assert_eq!(resumed.roundtrips, 2, "往返计数必须跨会话保留");
        assert_eq!(resumed.phase, ChangePhase::Testing, "从 testing 继续");
    }

    #[test]
    fn counters_are_idempotent() {
        let mut st = ChangeState::new("c1", "sha1");
        st.mark_step_done(1);
        st.mark_step_done(1);
        st.mark_acceptance_done("a");
        st.mark_acceptance_done("a");
        assert_eq!(st.impl_steps_done, vec![1]);
        assert_eq!(st.acceptance_done, vec!["a".to_string()]);
    }

    #[test]
    fn failure_context_persists_across_sessions() {
        // 错实现形态：上下文只活在内存里（不落盘）→ 续跑丢明细，本用例必须变红
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let mut st = ChangeState::new("c1", "sha1");
        st.set_test_failure(
            1,
            vec![
                "tests/change_ask_ring.rs:557 断言失败".to_string(),
                "接口返回 500".to_string(),
            ],
        );
        save(&s, &st).unwrap();

        // 新会话（新 StateStore，同一黑板路径）续跑：上下文仍在、明细逐字不变
        let s2 = StateStore::new(tmp.path(), StateKey::new("proj", "task", CHANGE_SOP));
        let got = load(&s2, "c1", "sha1");
        let ctx = got.last_test_failure.expect("失败上下文必须跨会话保留");
        assert_eq!(ctx.round, 1, "轮次要原样保留");
        assert_eq!(
            ctx.issues,
            vec![
                "tests/change_ask_ring.rs:557 断言失败".to_string(),
                "接口返回 500".to_string()
            ],
            "失败明细必须逐条原文保留（含 path:line）"
        );
    }

    #[test]
    fn old_checkpoint_without_failure_key_still_loads() {
        // 错实现形态：新字段没加 default / 用了非 Option → 旧断点反序列化失败被 .ok() 吞掉
        // → 断点冷启动（计数归零）→ 本用例必须变红
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let mut rs = s.fresh_state();
        // 旧版断点：change 段里没有失败上下文这个键
        rs.data = serde_json::json!({"change": {
            "phase": "coding",
            "baseline_sha": "sha1",
            "roundtrips": 2,
            "impl_steps_done": [1, 2],
            "acceptance_done": ["x=1 → 2"],
            "review_rounds": 1,
            "test_reroutes": 2,
            "change_id": "c1"
        }});
        s.save(&mut rs).unwrap();

        let got = load(&s, "c1", "sha1");
        assert_eq!(
            got.last_test_failure, None,
            "旧断点没有失败上下文 → 加载后应是「无上下文」"
        );
        assert_eq!(got.phase, ChangePhase::Coding, "旧断点不能被冷启动重置");
        assert_eq!(got.roundtrips, 2, "往返计数不能丢");
        assert_eq!(got.test_reroutes, 2, "回注计数不能丢");
        assert_eq!(got.impl_steps_done, vec![1, 2], "已完成步骤不能丢");
        assert_eq!(got.acceptance_done, vec!["x=1 → 2".to_string()]);
    }

    #[test]
    fn failure_context_is_overwritten_not_appended() {
        // 错实现形态：新一次回注把 issues 追加到旧列表 → 已修好的旧失败又被喂回去
        let mut st = ChangeState::new("c1", "sha1");
        st.set_test_failure(1, vec!["旧失败".to_string()]);
        st.set_test_failure(2, vec!["新失败".to_string()]);
        let ctx = st.last_test_failure.expect("应有上下文");
        assert_eq!(ctx.round, 2, "轮次要跟着更新");
        assert_eq!(ctx.issues, vec!["新失败".to_string()], "新 issues 覆盖旧的");
    }

    #[test]
    fn clearing_failure_context_empties_it() {
        let mut st = ChangeState::new("c1", "sha1");
        st.set_test_failure(1, vec!["失败".to_string()]);
        st.clear_test_failure();
        assert_eq!(st.last_test_failure, None, "清除后应回到「无上下文」");
    }

    #[test]
    fn foreign_state_is_ignored() {
        // 另一个 SOP 的断点不会污染 change 的扩段
        let tmp = tempfile::tempdir().unwrap();
        let other = StateStore::new(tmp.path(), StateKey::new("proj", "task", "doc-from-zero"));
        let mut rs = other.fresh_state();
        rs.data = serde_json::json!({"done": ["u1"]});
        other.save(&mut rs).unwrap();

        let s = store(tmp.path());
        let st = load(&s, "c1", "sha1");
        assert_eq!(st.roundtrips, 0);
        assert_eq!(st.change_id, "c1");
    }
}
