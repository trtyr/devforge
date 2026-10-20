//! change SOP 的真实 `ChangeSession`（P009 接线）：把 pi 子进程会话与审查实例接到编排器。
//!
//! 分工：本层只做「对话 + 解析 + 调机械门」，不做业务判断——
//! 判断在提示词（agent）与机械门（`plan_audit` / `coding_gate` / `boundary` / `testing_gate`）里。
//!
//! 所有子会话都是注入的 trait 对象（`PlanAgentSession` / `ReviewSessionFactory`），
//! 因此本层可以完全用假实现做单测，不需要真跑 pi。

use std::path::{Path, PathBuf};

use crate::change::boundary::{parse_route, replan_instruction, ImpactGap};
use crate::change::coding_gate::{changed_files, check_baseline_log, check_step_commits, read_steps};
use crate::change::gate0::DocLevel;
use crate::change::orchestrator::{ChangeSession, CodingTurn, PlanTurn, TestingTurn};
use crate::change::plan_audit::{audit_plan_file, review_instruction};
use crate::change::testing_gate::{
    check_regression_log, parse_acceptance_table, REGRESSION_LOG,
};
use crate::error::{Context as _, DevforgeError, Result};
use crate::sop::coding_protocol::{parse_review_report, parse_stage_complete, StageVerdict};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};

/// 一次 change run 的固定上下文。
#[derive(Debug, Clone)]
pub struct ChangeCtx {
    /// 目标仓（真代码）。
    pub repo: PathBuf,
    /// 黑板（`plans/<p>/<t>`，impl-steps / evidence 在这）。
    pub blackboard: PathBuf,
    /// 计划目录（目标仓 `docs/plantree/changes/<id>`）。
    pub change_dir: PathBuf,
    pub change_id: String,
    pub baseline: String,
    /// 用户需求原文（plan kickoff 要用）。
    pub requirement: String,
    pub doc_level: DocLevel,
}

impl ChangeCtx {
    pub fn plan_path(&self) -> PathBuf {
        self.change_dir.join("change-plan.md")
    }

    pub fn acceptance_path(&self) -> PathBuf {
        self.blackboard.join("testing/acceptance.md")
    }
}

/// 三阶段会话（生产：真 pi；单测：假实现）。
pub struct ProdChangeSession {
    pub plan_agent: Box<dyn PlanAgentSession>,
    pub plan_review: Box<dyn ReviewSessionFactory>,
    pub coding_agent: Box<dyn PlanAgentSession>,
    pub coding_review: Box<dyn ReviewSessionFactory>,
    pub test_agent: Box<dyn PlanAgentSession>,
    pub test_review: Box<dyn ReviewSessionFactory>,
    pub ctx: ChangeCtx,
}

// ---------------- 提示词 ----------------

pub fn plan_kickoff(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-plan-agent。change-id：`{}`\n\
目标仓：{}\n\
需求：{}\n\
文档档位：{}（计划阶段照常读已有文档；不要自己去跑文档流水线）\n\n\
产出：`{}`（六块：需求复述与歧义 / 影响面（每条 path:line）/ 三张表 / 白名单 / 验收标准 / 风险）。\n\
工具纪律：结构问题一律走 codegraph；不要写实施顺序，不要写回归命令，不要改代码。\n\
完成后在回复末尾单独一行输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.repo.display(),
        ctx.requirement,
        ctx.doc_level.as_str(),
        ctx.plan_path().display()
    )
}

pub fn coding_kickoff(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-coding-agent。change-id：`{}`，基线 `{}`。\n\
目标仓：{}\n计划：{}\n黑板（当前目录）：{}\n\n\
顺序：先跑既有测试并把命令与退出码写进 `evidence/baseline-test.log`（红就停）；\
再写 `impl-steps.md`（每步：文件 / 对应验收标准 / 怎么验），按步执行、每步一提交（提交信息含 change-id 与 `step N`）。\n\
只改计划白名单内的文件；越界必须输出 `<route>plan</route>` + `<impact-gap>`。\n\
完成后输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.baseline,
        ctx.repo.display(),
        ctx.plan_path().display(),
        ctx.blackboard.display()
    )
}

