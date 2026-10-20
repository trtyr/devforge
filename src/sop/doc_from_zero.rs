//! doc-from-zero 编排循环（P008 · D009/D010/D016/D020）。
//!
//! ```text
//! 门 0（引擎侧，run 启动前）：清单 + 闭包 + 单元编号 → 写 _meta/inventory.json
//!   ↓
//! 逐单元：doc-agent 写本单元 → <doc-complete>unit-x</doc-complete>
//!   → doc-review（每轮全新实例）→ PASS → 提交 + 写 state.json → 下一单元
//!                              → FAIL → <issues> + <route> 回注 → 按 route 修
//!   → PASS + route=finish（或单元耗尽）→ 收工
//! ```
//!
//! 零硬限制：单元数、打回轮次、重推次数均无上限；审查协议违例按 C19-R1 口径重跑审查（上限 3）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};
use crate::sop::doc_protocol::{
    MAX_DOC_CONTINUE_NUDGES, MAX_DOC_PROTOCOL_VIOLATIONS, MAX_DOC_REVIEW_RETRIES, DocVerdict,
    parse_doc_complete, parse_doc_review,
};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};

/// 产出目录（D016）：目标项目内。
pub const DOCS_DIR: &str = "docs/contexts";
/// 元数据目录（清单/词表/置信度/缺口/检查点）。
pub const META_DIR: &str = "docs/contexts/_meta";
/// 检查点（D016：不占项目根）。
pub const STATE_FILE: &str = "docs/contexts/_meta/state.json";

/// 无有效完成标记时的重推话术。
pub const DOC_CONTINUE_NUDGE: &str = "\
继续当前单元。写完并自检（必填节 / 实体存在 / 链接 / Runbook 七件套 / doc-lint 退出码 0）后，\
在回复末尾单独一行输出 <doc-complete>unit-xxx</doc-complete>（xxx 为 kickoff 给的单元号）。";

/// 打回回注前缀：必须带回 route，让 agent 回对那一层。
pub const DOC_REMEDIATION_PREFIX: &str = "审查未通过，以下是必须修复的问题（请按 route 回到对应阶段修）：";

/// 单元完成状态（断点续跑，D002/T008）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocState {
    /// 已通过审查并落盘的单元号（预分配编号，重跑不漂移——D020）。
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub review_rounds: usize,
    #[serde(default)]
    pub issues_total: usize,
}

impl DocState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx(format!("建目录 {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).ctx(format!("写检查点 {}", path.display()))?;
        Ok(())
    }

    pub fn is_done(&self, unit: &str) -> bool {
        self.done.iter().any(|u| u == unit)
    }
}

/// doc-agent 开场指令：告知仓库、产出目录、本单元、文档档位。
pub fn doc_kickoff(repo: &Path, unit: &str, doc_level: &str) -> String {
    format!(
        "你是文档 Agent：本单元 `{unit}`（档位 `{doc_level}`）。\
\n目标仓库（只读，除了产出目录）：{}\n产出目录：{DOCS_DIR}/（本单元的所有文档写这里；元数据写 {META_DIR}/）\
\n清单：{META_DIR}/inventory.json（先读它，本单元包含哪些文件/符号/入口）\
\n纪律：只写 {DOCS_DIR}/ 下；codegraph-first；勘察只记事实、解释标 🟢/🟡/🔴；按 doc-contract 写、不适用写 N/A。\
\n交审前自检后用 <doc-complete>{unit}</doc-complete> 收尾。",
        repo.display()
    )
}

/// 审查指令：全新实例，带轮次号与单元号。
pub fn doc_review_instruction(unit: &str, round: usize) -> String {
    format!(
        "你是文档审查 Agent（第 {round} 轮，审单元 `{unit}`）。\
\n审查面：覆盖（清单条目 ↔ 文档，按档位集合）、完整性（必填节 / N-A 规则）、真实性（实体存在）、置信度诚实性（标 🟢 必须给得出 path:line）、可跑（**照 Runbook 从零 boot 一遍**：装依赖→配置→启动→健康检查→种子数据，跑不起来 = FAIL）。\
\n先写 {META_DIR}/../_meta/reviews/review-{round}.md（或 kickoff 指定处），再输出协议块：<doc-verdict>PASS|FAIL</doc-verdict> + <route>...</route> + <issues>...</issues>。\
\n引擎审计是确定性事实（覆盖/lint/Runbook 命令块），直接采信并逐条验收，不要重算。"
    )
}

