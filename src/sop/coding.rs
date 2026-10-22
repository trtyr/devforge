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
use crate::sop::review_gate::{outcome_of_review_report, review_with_gate, ReviewBudget};

/// 回复尾巴（错误里带证据，供事后定位；按字符边界截取）。
pub fn tail(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(200);
    chars[start..].iter().collect()
}

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

/// 无有效标记时的重推话术（有界，005 decisions/015：连续 3 次不发标记 → 第 4 次判质量失败）。
pub const CONTINUE_NUDGE: &str = "\
继续当前阶段。阶段 todo 全部完成、单测全绿、文档同步后，\
在回复末尾单独一行输出 <stage-complete>N</stage-complete>（N 为阶段号）。";

/// 无标记重推上限：超过即质量失败（不自动重启，plan 010 D004）。
pub const MAX_CONTINUE_NUDGES: usize = 3;

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

/// 协议违例重试上限（C19-R1）：审查输出不合规时重跑审查而不是回注被审 Agent。
/// 单一事实源在 [`crate::sop::review_gate`]（D026），此处只做别名。
pub const MAX_PROTOCOL_VIOLATIONS: usize = crate::sop::review_gate::MAX_PROTOCOL_VIOLATIONS;

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
        // 审查预算的生命周期 = 整个 run（违例重审与实例失败重试都从这里计数）
        let mut budget = ReviewBudget::default();
        let mut nudges = 0usize;
        // 阶段推进记录：上一个阶段号（首个阶段的前驱 = "start"）
        let mut last_stage: Option<u64> = None;
        // 被打回后待收尾的修复轮（回注在下一轮 agent 回合才发生 → fix.end 落在那一回合返回处）
        let mut pending_fix: Option<u32> = None;

        loop {
            let text = self.agent.converse(&message, ui)?;
            // 上一轮打回的回注已随本回合交卷 → 修复轮收尾
            if let Some(round) = pending_fix.take() {
                tracing::info!(
                    component = "coding-engine",
                    event = "fix.end",
                    round,
                    "打回重写结束"
                );
            }
            let Some(stage) = parse_stage_complete(&text) else {
                // 无有效标记 → 有界重推（005/015）：标记由模型生成，卡住时不能无限打转
                nudges += 1;
                if nudges > MAX_CONTINUE_NUDGES {
                    return Err(crate::error::DevforgeError::Quality(format!(
                        "连续 {nudges} 次未输出 <stage-complete>，判定为质量失败（不自动重启）；最近一次回复尾巴：{}",
                        tail(&text)
                    )));
                }
                message = CONTINUE_NUDGE.to_string();
                continue;
            };
            nudges = 0;
            // 阶段推进：agent 报出的新阶段号（前一个阶段 → 当前阶段）。
            // 重发同一阶段号（打回后重交）**不是**推进 → 不落里程碑（from/to 同值 = 噪声）。
            if last_stage != Some(stage) {
                tracing::info!(
                    component = "coding-engine",
                    event = "phase.change",
                    from = last_stage.map(|s| s.to_string()).unwrap_or_else(|| "start".into()),
                    to = stage.to_string(),
                    "MVP coding 阶段推进"
                );
                last_stage = Some(stage);
            }

            // 本轮审查：全新实例，用完即弃。
            // C19-R1 + D026：协议违例（审查输出本身不合规）→ 换新实例重审（新 fresh 实例），
            // 绝不把违例问题当作代码问题回注给 Coding Agent；实例失败（空回复/早退/通道错误）
            // → 换新实例重试；两者超上限分别判质量失败与基础设施故障。
            // 审查轮次口径（Q6）：`review_rounds` 是**已开过的实例数** → 本轮序号 = review_rounds + 1（首审 = 1）。
            let review_round = (review_rounds + 1) as u32;
            // 「审查开庭」里程碑：在**发起**那一刻落（不是等审完）
            tracing::info!(
                component = "coding-engine",
                event = "review.open",
                role = "coding-review",
                round = review_round,
                "审查开庭"
            );
            let (report, opened) = review_with_gate(
                &mut self.review_factory,
                review_rounds,
                |round| coding_review_instruction(workspace, round),
                &mut budget,
                |text| Ok(outcome_of_review_report(parse_review_report(text)?)),
            )?;
            review_rounds += opened;
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
                    // 打回：fix 起止一对（含轮次）——回注在下一轮 agent 回合发生
                    tracing::info!(
                        component = "coding-engine",
                        event = "fix.start",
                        round = review_round,
                        "打回重写开始"
                    );
                    pending_fix = Some(review_round);
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
    fn review_open_and_fix_round_are_logged_as_milestones() {
        // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
        // ②打回重写不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
        let agent = MockAgent::new(&[
            Turn::Say("阶段 1 完成。\n<stage-complete>1</stage-complete>"),
            Turn::ExpectContains(
                "登录没处理过期 token",
                "修好了。\n<stage-complete>1</stage-complete>",
            ),
        ]);
        let mut factory = MockReviewFactory::new(&[FAIL_FIX_ONE, PASS_FINISH]);
        let mut user = NoUser;
        let text = crate::observability::test_support::capture_jsonl(|| {
            CodingSopLoop::new(agent, &mut factory)
                .run(&mut user, Path::new("/w"), "p", "t")
                .unwrap();
        });

        // 审查开庭：两轮各一条，角色名与轮次（首审 = 1）
        let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
        assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
        assert!(
            opens[0].contains("\"role\":\"coding-review\""),
            "开庭要带角色名：{}",
            opens[0]
        );
        assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
        assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
        assert!(
            opens[0].contains("\"target\":\"devforge::sop::coding\""),
            "里程碑的 target 必须是发出它的模块：{}",
            opens[0]
        );

        // 打回重写：一对 fix.start / fix.end（含轮次）
        let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
        let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
        assert_eq!(starts.len(), 1, "打回应落一条 fix.start：{text}");
        assert_eq!(ends.len(), 1, "打回应落一条 fix.end：{text}");
        assert!(starts[0].contains("\"round\":1"), "打回轮次要带上：{}", starts[0]);
        assert!(ends[0].contains("\"round\":1"), "打回轮次要带上：{}", ends[0]);

        // 阶段推进：start → 1（重发同一阶段号不是推进 → 不重复落）
        let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
        assert_eq!(phases.len(), 1, "只真推进时落一条：{text}");
        assert!(
            phases[0].contains("\"from\":\"start\"") && phases[0].contains("\"to\":\"1\""),
            "首个阶段从 start 推进：{}",
            phases[0]
        );
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
    fn repeated_missing_marker_fails_fast() {
        // 005/015：连续 4 次不发标记 → 质量失败（不无限空转）
        let agent = MockAgent::new(&[
            Turn::Say("还在写"),
            Turn::Say("还在写"),
            Turn::Say("还在写"),
            Turn::Say("就是不发标记"),
        ]);
        let mut factory = MockReviewFactory::new(&[]);
        let mut user = NoUser;
        let err = CodingSopLoop::new(agent, &mut factory)
            .run(&mut user, Path::new("/w"), "p", "t")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("stage-complete"), "{msg}");
        assert!(msg.contains("质量失败"), "{msg}");
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