pub fn testing_kickoff(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-test-agent。change-id：`{}`。\n\
目标仓：{}\n验收标准来自计划：{}\n\
黑板（当前目录）：{}\n\n\
顺序：先跑既有全套回归并把命令与退出码写进 `{}`；再为变更点写测试；\
把对账表写到 `testing/acceptance.md`（每行 `- <验收标准原文> → <测试文件>:<行>`）；\
至少做一次反向验证。\n\
失败时输出 `<route>coding</route>`（计划问题用 `<route>plan</route>`）并附失败摘要；\
通过时输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.repo.display(),
        ctx.plan_path().display(),
        ctx.blackboard.display(),
        REGRESSION_LOG
    )
}

// ---------------- 内部小工具 ----------------

fn require_marker(stage: &str, reply: &str) -> Result<()> {
    if parse_stage_complete(reply).is_none() {
        return Err(DevforgeError::Quality(format!(
            "{stage} 未输出 <stage-complete>N（最近一次回复尾巴：{}）",
            crate::sop::coding::tail(reply)
        )));
    }
    Ok(())
}

fn issues_of(reply: &str) -> Vec<String> {
    let Some(start) = reply.find("<issues>") else {
        return Vec::new();
    };
    let body_start = start + "<issues>".len();
    let body_end = reply[body_start..]
        .find("</issues>")
        .map(|i| body_start + i)
        .unwrap_or(reply.len());
    reply[body_start..body_end]
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*']).trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn review_once(
    factory: &mut dyn ReviewSessionFactory,
    instruction: &str,
) -> Result<crate::sop::coding_protocol::ReviewReport> {
    let mut session = factory.open()?;
    let text = session.review(instruction)?;
    drop(session);
    if text.trim().is_empty() {
        return Err(DevforgeError::System(
            "审查会话无有效回复（进程正常结束但未产出一字）——按基础设施故障处理".into(),
        ));
    }
    parse_review_report(&text)
}

impl ProdChangeSession {
    /// plan / amend 共用：发指令 → 等标记 → 审计计划 → 审查。
    fn run_plan_with(&mut self, ui: &mut dyn UserLink, message: &str) -> Result<PlanTurn> {
        let reply = self.plan_agent.converse(message, ui)?;
        require_marker("change-plan-agent", &reply)?;
        let audit = audit_plan_file(&self.ctx.plan_path(), &self.ctx.repo).map_err(|e| {
            DevforgeError::Quality(format!(
                "计划文件不可读（{}）：{e}",
                self.ctx.plan_path().display()
            ))
        })?;
        let report = review_once(
            &mut *self.plan_review,
            &review_instruction(&self.ctx.change_id, &audit),
        )?;
        if report.protocol_violation {
            return Err(DevforgeError::Quality(format!(
                "变更计划审查协议违例：{}",
                report.issues.join("；")
            )));
        }
        match report.verdict {
            StageVerdict::Pass => Ok(PlanTurn::Pass),
            StageVerdict::Fail => Ok(PlanTurn::Fail {
                issues: report.issues,
            }),
        }
    }
}

impl ChangeSession for ProdChangeSession {
    fn run_plan(&mut self, ui: &mut dyn UserLink, _roundtrip: usize) -> Result<PlanTurn> {
        let message = plan_kickoff(&self.ctx);
        self.run_plan_with(ui, &message)
    }

    fn amend_plan(
        &mut self,
        ui: &mut dyn UserLink,
        gaps: &[ImpactGap],
        roundtrip: usize,
    ) -> Result<PlanTurn> {
        let message = format!(
            "{}\n\n补充上下文：目标仓 {}，计划文件 {}。",
            replan_instruction(gaps, roundtrip),
            self.ctx.repo.display(),
            self.ctx.plan_path().display()
        );
        self.run_plan_with(ui, &message)
    }

