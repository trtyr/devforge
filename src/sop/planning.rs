//! 规划循环状态机（P001 · decisions/008 零校验流转 + decisions/004 Review 无状态）。
//!
//! 状态流转：
//! ```text
//! PLAN_EDITING（Plan Agent 与用户对话，强制 ask-user-question 确认完成）
//!   → 完成标记 → PLAN_HOLD（本抽象中 Plan 会话保活，跨轮次复用）
//! REVIEWING（每轮全新 Review 实例，从头查）
//!   → PASS → 阶段完成（decisions/008 零校验，编排器信任 verdict）
//!   → FAIL → Review 实例即弃 → 问题清单回注活着的 Plan Agent → 自动重查（无上限）
//! ```
//!
//! Review 永不碰用户（P004 边界）；用户交互只在 Plan 环节内（ask-user-question）。

use crate::error::{DevforgeError, Result};
use crate::sop::marker::{MarkerNudge, MarkerStep};
use crate::sop::output::{is_complete, parse_verdict, Verdict};
use crate::sop::review_gate::{review_with_gate, ReviewBudget, ReviewOutcome};

/// Plan Agent 开场指令（角色 + 强制完成确认 + 完成标记约定）。
pub const PLAN_KICKOFF: &str = "\
你是规划 Agent：读取 plan-tree skill，与用户沟通，产出 plan tree 到当前目录（任务目录即 plan root）。\n\
规则：\n\
1. 规划过程中用户确认类问题必须用 ask-user-question 询问。\n\
2. 结束前【必须】用 ask-user-question 问用户是否完成；用户确认后才算完成。\n\
3. 完成后，在你的最终回复末尾单独一行输出：<plan-complete>";

/// Review 检查指令（每轮全新实例都收到同一份）。
pub const REVIEW_INSTRUCTION: &str = "\
你是规划检查 Agent：读取 plan tree 与 plan-tree skill，只检查语义是否通顺、前后是否矛盾。\n\
不评设计好坏、不找潜在坑、不与用户交互。\n\
在回复末尾输出 verdict 块：\n\
通过：<verdict>PASS</verdict>\n\
不通过：<verdict>FAIL</verdict> 加 <issues> 清单（每行一条，`- ` 开头）。";

/// 打回回注前缀。
pub const REMEDIATION_PREFIX: &str = "Review 发现以下语义问题，请修订 plan tree：";

/// 用户链路抽象：Plan Agent 的 ask-user-question 经此抵达用户并带回回答。
/// 真身 = P003 API 中继（前端/MVP 裸 HTTP）；测试 = 脚本回答。
pub trait UserLink {
    fn ask(&mut self, question: &str, options: &[&str]) -> Result<String>;
}

/// Plan Agent 会话（干活者：跨轮次复用，上下文连续——decisions/004）。
pub trait PlanAgentSession {
    /// 与 Plan Agent 一轮对话。UI 问答（ask-user-question）在实现内部经 `ui` 发生。
    fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String>;
}

impl<P: PlanAgentSession + ?Sized> PlanAgentSession for &mut P {
    fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String> {
        (**self).converse(message, ui)
    }
}

/// Review 会话（检查者：用完即弃，decisions/004）。
pub trait ReviewSession {
    /// 丢检查指令，等结果，返回 Review 最终文本。
    fn review(&mut self, instruction: &str) -> Result<String>;

    /// 带墙钟的 review（第三方 r3 M4）：默认忽略墙钟（脚本替身无阻塞语义）；
    /// 真 RPC 会话覆写成「到点返回 Quality」。
    fn review_with_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        let _ = deadline;
        self.review(instruction)
    }
}

/// Review 会话工厂（每轮全新实例，不带前轮记忆）。
pub trait ReviewSessionFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>>;
}

impl<F: ReviewSessionFactory + ?Sized> ReviewSessionFactory for &mut F {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        (**self).open()
    }
}

/// 阶段报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    /// Review 轮数。
    pub rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
}