/// 终审指令（同一 doc-review profile，全新实例）：
/// 跨模块重复、总览/汇编引用一致性、全量 lint 与覆盖——单元审看不到的东西只在这里查。
pub fn doc_final_instruction(audit: &str) -> String {
    format!(
        "你是文档审查 Agent（**终审轮**，全库范围）。\
\n审查面（单元审没看到的部分）：\
1. **跨模块重复**：引擎分两段给——「同名出现（提示级）」只是提示（实体名在多篇里互相引用是正常写法，**不单独判 FAIL**）；「跨模块重复正文」才是判据（同一段解释被两篇以上具体描述 = FAIL，应改成引用）。\
2. **汇编层引用**：总览/架构/数据流里的每个引用是否指向存在的叶子文档；上层断言能否溯源到叶子证据。\
3. **全量 lint 与覆盖**：引擎审计已给退出码与缺口清单，逐条验收（禁止重算）。\
4. **置信度诚实性抽样**：抽 3 条标 🟢 的断言，看能不能真给 path:line。\
\n## 引擎审计（确定性，勿质疑）\n\n{audit}\n\n写 {META_DIR}/../_meta/reviews/final.md，然后输出协议块：<doc-verdict>PASS|FAIL</doc-verdict> + <route>...</route> + <issues>...</issues>。"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocReport {
    pub units_done: Vec<String>,
    pub review_rounds: usize,
    pub issues_total: usize,
}

/// 引擎侧审计提供者：输入单元号，返回确定性审计文本（覆盖 + lint）。
pub type AuditProvider = Box<dyn Fn(&str) -> String + Send>;

pub struct DocSopLoop<P: PlanAgentSession, F: ReviewSessionFactory> {
    pub agent: P,
    pub review_factory: F,
    /// 检查点文件（`_meta/state.json`）。
    pub state_path: PathBuf,
    /// 只跑这一个单元（`--only`，幂等重派）。
    pub only: Option<String>,
    /// 引擎侧确定性审计（每轮审查前算好，作为审查输入注入——D012：不靠 agent 自述）。
    pub audit: Option<AuditProvider>,
}

impl<P: PlanAgentSession, F: ReviewSessionFactory> DocSopLoop<P, F> {
    pub fn new(agent: P, review_factory: F, state_path: PathBuf) -> Self {
        Self {
            agent,
            review_factory,
            state_path,
            only: None,
            audit: None,
        }
    }

    pub fn with_only(mut self, unit: impl Into<String>) -> Self {
        self.only = Some(unit.into());
        self
    }

    /// 注入引擎侧审计（覆盖 + lint 摘要）；审查官看到的是确定性事实。
    pub fn with_audit(mut self, audit: AuditProvider) -> Self {
        self.audit = Some(audit);
        self
    }

    /// 跑完全部单元（或被 `only` 限定的一单元）。
    pub fn run(
        &mut self,
        ui: &mut dyn UserLink,
        repo: &Path,
        units: &[String],
        doc_level: &str,
    ) -> Result<DocReport> {
        let mut state = DocState::load(&self.state_path);
        let mut protocol_violations = 0usize;

        for unit in units {
            if let Some(only) = &self.only
                && only != unit
            {
                continue;
            }
            if state.is_done(unit) {
                continue;
            }

            let mut message = doc_kickoff(repo, unit, doc_level);
            let mut nudges = 0usize;
            loop {
                let text = self.agent.converse(&message, ui)?;
                let Some(done_unit) = parse_doc_complete(&text) else {
                    nudges += 1;
                    if nudges > MAX_DOC_CONTINUE_NUDGES {
                        return Err(DevforgeError::System(format!(
                            "单元 `{unit}` 连续 {nudges} 次未输出 <doc-complete>，判定为基础设施故障（不无限重推）"
                        )));
                    }
                    message = DOC_CONTINUE_NUDGE.to_string();
                    continue;
                };
                nudges = 0;
                if done_unit != *unit {
                    // 单元号不匹配：可能是预分配漂移，按错报处理（D020 编号必须稳定）
                    message = format!(
                        "你输出的是 `{done_unit}`，但本单元是 `{unit}`。请确认编号（不得自行改号）并重新输出 <doc-complete>{unit}</doc-complete>。"
                    );
                    continue;
                }

                // 本轮审查：全新实例，用完即弃；协议违例重跑审查（C19-R1 口径）
                let mut review_retries = 0usize;
                let report = loop {
                    state.review_rounds += 1;
                    let mut session = self.review_factory.open()?;
                    let mut instruction = doc_review_instruction(unit, state.review_rounds);
                    if let Some(audit) = &self.audit {
                        instruction.push_str("\n\n## 引擎审计（确定性，勿质疑）\n\n");
                        instruction.push_str(&audit(unit));
                    }
                    // 空回复 / 进程异常：换新实例重试，而不是把整个 run 判死（2026-10-20 实测）
                    let review_text = match session.review(&instruction) {
                        Ok(t) if !t.trim().is_empty() => t,
                        Ok(_) => {
                            drop(session);
                            review_retries += 1;
                            if review_retries > MAX_DOC_REVIEW_RETRIES {
                                return Err(DevforgeError::System(format!(
                                    "单元 `{unit}` 审查会话连续 {review_retries} 次返回空回复，判定为基础设施故障"
                                )));
                            }
                            tracing::warn!(
                                component = "doc-sop",
                                round = state.review_rounds,
                                "审查空回复，换新实例重试"
                            );
                            continue;
                        }
                        Err(e) => {
                            drop(session);
                            review_retries += 1;
                            if review_retries > MAX_DOC_REVIEW_RETRIES {
                                return Err(DevforgeError::System(format!(
                                    "单元 `{unit}` 审查会话连续 {review_retries} 次失败（最后一次：{e}）"
                                )));
                            }
                            tracing::warn!(
                                component = "doc-sop",
                                round = state.review_rounds,
                                error = %e,
                                "审查会话异常，换新实例重试"
                            );
                            continue;
                        }
                    };
                    drop(session);

                    let report = parse_doc_review(&review_text)?;
                    if !report.protocol_violation {
                        break report;
                    }
                    protocol_violations += 1;
                    if protocol_violations > MAX_DOC_PROTOCOL_VIOLATIONS {
                        return Err(DevforgeError::System(format!(
                            "审查连续 {protocol_violations} 轮输出协议违例（缺 doc-verdict/route 块），判定为基础设施故障：{}",
                            report.issues.join("；")
                        )));
                    }
                    tracing::warn!(
                        component = "doc-sop",
                        round = state.review_rounds,
                        violations = protocol_violations,
                        "文档审查协议违例，重跑本轮审查（不回注被审 Agent）"
                    );
                };

                match report.verdict {
                    DocVerdict::Pass => {
                        state.done.push(unit.clone());
                        state.save(&self.state_path)?;
                        tracing::info!(
                            component = "doc-sop",
                            unit,
                            round = state.review_rounds,
                            "单元通过并落盘"
                        );
                        // PASS + route=finish 只意味着「这个单元收口了」，**不是整个 run 结束**。
                        // 2026-10-19 实测：审查官对第一个单元就回了 finish，旧逻辑直接 return，
                        // 后面的 unit-root-02 压根没写，run 却报了 Settled。
                        break;
                    }
                    DocVerdict::Fail => {
                        state.issues_total += report.issues.len();
                        let mut msg = format!(
                            "{DOC_REMEDIATION_PREFIX}\n<route>{}</route>\n",
                            report.route.as_str()
                        );
                        for i in &report.issues {
                            msg.push_str(&format!("- {i}\n"));
                        }
                        message = msg;
                    }
                }
            }
        }

        Ok(DocReport {
            units_done: state.done,
            review_rounds: state.review_rounds,
            issues_total: state.issues_total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::planning::ReviewSession;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    struct NoUser;
    impl UserLink for NoUser {
        fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
            Err(DevforgeError::System(format!("测试不应有用户交互: {q}")))
        }
    }

    struct MockAgent {
        turns: VecDeque<String>,
        received: Rc<RefCell<Vec<String>>>,
    }
    impl MockAgent {
        fn new(turns: &[&str]) -> Self {
            Self {
                turns: turns.iter().map(|s| s.to_string()).collect(),
                received: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for MockAgent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.received.borrow_mut().push(message.into());
            Ok(self.turns.pop_front().expect("MockAgent 轮次耗尽"))
        }
    }

    struct MockReview {
        replies: Rc<RefCell<VecDeque<String>>>,
        instructions: Rc<RefCell<Vec<String>>>,
    }
    struct MockReviewFactory {
        replies: Rc<RefCell<VecDeque<String>>>,
        instructions: Rc<RefCell<Vec<String>>>,
    }
    impl ReviewSessionFactory for MockReviewFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            Ok(Box::new(MockReview {
                replies: self.replies.clone(),
                instructions: self.instructions.clone(),
            }))
        }
    }
    impl ReviewSession for MockReview {
        fn review(&mut self, instruction: &str) -> Result<String> {
            self.instructions.borrow_mut().push(instruction.into());
            Ok(self
                .replies
                .borrow_mut()
                .pop_front()
                .expect("MockReview 轮次耗尽"))
        }
    }

    fn factory(replies: &[&str]) -> (MockReviewFactory, Rc<RefCell<Vec<String>>>) {
        let replies = Rc::new(RefCell::new(
            replies.iter().map(|s| s.to_string()).collect::<VecDeque<_>>(),
        ));
        let instructions = Rc::new(RefCell::new(Vec::new()));
        (
            MockReviewFactory {
                replies,
                instructions: instructions.clone(),
            },
            instructions,
        )
    }

    fn state_path(tmp: &tempfile::TempDir) -> PathBuf {
        tmp.path().join("docs/contexts/_meta/state.json")
    }

    #[test]
    fn two_units_pass_and_checkpoint_written() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完 A\n<doc-complete>unit-a-01</doc-complete>",
            "写完 B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let (f, _) = factory(&[
            "<doc-verdict>PASS</doc-verdict>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let rep = lp.run(&mut NoUser, tmp.path(), &units, "complete").unwrap();
        assert_eq!(rep.units_done, units);
        assert_eq!(rep.review_rounds, 2);
        let saved = DocState::load(&state_path(&tmp));
        assert_eq!(saved.done, units);
    }

    #[test]
    fn empty_review_reply_is_retried_with_fresh_instance() {
        // 2026-10-20 实测：doc-review 返回空回复（模型一字未出），旧行为把整个 run 判死
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
        let (f, instructions) = factory(&["", "<doc-verdict>PASS</doc-verdict>"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let rep = lp
            .run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap();
        assert_eq!(rep.units_done, vec!["unit-a-01"]);
        assert_eq!(instructions.borrow().len(), 2, "空回复应换实例重试一次");
    }

    #[test]
    fn repeated_empty_reviews_fail_fast() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
        let (f, _) = factory(&["", "", "", ""]);
        let err = DocSopLoop::new(agent, f, state_path(&tmp))
            .run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("空回复"), "{msg}");
    }

    #[test]
    fn missing_marker_is_bounded_not_infinite() {
        // 零上限的旧口径在这里就是无限空转：连续不发标记必须报错
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["想想", "再想想", "还在想", "就是不写"]);
        let (f, _) = factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let err = DocSopLoop::new(agent, f, state_path(&tmp))
            .run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap_err();
        assert!(format!("{err}").contains("doc-complete"), "{err}");
    }

    #[test]
    fn pass_with_finish_route_does_not_end_the_run() {
        // 2026-10-19 实测：审查官对第一个单元就回 PASS+finish，旧逻辑直接 return，
        // 第二个单元压根没写，run 却报 Settled。
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完 A\n<doc-complete>unit-a-01</doc-complete>",
            "写完 B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let (f, _) = factory(&[
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let rep = lp.run(&mut NoUser, tmp.path(), &units, "complete").unwrap();
        assert_eq!(rep.units_done, units, "finish 只收口本单元，不得结束整个 run");
        assert_eq!(rep.review_rounds, 2);
    }

    #[test]
    fn fail_message_contains_route_and_issues() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "v1\n<doc-complete>unit-a-01</doc-complete>",
            "v2\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let received = agent.received.clone();
        let (f, _) = factory(&[
            "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        lp.run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap();
        let msgs = received.borrow();
        assert!(msgs.len() >= 2);
        assert!(msgs[1].contains("<route>phase1</route>"));
        assert!(msgs[1].contains("事实错"));
    }

    #[test]
    fn audit_text_is_injected_into_review_instruction() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let (f, instructions) = factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp)).with_audit(Box::new(|unit: &str| {
            format!("覆盖审计：单元 {unit} 缺 2 条")
        }));
        lp.run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap();
        let ins = instructions.borrow();
        assert!(ins[0].contains("引擎审计（确定性，勿质疑）"));
        assert!(ins[0].contains("覆盖审计：单元 unit-a-01 缺 2 条"));
    }

    #[test]
    fn protocol_violation_retries_review_without_touching_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let received = agent.received.clone();
        let (f, _) = factory(&[
            "我审完了，写得不错。", // 缺协议块 → 违例
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let rep = lp
            .run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap();
        assert_eq!(rep.units_done.len(), 1);
        assert_eq!(rep.review_rounds, 2, "违例后重跑审查");
        assert_eq!(received.borrow().len(), 1, "agent 只被调用一次（不回注违例）");
    }

    #[test]
    fn violation_cap_aborts_run() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let (f, _) = factory(&["无协议块", "无协议块", "无协议块", "无协议块"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let err = lp
            .run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap_err();
        assert!(err.to_string().contains("协议违例"), "{err}");
    }

    #[test]
    fn done_units_are_skipped_on_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let state = DocState {
            done: vec!["unit-a-01".to_string()],
            review_rounds: 1,
            issues_total: 0,
        };
        state.save(&state_path(&tmp)).unwrap();

        let agent = MockAgent::new(&["B\n<doc-complete>unit-b-02</doc-complete>"]);
        let (f, _) = factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        let rep = lp
            .run(
                &mut NoUser,
                tmp.path(),
                &["unit-a-01".to_string(), "unit-b-02".to_string()],
                "complete",
            )
            .unwrap();
        assert_eq!(rep.units_done, vec!["unit-a-01", "unit-b-02"]);
        assert_eq!(rep.review_rounds, 2, "已完成的单元不再审");
    }

    #[test]
    fn only_limits_to_single_unit_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let state = DocState {
            done: vec!["unit-a-01".to_string()],
            review_rounds: 1,
            issues_total: 0,
        };
        state.save(&state_path(&tmp)).unwrap();

        let agent = MockAgent::new(&["B\n<doc-complete>unit-b-02</doc-complete>"]);
        let (f, _) = factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp)).with_only("unit-b-02");
        let rep = lp
            .run(
                &mut NoUser,
                tmp.path(),
                &["unit-a-01".to_string(), "unit-b-02".to_string()],
                "complete",
            )
            .unwrap();
        assert_eq!(rep.units_done, vec!["unit-a-01", "unit-b-02"]);
        assert_eq!(rep.review_rounds, 2);
    }

    #[test]
    fn unit_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完\n<doc-complete>unit-wrong-99</doc-complete>",
            "改正\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let received = agent.received.clone();
        let (f, _) = factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
        let mut lp = DocSopLoop::new(agent, f, state_path(&tmp));
        lp.run(&mut NoUser, tmp.path(), &["unit-a-01".to_string()], "complete")
            .unwrap();
        assert!(received.borrow()[1].contains("不得自行改号"));
    }
}