    fn run_coding(&mut self, ui: &mut dyn UserLink, _roundtrip: usize) -> Result<CodingTurn> {
        // 回归前置：红基线不许动代码（引擎只验证据，不自己跑）
        check_baseline_log(&self.ctx.blackboard)?;
        let reply = self.coding_agent.converse(&coding_kickoff(&self.ctx), ui)?;
        require_marker("change-coding-agent", &reply)?;
        // 实施步骤与按步提交必须对得上
        let steps = read_steps(&self.ctx.blackboard)?;
        check_step_commits(&self.ctx.repo, &self.ctx.baseline, &self.ctx.change_id, &steps)?;
        let changed = changed_files(&self.ctx.repo, &self.ctx.baseline)?;
        Ok(CodingTurn { reply, changed })
    }

    fn run_testing(&mut self, ui: &mut dyn UserLink) -> Result<TestingTurn> {
        let reply = self.test_agent.converse(&testing_kickoff(&self.ctx), ui)?;
        let route = parse_route(&reply);
        match route.as_deref() {
            Some("coding") => {
                let failures = issues_of(&reply);
                return Ok(TestingTurn::RouteCoding {
                    failures: if failures.is_empty() {
                        vec!["测试阶段判定失败（未给出 issues 明细）".into()]
                    } else {
                        failures
                    },
                });
            }
            Some("plan") => {
                let failures = issues_of(&reply);
                return Ok(TestingTurn::RoutePlan {
                    failures: if failures.is_empty() {
                        vec!["测试阶段判定需求/计划问题（未给出 issues 明细）".into()]
                    } else {
                        failures
                    },
                });
            }
            _ => {}
        }
        require_marker("change-test-agent", &reply)?;
        // 声称通过 → 回归日志必须是绿的
        check_regression_log(&self.ctx.blackboard, true)?;
        // 验收对账表由 agent 落盘，引擎逐条读
        let table_text = std::fs::read_to_string(self.ctx.acceptance_path()).ctx(format!(
            "缺少验收对账表 {}（每条验收标准一行 `- 标准 → 测试:行`）",
            self.ctx.acceptance_path().display()
        ))?;
        let table = parse_acceptance_table(&table_text);
        let done: Vec<String> = table.iter().map(|m| m.criterion.clone()).collect();
        // 表里每条都必须指到具体测试（path:line）
        for m in &table {
            if crate::change::plan_audit::path_refs(&m.test_ref).is_empty() {
                return Err(DevforgeError::Quality(format!(
                    "验收项 `{}` 没有指到测试 path:line（实际写的是 `{}`）",
                    m.criterion, m.test_ref
                )));
            }
        }
        Ok(TestingTurn::Pass {
            acceptance_done: done,
        })
    }
}

/// 只读检查：计划文件是否已存在（编排层判断能否续跑时用）。
pub fn plan_exists(ctx: &ChangeCtx) -> bool {
    ctx.plan_path().is_file()
}