pub struct PlanningLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    pub plan: P,
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> PlanningLoop<P, F> {
    pub fn new(plan: P, review_factory: F) -> Self {
        Self { plan, review_factory }
    }

    /// 跑完规划阶段直至 Review 通过。
    ///
    /// 零硬限制（decisions/003）：打回轮次无上限；零校验（decisions/008）：
    /// Plan Agent 修复后直接自动重查，不验修复质量、不再问用户。
    pub fn run(&mut self, ui: &mut dyn UserLink) -> Result<StageReport> {
        // ---- PLAN 阶段：循环直至完成标记（完成判定在 Agent 环节内）----
        // 追补计数走全仓单点（自审 minor 78）：这里用零上限入口，decisions/003 不设硬限制。
        let mut gate = MarkerNudge::unlimited();
        let mut message = PLAN_KICKOFF.to_string();
        loop {
            let text = self.plan.converse(&message, ui)?;
            match gate.observe(is_complete(&text)) {
                MarkerStep::Done => break,
                MarkerStep::Nudge(_) => {
                    message = "继续。完成前记得 ask-user-question 向用户确认，并在最终回复输出完成标记。".into();
                }
                MarkerStep::Exhausted(n) => {
                    // unlimited() 的 limit = usize::MAX，计数不可能到达——到了说明计数本身坏了，
                    // 属基础设施异常（System，可重启），不是内容不合格。
                    return Err(DevforgeError::System(format!(
                        "规划阶段标记追补计数溢出（nudges={n}），计数纪律失效"
                    )));
                }
            }
        }
        // 阶段推进：规划阶段交卷 → 进入审查阶段（里程碑：阶段怎么变的）
        tracing::info!(
            component = "planning-engine",
            event = "phase.change",
            from = "plan",
            to = "review",
            "MVP planning 阶段推进"
        );

        // ---- REVIEW 循环：每轮全新实例；FAIL 打回后零校验自动重查 ----
        let mut rounds = 0usize;
        let mut issues_total = 0usize;
        // 预算的生命周期 = 整个 review 循环（违例重审与实例失败重试都从这里计数）
        let mut budget = ReviewBudget::default();
        loop {
            // 协议违例 → 换全新实例重审（≤3，超限质量失败，绝不把违例当 issue 回注规划 agent）；
            // 判负（FAIL + 非空 issues）→ 照旧回注活着的规划 agent（零校验自动重查）。
            // 审查轮次口径（Q6）：`rounds` 是**已开过的实例数** → 本轮序号 = rounds + 1（首审 = 1）。
            let review_round = rounds + 1;
            // 「审查开庭」里程碑：在**发起**那一刻落（不是等审完）
            tracing::info!(
                component = "planning-engine",
                event = "review.open",
                role = "plan-review",
                round = review_round,
                "审查开庭"
            );
            let (verdict, opened) = review_with_gate(
                &mut self.review_factory,
                rounds,
                |_| REVIEW_INSTRUCTION.to_string(),
                &mut budget,
                |text| {
                    let v = parse_verdict(text)?;
                    let violation = v.violation_reason();
                    Ok(ReviewOutcome {
                        report: v,
                        violation,
                    })
                },
            )?;
            rounds += opened;
            match verdict {
                Verdict::Pass => {
                    return Ok(StageReport { rounds, issues_total });
                }
                Verdict::Fail { issues } => {
                    issues_total += issues.len();
                    let remediation = format!(
                        "{REMEDIATION_PREFIX}\n{}",
                        issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    // 判负回注：fix 起止一对（含轮次）——回注活着的 Plan Agent（它的上下文连续）；
                    // 修完零校验自动重查
                    tracing::info!(
                        component = "planning-engine",
                        event = "fix.start",
                        round = review_round,
                        "判负回注开始"
                    );
                    let _ = self.plan.converse(&remediation, ui)?;
                    tracing::info!(
                        component = "planning-engine",
                        event = "fix.end",
                        round = review_round,
                        "判负回注结束"
                    );
                }
                Verdict::ProtocolViolation { reason } => {
                    // 门已把违例收进「换新实例重审」通道：走到这里 = 门与解析器形状不一致
                    return Err(crate::error::DevforgeError::System(format!(
                        "审查协议违例未进门：{reason}"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;
