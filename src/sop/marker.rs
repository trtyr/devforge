//! 完成标记追补的**计数纪律**单点（自审 minor 78）。
//!
//! 四条链路都在做同一件事——「agent 这轮没交完成标记，就再要一次；连续要了
//! 这么多次还不交，判质量失败」：
//!
//! | 链路 | 标记 | 上限 |
//! | --- | --- | --- |
//! | [`crate::change`] 计划/编码/测试轮 | `<stage-complete>N</stage-complete>` | [`MAX_MARKER_NUDGES`] |
//! | [`crate::sop::planning`] | `<plan-complete>` | 零上限（decisions/003） |
//! | [`crate::sop::testing`] | `<stage-complete>` | [`MAX_TEST_CONTINUE_NUDGES`] |
//! | [`crate::sop::doc::runner`] | `<doc-complete>unit</doc-complete>` | [`MAX_DOC_CONTINUE_NUDGES`]，按当前单元计 |
//!
//! 以前是四份独立实现，各自手写 `nudges += 1` / `> MAX` / 清零。计数规则本来就是
//! 一条（**同一对象上连续失败才累加，交了就清零，超限即质量失败**），四份实现意味着
//! 「错号要不要计数」这类问题被答了四遍（doc 那条就曾因为错号不计而永不放行）。
//!
//! 单点化只收**计数纪律**：话术与错误文案仍由各链路自持——标记名、单元号、回复尾巴
//! 这些东西天然是链路特有的，强行统一只会把四条不同的诊断信息压成一句没用的套话。
//!
//! 超限之后**不自动重启**（这是四条链路一致的判定）：模型不配合是内容问题，
//! 换个实例重跑同样的提示词只会烧额度。

/// 追补决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerStep {
    /// 本轮已交标记：计数清零，调用方继续推进。
    Done,
    /// 没交：计数已 +1（未超限），调用方把追补话术送回去再问一轮。
    Nudge(usize),
    /// 没交且已超过上限：计数 = 上限 + 1。调用方按自己的话术报质量失败。
    Exhausted(usize),
}

/// 标记追补计数器。
#[derive(Debug, Clone)]
pub struct MarkerNudge {
    limit: usize,
    nudges: usize,
}

impl MarkerNudge {
    /// 有上限：连续 `limit` 次没交标记仍放行追补，第 `limit + 1` 次判超限。
    pub fn bounded(limit: usize) -> Self {
        Self { limit, nudges: 0 }
    }

    /// 零上限（`decisions/003`：MVP 规划阶段不设硬限制）。
    ///
    /// `observe` 永远不会给出 [`MarkerStep::Exhausted`]——调用方仍需处理该分支，
    /// 但那是「不可能发生」的 System 类兜底，不是内容判定。
    pub fn unlimited() -> Self {
        Self {
            limit: usize::MAX,
            nudges: 0,
        }
    }

    /// 当前连续失败次数（用于话术里的「第 N 次」）。
    pub fn nudges(&self) -> usize {
        self.nudges
    }

    /// 上限（用于错误文案里的「已追补 N 次」）。
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// 交了就清零——换单元 / 进入下一轮时调用。
    pub fn reset(&mut self) {
        self.nudges = 0;
    }

    /// 计入一次失败：+1 并分类。
    ///
    /// 「错号」这类**也要计**的失败直接走这个入口——doc 链路曾有「错号不计上限」的
    /// 旧实现，模型反复答错号就能无限重问。
    pub fn bump(&mut self) -> MarkerStep {
        self.nudges += 1;
        if self.nudges > self.limit {
            MarkerStep::Exhausted(self.nudges)
        } else {
            MarkerStep::Nudge(self.nudges)
        }
    }

    /// 按「本轮是否交了标记」判定：交了 → [`MarkerStep::Done`] 并清零；没交 → [`MarkerNudge::bump`]。
    pub fn observe(&mut self, present: bool) -> MarkerStep {
        if present {
            self.reset();
            MarkerStep::Done
        } else {
            self.bump()
        }
    }
}

/// change 链路（计划/编码/测试/回注轮）的阶段完成标记追补上限：拒收 + 追补 ×3，
/// 与 c6 空箱拒收/重审裁决同口径。
pub const MAX_MARKER_NUDGES: usize = 3;

#[cfg(test)]
#[path = "marker_tests.rs"]
mod tests;