/// 黑板上的基线证据路径（测试注入用）。
pub fn baseline_log_path(blackboard: &Path) -> PathBuf {
    blackboard.join(crate::change::coding_gate::BASELINE_LOG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::planning::{ReviewSession, ReviewSessionFactory};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::process::Command;
    use std::rc::Rc;

    struct NoUser;
    impl UserLink for NoUser {
        fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
            Err(DevforgeError::System(format!("单测不应问用户：{q}")))
        }
    }

    struct Agent {
        replies: VecDeque<String>,
        seen: Rc<RefCell<Vec<String>>>,
    }
    impl Agent {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: replies.iter().map(|s| s.to_string()).collect(),
                seen: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for Agent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.seen.borrow_mut().push(message.to_string());
            Ok(self.replies.pop_front().expect("Agent 脚本耗尽"))
        }
    }

    struct OneReview {
        queue: Rc<RefCell<VecDeque<String>>>,
        seen: Rc<RefCell<Vec<String>>>,
    }
    impl ReviewSession for OneReview {
        fn review(&mut self, instruction: &str) -> Result<String> {
            self.seen.borrow_mut().push(instruction.to_string());
            Ok(self.queue.borrow_mut().pop_front().expect("Review 脚本耗尽"))
        }
    }
    struct ReviewFactory {
        queue: Rc<RefCell<VecDeque<String>>>,
        seen: Rc<RefCell<Vec<String>>>,
    }
    impl ReviewFactory {
        fn new(replies: &[&str]) -> Self {
            Self {
                queue: Rc::new(RefCell::new(replies.iter().map(|s| s.to_string()).collect())),
                seen: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl ReviewSessionFactory for ReviewFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            Ok(Box::new(OneReview {
                queue: self.queue.clone(),
                seen: self.seen.clone(),
            }))
        }
    }

    const PLAN_PASS: &str = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
    const PLAN_FAIL: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 影响面漏了调用方\n</issues>";

    fn plan_text() -> String {
        "# 变更计划\n\n\
## 1 需求复述\n改 a。\n\n\
## 2 影响面\n- `src/a.rs:1`\n\n\
## 3 要改的\n| `src/a.rs` | 需求 |\n\n\
## 4 白名单\n`src/a.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
            .to_string()
    }

    struct Fixture {
        tmp: tempfile::TempDir,
        ctx: ChangeCtx,
    }

    fn fixture(with_git: bool) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
        let blackboard = tmp.path().join("bb");
        std::fs::create_dir_all(&blackboard).unwrap();
        let change_dir = repo.join("docs/plantree/changes/change-1");
        std::fs::create_dir_all(&change_dir).unwrap();
        std::fs::write(change_dir.join("change-plan.md"), plan_text()).unwrap();
        let mut baseline = String::new();
        if with_git {
            git(&repo, &["init", "-q"]);
            git(&repo, &["add", "-A"]);
            let out = Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "baseline",
                ])
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success());
            baseline = String::from_utf8_lossy(
                &Command::new("git")
                    .args(["rev-parse", "HEAD"])
                    .current_dir(&repo)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .trim()
            .to_string();
        }
        Fixture {
            ctx: ChangeCtx {
                repo,
                blackboard,
                change_dir,
                change_id: "change-1".into(),
                baseline,
                requirement: "把 a 改成 b".into(),
                doc_level: DocLevel::None,
            },
            tmp,
        }
    }

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn plan_pass_runs_audit_and_review_with_audit_injected() {
        let f = fixture(false);
        let agent = Agent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
        let seen_agent = agent.seen.clone();
        let plan_review = ReviewFactory::new(&[PLAN_PASS]);
        let seen_review = plan_review.seen.clone();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(agent),
            plan_review: Box::new(plan_review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_plan(&mut NoUser, 0).unwrap();
        assert_eq!(turn, PlanTurn::Pass);
        let kick = &seen_agent.borrow()[0];
        assert!(kick.contains("change-1"), "kickoff 要带 change-id：{kick}");
        assert!(kick.contains("change-plan.md"), "kickoff 要给计划路径：{kick}");
        let ins = &seen_review.borrow()[0];
        assert!(ins.contains("勿质疑、勿重算"), "审查指令要注入审计：{ins}");
        assert!(ins.contains("path:line"), "审查指令要带判据：{ins}");
    }

    #[test]
    fn plan_fail_carries_issues_back() {
        let f = fixture(false);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(ReviewFactory::new(&[PLAN_FAIL])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        match session.run_plan(&mut NoUser, 0).unwrap() {
            PlanTurn::Fail { issues } => assert!(issues.iter().any(|i| i.contains("调用方"))),
            other => panic!("应为 Fail，实际 {other:?}"),
        }
    }

    #[test]
    fn plan_without_marker_is_quality_failure() {
        let f = fixture(false);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["就是不发标记"])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_plan(&mut NoUser, 0).unwrap_err();
        assert!(err.to_string().contains("stage-complete"), "{err}");
    }

    fn coding_ready(f: &Fixture) -> usize {
        // 基线绿 + 一步实施 + 一次提交（与 impl-steps 对得上）
        std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
        std::fs::write(
            f.ctx.blackboard.join("evidence/baseline-test.log"),
            "command: cargo test\nexit_code: 0\n",
        )
        .unwrap();
        std::fs::write(
            f.ctx.blackboard.join("impl-steps.md"),
            "# 实施步骤\n## 步骤 1 改 a\n",
        )
        .unwrap();
        std::fs::write(f.ctx.repo.join("src/a.rs"), "// a changed\n").unwrap();
        git(&f.ctx.repo, &["add", "-A"]);
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@localhost",
                "commit",
                "-q",
                "-m",
                "change: change-1 step 1",
            ])
            .current_dir(&f.ctx.repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        1
    }

    #[test]
    fn coding_runs_gates_and_returns_changed_files() {
        let f = fixture(true);
        coding_ready(&f);
        let agent = Agent::new(&["改完了\n<stage-complete>1</stage-complete>"]);
        let seen = agent.seen.clone();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_coding(&mut NoUser, 0).unwrap();
        assert!(turn.changed.contains(&"src/a.rs".to_string()), "{:?}", turn.changed);
        assert!(seen.borrow()[0].contains("baseline-test.log"), "kickoff 要提基线证据");
    }

    #[test]
    fn coding_blocks_on_red_baseline_before_agent_runs() {
        let f = fixture(true);
        std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
        std::fs::write(
            f.ctx.blackboard.join("evidence/baseline-test.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .unwrap();
        let agent = Agent::new(&[]);
        let seen = agent.seen.clone();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_coding(&mut NoUser, 0).unwrap_err();
        assert!(err.to_string().contains("基线不是绿的"), "{err}");
        assert!(seen.borrow().is_empty(), "红基线不得先跑 agent");
    }

    #[test]
    fn testing_pass_reads_acceptance_table() {
        let f = fixture(true);
        std::fs::create_dir_all(f.ctx.blackboard.join("testing")).unwrap();
        std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
        std::fs::write(
            f.ctx.blackboard.join("evidence/regression.log"),
            "command: cargo test\nexit_code: 0\n",
        )
        .unwrap();
        std::fs::write(
            f.ctx.blackboard.join("testing/acceptance.md"),
            "- 给定 x=1 → 返回 2 → tests/a_test.rs:10\n",
        )
        .unwrap();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&["通过了\n<stage-complete>1</stage-complete>"])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        match session.run_testing(&mut NoUser).unwrap() {
            TestingTurn::Pass { acceptance_done } => {
                assert_eq!(acceptance_done, vec!["给定 x=1 → 返回 2".to_string()])
            }
            other => panic!("应为 Pass，实际 {other:?}"),
        }
    }

    #[test]
    fn testing_route_coding_returns_failures() {
        let f = fixture(true);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[
                "回归红了\n<route>coding</route>\n<issues>\n- tests/a_test.rs:10 断言失败\n</issues>",
            ])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        match session.run_testing(&mut NoUser).unwrap() {
            TestingTurn::RouteCoding { failures } => {
                assert!(failures.iter().any(|f| f.contains("断言失败")), "{failures:?}")
            }
            other => panic!("应回注 coding，实际 {other:?}"),
        }
    }

    #[test]
    fn acceptance_table_without_test_ref_is_rejected() {
        let f = fixture(true);
        std::fs::create_dir_all(f.ctx.blackboard.join("testing")).unwrap();
        std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
        std::fs::write(
            f.ctx.blackboard.join("evidence/regression.log"),
            "command: cargo test\nexit_code: 0\n",
        )
        .unwrap();
        std::fs::write(
            f.ctx.blackboard.join("testing/acceptance.md"),
            "- 给定 x=1 → 返回 2 → （没写测试）\n",
        )
        .unwrap();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_testing(&mut NoUser).unwrap_err();
        assert!(err.to_string().contains("没有指到测试"), "{err}");
        let _ = &f.tmp;
    }
}

