//! 文档修补循环状态机（P007 · D004 写文档模式 + D008 doc-review 内置审查轮）。
//!
//! ```text
//! DOC_FIX（Doc Fix Agent 单轮工作：读 findings(doc-side) → 逐条修文档 → 回写处置摘要）
//!   → doc-review 内置审查（每轮全新实例，D008：一致性/自洽/Runbook 契约/findings 完整性）
//!   → PASS → 返回（编排层恢复 testing，D004 流回测试环节）
//!   → FAIL → 问题清单回注 Agent → 修订 → 重审（零上限）
//! ```
//!
//! 与 CodingSopLoop 的差异：无阶段推进（doc-fix 是定点修补，不是分阶段开发），
//! 审查裁决只有 PASS/FAIL（无 advance/finish，协议见 testing_protocol::parse_doc_review_report）。

use std::path::Path;

use crate::error::Result;
use crate::sop::coding_protocol::StageVerdict;
use crate::sop::doc::protocol::MAX_DOC_FIX_FAILS;
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};
use crate::sop::testing_protocol::{parse_doc_review_report, FINDINGS_DIR};

/// Doc Fix Agent 开场指令：黑板（cwd）+ 代码工作区 + findings 位置 + 待补文档位置（D004）。
pub fn docfix_kickoff(workspace: &Path, project: &str, task: &str) -> String {
    format!(
        "你是文档修补 Agent：项目 `{project}` / 任务 `{task}`。\
\n当前目录是共享黑板（任务目录）：测试审查判定文档不足（doc-side），由你按发现补齐文档。\
\n代码工作区（triage 时允许读代码核实事实）：{}\n\
findings 清单在 {}（处置 doc-side 项），待补文档在 coding/docs/。\
逐条处置：修文档、补 Runbook、回写处置摘要。本轮只改文档，不改代码。",
        workspace.display(),
        FINDINGS_DIR
    )
}

/// doc-review 审查指令：全新实例 kickoff（黑板与工作区路径 + 轮次号）。
pub fn doc_review_instruction(workspace: &Path, round: usize) -> String {
    format!(
        "你是文档审查 Agent（第 {round} 轮）。\
\n黑板 = 当前目录（plan tree、coding/docs/ 与 testing/findings/ 都在这）；代码工作区 = {}\n\
按你的提示词执行全部审查面：findings 处置完整性、plan tree 一致性、内部自洽、Runbook 七件套，\
先写 testing/reports/doc-review-{round}.md，再在回复末尾输出协议块（stage-verdict / issues / notes）。",
        workspace.display()
    )
}

/// 打回回注前缀（doc-review FAIL）。
pub const DOC_FIX_REMEDIATION_PREFIX: &str = "文档审查未通过，以下是必须修复的问题：";

/// 协议违例重试上限（C19-R1）：超过即判基础设施故障，快速报错。
pub const MAX_DOC_PROTOCOL_VIOLATIONS: usize = 3;

/// 文档修补 SOP 报告。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocFixReport {
    /// doc-review 审查轮数（含首轮；打回重审累计）。
    pub review_rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
}

pub struct DocFixSopLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    /// Doc Fix Agent 会话（打回重审在同一会话继续——保活语义沿 coding 家族）。
    pub agent: P,
    /// doc-review 实例工厂（每轮全新，用完即弃）。
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> DocFixSopLoop<P, F> {
    pub fn new(agent: P, review_factory: F) -> Self {
        Self {
            agent,
            review_factory,
        }
    }

    /// 跑完文档修补 SOP 直至 doc-review 判 PASS（零上限：打回轮次无上限）。
    pub fn run(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        project: &str,
        task: &str,
    ) -> Result<DocFixReport> {
        let mut report = DocFixReport::default();

        // 单轮工作：按 findings 修文档（完成即止，无阶段协议）
        let message = docfix_kickoff(workspace, project, task);
        self.agent.converse(&message, ui)?;

        // 审查循环：PASS 出口，FAIL 打回（回注问题清单）后重审
        let mut violations = 0usize;
        // 自审 critical：FAIL 轮上限——doc-review 判 FAIL 但不带 <issues> 时，回注的
        // 是空问题清单，打回无上限 = run 无限空转（每圈真烧两次 LLM）。
        let mut fail_rounds = 0usize;
        loop {
            report.review_rounds += 1;
            // 「审查开庭」里程碑：在**发起**那一刻落
            // 轮次口径（Q6）：doc-fix 是**先自增再开实例** → 落 `report.review_rounds`，**不加 1**（首审 = 1）。
            tracing::info!(
                component = "docfix-engine",
                event = "review.open",
                role = "doc-review",
                round = report.review_rounds,
                "审查开庭"
            );
            let mut session = self.review_factory.open()?;
            // 自审 critical：审查走墙钟——裸 review() 绕过仓内统一墙钟，doc SOP 默认
            // 无失活收割，审查实例挂死 = run 永挂并占住串行槽（server #6 同款）。
            let review_text = session.review_with_deadline(
                &doc_review_instruction(workspace, report.review_rounds),
                crate::sop::review_gate::review_deadline(),
            )?;
            drop(session);

            let parsed = parse_doc_review_report(&review_text);
            // C19-R1：审查输出本身不合规 → 重跑审查，不回注 doc-fix Agent
            if parsed.protocol_violation {
                violations += 1;
                if violations > MAX_DOC_PROTOCOL_VIOLATIONS {
                    return Err(crate::error::DevforgeError::Quality(format!(
                        "文档审查连续 {} 轮输出协议违例（缺 stage-verdict），判定为质量失败（不自动重启），停止 run：{}",
                        violations,
                        parsed.issues.join("；")
                    )));
                }
                continue;
            }
            match parsed.verdict {
                StageVerdict::Pass => return Ok(report),
                StageVerdict::Fail => {
                    fail_rounds += 1;
                    if fail_rounds > MAX_DOC_FIX_FAILS {
                        return Err(crate::error::DevforgeError::Quality(format!(
                            "文档审查连续 {fail_rounds} 轮 FAIL（上限 {MAX_DOC_FIX_FAILS}，末轮 issues {} 条），判定为质量失败（不自动重启），停止 run",
                            parsed.issues.len()
                        )));
                    }
                    report.issues_total += parsed.issues.len();
                    let remediation = format!(
                        "{DOC_FIX_REMEDIATION_PREFIX}\n{}",
                        parsed
                            .issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    // 打回回注：fix 起止一对（含轮次）
                    tracing::info!(
                        component = "docfix-engine",
                        event = "fix.start",
                        round = report.review_rounds,
                        "打回重写开始"
                    );
                    self.agent.converse(&remediation, ui)?;
                    tracing::info!(
                        component = "docfix-engine",
                        event = "fix.end",
                        round = report.review_rounds,
                        "打回重写结束"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "fix_tests.rs"]
mod tests;
