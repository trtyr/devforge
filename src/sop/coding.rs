//! 编码循环状态机（P005 · decisions/002/003/011 + 零校验流转沿袭 P001 decisions/008）。
//!
//! ```text
//! CODING（Coding Agent 长驻：读树→自判 N 阶段→逐阶段 todo→开发）
//!   → <stage-complete>N</stage-complete> → 本轮审查
//!   → 无有效标记 → 回注「继续」重推（零上限，decisions/003）
//! REVIEWING（每轮全新 Coding Review 实例，从头查）
//!   → FAIL+fix    → 问题清单回注活着的 Agent → 修完重发标记 → 重审（无上限）
//!   → PASS+advance → 消化 <notes> 建议 → 进入阶段 N+1（同一会话继续）
//!   → PASS+finish  → 树全完成 → 收工
//! ```
//!
//! 审查 Agent 无状态用完即弃（decisions/004）；树完成判定锚点=对照实物（decisions/011）；
//! 阻塞通道 = ask-user-question 挂起（decisions/012），用户交互只发生在 Coding 环节内。

use std::path::Path;

use crate::error::Result;
use crate::sop::coding_protocol::{parse_review_report, parse_stage_complete, NextAction};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};

/// Coding Agent 开场指令：黑板（cwd）与代码工作区双路径告知（decisions/001），
/// 项目/任务名供 stages.md 与文档归位。
pub fn coding_kickoff(workspace: &Path, project: &str, task: &str) -> String {
    format!(
        "你是编码 Agent：项目 `{project}` / 任务 `{task}`。\
\n当前目录是共享黑板（任务目录）：读这里的 plan tree，按你的提示词开工。\
\n代码工作区（项目登记地址，decisions/001）：{}\n\
先勘察、再分阶段、然后逐阶段开发。文档写 coding/docs/，阶段划分写 coding/stages.md，todo 写 coding/todo.md。",
        workspace.display()
    )
}

/// 审查指令：全新 Review 实例的 kickoff（黑板与工作区路径 + 轮次号）。
pub fn coding_review_instruction(workspace: &Path, round: usize) -> String {
    format!(
        "你是编码审查 Agent（第 {round} 轮）。\
\n黑板 = 当前目录（plan tree 与 coding/ 子树都在这）；代码工作区 = {}\n\
按你的提示词执行全部审查面：真读代码、真跑单测，先写 coding/reports/review-{round}.md，\
再在回复末尾输出协议块（stage-verdict / next / issues / notes）。",
        workspace.display()
    )
}

/// 无有效标记时的重推话术（零上限，decisions/003）。
pub const CONTINUE_NUDGE: &str = "\
继续当前阶段。阶段 todo 全部完成、单测全绿、文档同步后，\
在回复末尾单独一行输出 <stage-complete>N</stage-complete>（N 为阶段号）。";

/// 打回回注前缀（FAIL+fix）。
pub const CODING_REMEDIATION_PREFIX: &str = "审查未通过，以下是必须修复的问题：";

/// PASS+advance 的推进话术（附非阻塞建议先消化，decisions/011/012 口径）。
fn advance_message(stage: u64, notes: &[String]) -> String {
    let mut msg = format!(
        "阶段 {stage} 审查通过。先消化以下非阻塞建议再开始阶段 {}：",
        stage + 1
    );
    if notes.is_empty() {
        msg = format!("阶段 {stage} 审查通过，进入阶段 {}。", stage + 1);
    } else {
        for n in notes {
            msg.push_str(&format!("\n- {n}"));
        }
    }
    msg
}

/// 协议违例重试上限（C19-R1）：审查输出不合规（缺协议块/未知值/自相矛盾）时
/// 重跑审查而不是回注被审 Agent；连续违例超过此数视为基础设施故障，快速报错。
pub const MAX_PROTOCOL_VIOLATIONS: usize = 3;

/// 编码 SOP 报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingReport {
    /// 完成并通过审查的阶段号（升序）。
    pub stages_completed: Vec<u64>,
    /// Review 轮数。
    pub review_rounds: usize,
    /// 累计打回的问题条数。
    pub issues_total: usize,
}

pub struct CodingSopLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    /// Coding Agent 会话（长驻，跨全部阶段与审查轮——decisions/004 保活语义）。
    pub agent: P,
    /// 审查实例工厂（每轮全新，用完即弃）。
    pub review_factory: F,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> CodingSopLoop<P, F> {
    pub fn new(agent: P, review_factory: F) -> Self {
        Self {
            agent,
            review_factory,
        }
    }

    /// 跑完编码 SOP 直至审查判定 finish（树全完成）。
    ///
    /// 零硬限制（decisions/003）：阶段数、重推次数、打回轮次均无上限；
    /// 零校验（decisions/008）：修复后直接重审，编排层不做业务判断。
    pub fn run(
        &mut self,
        ui: &mut dyn UserLink,
        workspace: &Path,
        project: &str,
        task: &str,
    ) -> Result<CodingReport> {
        let mut message = coding_kickoff(workspace, project, task);
        let mut stages_completed = Vec::new();
        let mut review_rounds = 0usize;
        let mut issues_total = 0usize;
        let mut protocol_violations = 0usize;

        loop {
            let text = self.agent.converse(&message, ui)?;
            let Some(stage) = parse_stage_complete(&text) else {
                // 无有效标记 → 重推（零上限；模型自然收敛）
                message = CONTINUE_NUDGE.to_string();
                continue;
            };

            // 本轮审查：全新实例，用完即弃。
            // C19-R1：协议违例（审查输出本身不合规）→ 重跑本轮审查（新 fresh 实例），
            // 绝不把违例问题当作代码问题回注给 Coding Agent。
            let report = loop {
                review_rounds += 1;
                let mut session = self.review_factory.open()?;
                let instruction = coding_review_instruction(workspace, review_rounds);
                let review_text = session.review(&instruction)?;
                drop(session);

                let report = parse_review_report(&review_text)?;
                if !report.protocol_violation {
                    break report;
                }
                protocol_violations += 1;
                if protocol_violations > MAX_PROTOCOL_VIOLATIONS {
                    return Err(crate::error::DevforgeError::Quality(format!(
                        "审查连续 {} 轮输出协议违例（缺少 stage-verdict/next 块），判定为质量失败（不自动重启），停止 run：{}",
                        protocol_violations,
                        report.issues.join("；")
                    )));
                }
                tracing::warn!(
                    component = "coding-sop",
                    round = review_rounds,
                    violations = protocol_violations,
                    "审查输出协议违例，重跑本轮审查（不回注被审 Agent）"
                );
            };
            match (report.verdict, report.next) {
                (crate::sop::coding_protocol::StageVerdict::Pass, NextAction::Finish) => {
                    stages_completed.push(stage);
                    return Ok(CodingReport {
                        stages_completed,
                        review_rounds,
                        issues_total,
                    });
                }
                (crate::sop::coding_protocol::StageVerdict::Pass, NextAction::Advance) => {
                    stages_completed.push(stage);
                    message = advance_message(stage, &report.notes);
                }
                // (Fail, Fix)——其余组合已被解析器保守归一为 (Fail, Fix)
                _ => {
                    issues_total += report.issues.len();
                    message = format!(
                        "{CODING_REMEDIATION_PREFIX}\n{}",
                        report
                            .issues
                            .iter()
                            .map(|i| format!("- {i}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::planning::ReviewSession;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    // ---- 无用户交互：编码 SOP 里用户只经 ask-user-question（会话内部），测试不给用户脚本 ----
    struct NoUser;
    impl UserLink for NoUser {
        fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
            Err(crate::error::DevforgeError::System(format!(
                "测试不应有用户交互: {q}"
            )))
        }
    }

    // ---- Coding Agent 脚本 ----
    enum Turn {
        /// 回复文本
        Say(&'static str),
        /// 断言收到的消息包含片段，然后回复
        ExpectContains(&'static str, &'static str),
    }
    impl Clone for Turn {
        fn clone(&self) -> Self {
            match self {
                Turn::Say(a) => Turn::Say(a),
                Turn::ExpectContains(a, b) => Turn::ExpectContains(a, b),
            }
        }
    }
    struct MockAgent {
        turns: VecDeque<Turn>,
        received: Rc<RefCell<Vec<String>>>,
    }
    impl MockAgent {
        fn new(turns: &[Turn]) -> Self {
            Self {
                turns: turns.iter().cloned().collect(),
                received: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for MockAgent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.received.borrow_mut().push(message.into());
            match self.turns.pop_front().expect("MockAgent 轮次耗尽") {
                Turn::Say(t) => Ok(t.into()),
                Turn::ExpectContains(needle, reply) => {
                    assert!(
                        message.contains(needle),
                        "应收到含 `{needle}` 的消息，实际: {message}"
                    );
                    Ok(reply.into())
                }
            }
        }
    }

    // ---- Review 工厂脚本 ----
    struct MockReview {
        reply: &'static str,
        closed: Rc<Cell<bool>>,
        expect_round: usize,
    }
    impl ReviewSession for MockReview {
        fn review(&mut self, instruction: &str) -> Result<String> {
            assert!(
                instruction.contains(&format!("第 {} 轮", self.expect_round)),
                "审查指令应带轮次号 {}: {instruction}",
                self.expect_round
            );
            Ok(self.reply.into())
        }
    }
    impl Drop for MockReview {
        fn drop(&mut self) {
            self.closed.set(true);
        }
    }
    #[derive(Default)]
    struct MockReviewFactory {
        replies: VecDeque<&'static str>,
        opened: Vec<Rc<Cell<bool>>>,
    }
    impl MockReviewFactory {
        fn new(replies: &[&'static str]) -> Self {
            Self {
                replies: replies.iter().copied().collect(),
                opened: Vec::new(),
            }
        }
    }
    impl ReviewSessionFactory for MockReviewFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            let closed = Rc::new(Cell::new(false));
            self.opened.push(closed.clone());
            // 轮次号 = 即将开始的第 N 轮（全局计数，不是实例内计数）
            let round = self.opened.len(); // push 后取长度 = 第 N 轮
            Ok(Box::new(MockReview {
                reply: self.replies.pop_front().expect("Review 回复耗尽"),
                closed,
                expect_round: round,
            }))
        }
    }

    const PASS_ADVANCE: &str =
        "合格。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
    const PASS_FINISH: &str = "全部达成。\n<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
    const FAIL_FIX_ONE: &str = "有问题。\n<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 登录没处理过期 token\n</issues>";

    fn kickoff_starts_with(ws: &Path, first: &str) -> bool {
        let k = coding_kickoff(ws, "demo", "t1");
        k.starts_with(first) && k.contains(ws.display().to_string().as_str())
    }

    #[test]
    fn kickoff_carries_dual_paths() {
        // decisions/001：双路径告知（黑板 cwd + 代码工作区注入）
        let ws = Path::new("/tmp/ws-x");
        assert!(kickoff_starts_with(ws, "你是编码 Agent：项目 `demo` / 任务 `t1`"));
        assert!(coding_review_instruction(ws, 3).contains("第 3 轮"));
        assert!(coding_review_instruction(ws, 3).contains("/tmp/ws-x"));
    }

    #[test]
    fn two_stages_advance_then_finish() {
        // 阶段推进：s1 完成→PASS+advance→s2 完成→PASS+finish 收工
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "进入阶段 2",
                "阶段 2 完成。\n<stage-complete>2</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[PASS_ADVANCE, PASS_FINISH]);
        let mut user = NoUser;
        let report =
            CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.stages_completed, vec![1, 2]);
        assert_eq!(report.review_rounds, 2);
        assert_eq!(report.issues_total, 0);
        // 每轮审查都是全新实例且已弃
        assert!(factory.opened.iter().all(|f| f.get()));
    }

    #[test]
    fn fail_then_fix_then_reemit_marker_then_advance() {
        // 打回重审：FAIL+fix → 问题回注 → Agent 修复后重发标记 → 重审通过
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "登录没处理过期 token",
                "修好了。\n<stage-complete>1</stage-complete>",
            ),
            Turn::Say("好了。\n<stage-complete>2</stage-complete>"),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_FIX_ONE, PASS_ADVANCE, PASS_FINISH]);
        let mut user = NoUser;
        let report =
            CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.stages_completed, vec![1, 2]);
        assert_eq!(report.review_rounds, 3);
        assert_eq!(report.issues_total, 1);
    }

    #[test]
    fn no_marker_gets_nudged() {
        // 无标记重推（零上限）：停转但没发标记 → CONTINUE_NUDGE 回注
        let agent = MockAgent::new(&[
            Turn::Say("我停下来想想"),
            Turn::ExpectContains(
                "stage-complete",
                "阶段 1 完成。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let rx = std::rc::Rc::clone(&agent.received);
        let mut factory = MockReviewFactory::new(&[PASS_FINISH]);
        let mut user = NoUser;
        let report =
            CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.stages_completed, vec![1]);
        // 收到的第二条消息就是重推话术
        assert_eq!(rx.borrow()[1], CONTINUE_NUDGE);
    }

    #[test]
    fn advance_digests_notes_before_next_stage() {
        // PASS+advance 附非阻塞建议 → 回注消息含建议（先消化再开工）
        let adv_with_notes: &'static str =
            "合格，有建议。\n<stage-verdict>PASS</stage-verdict>\n<next>advance</next>\n<notes>\n- 补个边界测试\n- 命名收敛\n</notes>";
        let agent = MockAgent::new(&[
            Turn::Say("<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "补个边界测试",
                "阶段 2 完成。\n<stage-complete>2</stage-complete>",
            ),
            Turn::ExpectContains(
                "进入阶段 3",
                "<stage-complete>3</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[adv_with_notes, PASS_ADVANCE, PASS_FINISH]);
        let mut user = NoUser;
        let report =
            CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.stages_completed, vec![1, 2, 3]);
    }

    #[test]
    fn protocol_violation_retries_review_not_agent() {
        // C19-R1：审查输出协议违例（缺块）→ **重跑审查**（新 fresh 实例），
        // 不把「审查官输出格式问题」回注给被审 Agent；issues_total 也不应累计。
        let agent = MockAgent::new(&[Turn::Say("<stage-complete>1</stage-complete>")]);
        let mut factory = MockReviewFactory::new(&["我忘了输出协议块", PASS_FINISH]);
        let mut user = NoUser;
        let report =
            CodingSopLoop::new(agent, &mut factory).run(&mut user, Path::new("/w"), "p", "t").unwrap();
        assert_eq!(report.stages_completed, vec![1]);
        assert_eq!(report.review_rounds, 2, "违例后重跑了第二轮审查");
        assert_eq!(report.issues_total, 0, "违例不计入代码问题数");
        assert_eq!(factory.opened.len(), 2, "两个审查实例都开过且均被丢弃");
    }

    #[test]
    fn repeated_protocol_violations_fail_fast() {
        // 连续违例超上限（3）→ 基础设施故障报错，不无限重试、不回注
        let agent = MockAgent::new(&[Turn::Say("<stage-complete>1</stage-complete>")]);
        let mut factory = MockReviewFactory::new(&[
            "缺块1", "缺块2", "缺块3", "缺块4",
        ]);
        let mut user = NoUser;
        let err = CodingSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap_err();
        assert!(
            err.to_string().contains("协议违例"),
            "应报协议违例基础设施故障: {err}"
        );
    }
}
