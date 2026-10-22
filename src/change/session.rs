//! change SOP 的真实 `ChangeSession`（P009 接线）：把 pi 子进程会话与审查实例接到编排器。
//!
//! 分工：本层只做「对话 + 解析 + 调机械门」，不做业务判断——
//! 判断在提示词（agent）与机械门（`plan_audit` / `coding_gate` / `boundary` / `testing_gate`）里。
//!
//! 所有子会话都是注入的 trait 对象（`PlanAgentSession` / `ReviewSessionFactory`），
//! 因此本层可以完全用假实现做单测，不需要真跑 pi。

use std::path::{Path, PathBuf};

use crate::change::boundary::{parse_route, replan_instruction, ImpactGap};
use crate::change::coding_gate::{
    changed_files, check_baseline_log, check_self_check, check_step_commits, read_steps,
};
use crate::change::gate0::DocLevel;
use crate::change::orchestrator::{ChangeSession, CodingTurn, PlanTurn, TestingTurn};
use crate::change::plan_audit::review_instruction;
use crate::change::state::TestFailureContext;
use crate::change::testing_gate::{
    check_regression_log, parse_acceptance_table, REGRESSION_LOG,
};
use crate::error::{Context as _, DevforgeError, Result};
use crate::sop::coding_protocol::{parse_review_report, parse_stage_complete, StageVerdict};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};
use crate::sop::review_gate::{
    outcome_of_review_report, review_with_gate, ReviewBudget,
};

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
问询纪律：歧义条目标了 `[必须裁决]` 的，先用 ask-user-question 真问用户（不要自问自答、不要编造答复），\
把用户答复逐字写进同一条目的 `用户裁决：` 字段；不许带默认值自决放行。\n\
完成后在回复末尾单独一行输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.repo.display(),
        ctx.requirement,
        ctx.doc_level.as_str(),
        ctx.plan_path().display()
    )
}

/// 基线轮：只跑既有测试、不写代码（先把“绿不绿”变成事实，再允许动代码）。
pub fn baseline_kickoff(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-coding-agent，现在只做基线取证：**不要改任何代码，不要写测试**。\n\
目标仓：{}（cd 到那里跑测试，不是在黑板目录里跑）\n\
自己判断这个仓的测试怎么跑（README / CI 配置 / 包管理器），跑一遍既有全套，\n\
把结果写进 `{}`（相对黑板）：第一行 `command: <你跑的命令>`，并写一行 `exit_code: N`（真实退出码）。\n\
然后输出 `<stage-complete>1</stage-complete>`。\n\
基线红就如实写红——引擎会拦住，不许硬着头皮改代码。",
        ctx.repo.display(),
        crate::change::coding_gate::BASELINE_LOG
    )
}

/// coding 审查指令：diff ↔ 计划对账（越界、依据、测试类型、依赖、文档）。
pub fn coding_review_instruction(ctx: &ChangeCtx, changed: &[String]) -> String {
    format!(
        "你是 change-coding-review（change-id `{}`）。对账三份东西：计划 `{}`、黑板 `{}` 的 `impl-steps.md`、相对基线 `{}` 的 diff。\n\
本次改动文件（git diff --name-only）：{}\n\n\
逐项判：\n\
1. 每块 diff 是否能在计划里找到依据；无依据改动 = FAIL（**不得以 diff 大小作判据**）。\n\
2. 改动是否全在白名单内；越界是否有 `<route>plan</route>` + `<impact-gap>` 申报（有申报就不该进到你这一步）。\n\
3. 测试是否为单测（不是 e2e），断言不得全空或单边。\n\
4. 是否引入计划外依赖。\n\
5. 行为变了而 `docs/contexts/` 没跟、或 `devforge doc-lint` 不绿 = FAIL。\n\
6. **自证证据**（先证后报 / 行为断言）：黑板 `{}` 的命令与退出码是否真实、是否覆盖**本轮 diff**；被审方声称「跑过了」有没有可复核的证据；关键断言有没有写明「错实现形态」。证据与本轮 diff 对不上 = FAIL。\n\n\
先写 `coding/reports/review.md`，末尾输出：<stage-verdict>PASS|FAIL</stage-verdict> + <next>advance|fix</next> + <issues>（FAIL 时带 path:line）。",
        ctx.change_id,
        ctx.plan_path().display(),
        ctx.blackboard.display(),
        ctx.baseline,
        changed.join("、"),
        crate::change::coding_gate::SELF_CHECK_LOG
    )
}

/// 测试审查指令：验回归证据与验收对账（不重跑测试套件，只验证据与覆盖）。
pub fn testing_review_instruction(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-test-review（change-id `{}`）。黑板：{}；回归日志：{}；对账表：{}。\n\n\
逐项判：\n\
1. 回归真跑了：日志命令合理、退出码与结论一致（非零却称通过 = FAIL）。\n\
2. 验收对账表每条都指到具体测试 `path:line`，且不撒谎。\n\
3. 反向验证至少一处（破坏实现后测试会红）。\n\
4. 测试阶段不得改产品代码。\n\n\
先写 `testing/reports/review.md`，末尾输出：<stage-verdict>PASS|FAIL</stage-verdict> + <next>advance|fix</next> + <issues>。",
        ctx.change_id,
        ctx.blackboard.display(),
        REGRESSION_LOG,
        ctx.acceptance_path().display()
    )
}

/// coding 开工语。
///
/// `failure` = 上一轮测试失败的上下文（`None` = 无上下文）：
/// - `None` → 正文**逐字**等于改动前的开工语（不注入任何空块）；
/// - `Some` → 追加送达块（标题行 + 失败明细逐条原文），明细与回注面/日志面同源。
pub fn coding_kickoff(ctx: &ChangeCtx, failure: Option<&TestFailureContext>) -> String {
    let base = format!(
        "你是 change-coding-agent。change-id：`{}`，基线 `{}`。\n\
目标仓：{}\n计划：{}\n黑板（当前目录）：{}\n\n\
顺序：先跑既有测试并把命令与退出码写进 `evidence/baseline-test.log`（红就停）；\
再写 `impl-steps.md`（每步：文件 / 对应验收标准 / 怎么验），按步执行、每步一提交（提交信息含 change-id 与 `step N`）。\n\
只改计划白名单内的文件；越界必须输出 `<route>plan</route>` + `<impact-gap>`。\n\
本轮改完还要留**自证证据**（先证后报）：把本轮相关测试的**命令原文 + 真实退出码 + 输出摘要**写进黑板 `{}`（**覆盖写**，不许沿用上一轮残留）——引擎会验这份证据（存在 + 退出码为 0），缺了或红着会被质量门拦下。\n\
完成后输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.baseline,
        ctx.repo.display(),
        ctx.plan_path().display(),
        ctx.blackboard.display(),
        crate::change::coding_gate::SELF_CHECK_LOG
    );
    match failure {
        Some(f) => format!(
            "{base}\n\n{}",
            crate::change::testing_gate::coding_failure_block(f)
        ),
        None => base,
    }
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

/// 跑一次审查（开一个实例 → 审一次 → 解析），按三通道分类。
///
/// 返回（判负通道的报告，实际开过的实例数）：
/// - 协议违例（审查输出本身不合规）→ 换**全新实例**重审，超上限才 Quality（不自动重启）；
/// - 实例失败（空回复 / 进程早退 / 模型通道错误）→ 换新实例重试，超上限判基础设施故障（可重启）；
/// - 判负 → 交出报告与实例数（实例数即「审查轮次」，重审如实计入）。
fn review_once(
    factory: &mut dyn ReviewSessionFactory,
    instruction: &str,
) -> Result<(crate::sop::coding_protocol::ReviewReport, usize)> {
    let mut budget = ReviewBudget::default();
    review_with_gate(
        factory,
        0,
        |_| instruction.to_string(),
        &mut budget,
        |text| Ok(outcome_of_review_report(parse_review_report(text)?)),
    )
}

impl ProdChangeSession {
    /// plan / amend 共用：发指令 → 等标记 → 审计计划 → 审查。
    fn run_plan_with(&mut self, ui: &mut dyn UserLink, message: &str) -> Result<PlanTurn> {
        let reply = self.plan_agent.converse(message, ui)?;
        require_marker("change-plan-agent", &reply)?;
        // 计划文在目标仓，审查子进程 cwd 是黑板：必须把路径与全文一起给审查官
        let plan_path = self.ctx.plan_path();
        let plan_text = std::fs::read_to_string(&plan_path).map_err(|e| {
            DevforgeError::Quality(format!("计划文件不可读（{}）：{e}", plan_path.display()))
        })?;
        // 确定性硬门：标了「必须裁决」却没留痕 → 直接打回，不进审查轮
        // （确定性判定不交给审查官重判；打回走既有修复回路，轮次由 MAX_PLAN_FIXES 约束）
        let verdicts = crate::change::plan_audit::audit_verdicts(&plan_text);
        if !verdicts.is_clean() {
            return Ok(PlanTurn::Fail {
                issues: verdicts.issues,
            });
        }
        let audit = crate::change::plan_audit::audit_plan_file(&plan_path, &self.ctx.repo).map_err(
            |e| {
                DevforgeError::Quality(format!("计划文件不可读（{}）：{e}", plan_path.display()))
            },
        )?;
        // plan 侧的审查实例数（= 审查轮次）待步骤 9 落字段：`ProdChangeSession` 是字面量构造的，
        // 加字段必须改 `src/server/change_engine.rs`（不在计划 §4 白名单内，已按 impact-gap 申报）。
        let (report, _plan_instances) = review_once(
            &mut *self.plan_review,
            &review_instruction(&self.ctx.change_id, &plan_path, &plan_text, &audit),
        )?;
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

    fn fix_plan(
        &mut self,
        ui: &mut dyn UserLink,
        issues: &[String],
        round: usize,
    ) -> Result<PlanTurn> {
        let message = format!(
            "计划审查第 {round} 轮打回，逐条修复计划文件 `{}` 后重交：\n{}\n\n\
             只改计划文件本身（补全 path:line 全路径、补缺失块、修验收标准）；\
             问询纪律：若某条歧义标了 `[必须裁决]` 却缺 `用户裁决：` 留痕，先用 ask-user-question 问用户，\
把答复逐字写进该条目（不许带默认值自决放行）；\n\
             改完输出 <stage-complete>1</stage-complete> 等重审。",
            self.ctx.plan_path().display(),
            issues
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n")
        );
        self.run_plan_with(ui, &message)
    }

    fn run_coding(
        &mut self,
        ui: &mut dyn UserLink,
        _roundtrip: usize,
        last_failure: Option<&TestFailureContext>,
    ) -> Result<CodingTurn> {
        // ① 基线轮：先让 agent 只跑既有测试写证据（此时还没允许改代码）
        let baseline_reply = self.coding_agent.converse(&baseline_kickoff(&self.ctx), ui)?;
        require_marker("change-coding-agent（基线轮）", &baseline_reply)?;
        // 引擎不自己跑测试，只验证据：红/缺失就在这里拦住（coding 子进程不会再被叫去改代码）
        check_baseline_log(&self.ctx.blackboard)?;

        // ② coding 轮：真实改代码；越界申报按 D008 只发 <route>plan</route> + <impact-gap>，不打标记
        let reply = self
            .coding_agent
            .converse(&coding_kickoff(&self.ctx, last_failure), ui)?;
        if parse_route(&reply).as_deref() == Some("plan") {
            let changed = changed_files(&self.ctx.repo, &self.ctx.baseline).unwrap_or_default();
            return Ok(CodingTurn {
                reply,
                changed,
                reviews: 0,
            });
        }
        require_marker("change-coding-agent", &reply)?;
        // 先证后报：自证证据缺失或红 → Quality（不进审查）
        check_self_check(&self.ctx.blackboard)?;
        // 实施步骤与按步提交必须对得上
        let steps = read_steps(&self.ctx.blackboard)?;
        check_step_commits(&self.ctx.repo, &self.ctx.baseline, &self.ctx.change_id, &steps)?;
        let changed = changed_files(&self.ctx.repo, &self.ctx.baseline)?;

        // ③ coding review：diff ↔ 计划对账（越界已由引擎先算，这里管语义）
        let (report, instances) = review_once(
            &mut *self.coding_review,
            &coding_review_instruction(&self.ctx, &changed),
        )?;
        match report.verdict {
            StageVerdict::Pass => Ok(CodingTurn {
                reply,
                changed,
                reviews: instances,
            }),
            StageVerdict::Fail => Err(DevforgeError::Quality(format!(
                "变更编码审查未通过：{}",
                report.issues.join("；")
            ))),
        }
    }

    fn run_testing(&mut self, ui: &mut dyn UserLink) -> Result<TestingTurn> {        let reply = self.test_agent.converse(&testing_kickoff(&self.ctx), ui)?;
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
        // test review：验回归证据与对账（只验证据，不重跑套件）
        let (report, instances) = review_once(
            &mut *self.test_review,
            &testing_review_instruction(&self.ctx),
        )?;
        match report.verdict {
            StageVerdict::Pass => Ok(TestingTurn::Pass {
                acceptance_done: done,
                reviews: instances,
            }),
            StageVerdict::Fail => Err(DevforgeError::Quality(format!(
                "变更测试审查未通过：{}",
                report.issues.join("；")
            ))),
        }
    }

    fn plan_reviews(&self) -> usize {
        1
    }

    fn plan_facts(&mut self) -> Result<(Vec<String>, Vec<String>)> {
        let text = std::fs::read_to_string(self.ctx.plan_path()).ctx(format!(
            "读计划 {} 失败",
            self.ctx.plan_path().display()
        ))?;
        Ok(crate::change::plan_audit::plan_facts(&text))
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
### 1.1 歧义与本计划采用的默认\n\
- [自决] 命名细节由 agent 定\n\n\
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
    fn plan_facts_are_read_back_from_plan_file() {
        let f = fixture(false);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let (criteria, whitelist) = session.plan_facts().unwrap();
        assert_eq!(criteria, vec!["给定 x=1 → 返回 2".to_string()]);
        assert_eq!(whitelist, vec!["src/a.rs".to_string()]);
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

    /// 把计划文件里的歧义条目换成给定内容（其余块不动）。
    fn write_plan(f: &Fixture, body: &str) {
        let text = plan_text().replace("- [自决] 命名细节由 agent 定", body);
        std::fs::write(f.ctx.plan_path(), text).unwrap();
    }

    fn plan_session(f: &Fixture, plan_agent: Agent, plan_review: ReviewFactory) -> ProdChangeSession {
        ProdChangeSession {
            plan_agent: Box::new(plan_agent),
            plan_review: Box::new(plan_review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        }
    }

    #[test]
    fn plan_with_must_ask_without_answer_fails_before_review() {
        // 错实现形态：把「裁决门」从计划轮里摘掉 → 本用例必须变红（会变成 Pass）
        let f = fixture(false);
        write_plan(&f, "- [必须裁决] 复合词算一个词还是两个词");
        // 审查队列空：若硬门放行到审查，脚本耗尽直接 panic
        let plan_review = ReviewFactory::new(&[]);
        let seen_review = plan_review.seen.clone();
        let mut session = plan_session(
            &f,
            Agent::new(&["写完了\n<stage-complete>1</stage-complete>"]),
            plan_review,
        );
        match session.run_plan(&mut NoUser, 0).unwrap() {
            PlanTurn::Fail { issues } => assert!(
                issues.iter().any(|i| i.contains("必须裁决")),
                "打回问题要点名必须裁决：{issues:?}"
            ),
            other => panic!("应为 Fail，实际 {other:?}"),
        }
        assert!(
            seen_review.borrow().is_empty(),
            "硬门不过不得进审查轮（确定性判定不交审查官重判）"
        );
    }

    #[test]
    fn plan_with_must_ask_and_answer_passes_and_review_sees_verdict_facts() {
        // 错实现形态：把「裁决门事实注入审查指令」删掉 → 本用例必须变红
        let f = fixture(false);
        write_plan(
            &f,
            "- [必须裁决] 复合词算一个词还是两个词\n  用户裁决：算一个词",
        );
        let plan_review = ReviewFactory::new(&[PLAN_PASS]);
        let seen_review = plan_review.seen.clone();
        let mut session = plan_session(
            &f,
            Agent::new(&["写完了\n<stage-complete>1</stage-complete>"]),
            plan_review,
        );
        let turn = session.run_plan(&mut NoUser, 0).unwrap();
        assert_eq!(turn, PlanTurn::Pass, "留痕齐全的计划应当过审");
        let seen = seen_review.borrow();
        assert_eq!(seen.len(), 1, "审查轮次应当是 1");
        assert!(
            seen[0].contains("裁决门事实"),
            "审查指令要注入裁决门事实：{}",
            seen[0]
        );
        assert!(
            seen[0].contains("留痕核对") && seen[0].contains("分类恰当性"),
            "审查指令要带新审点：{}",
            seen[0]
        );
    }

    #[test]
    fn fix_plan_message_carries_the_ask_instruction() {
        // 错实现形态：把打回消息里的 ask 指令删掉 → 本用例必须变红
        let f = fixture(false);
        let agent = Agent::new(&["改好了\n<stage-complete>1</stage-complete>"]);
        let seen = agent.seen.clone();
        let mut session = plan_session(&f, agent, ReviewFactory::new(&[PLAN_PASS]));
        session
            .fix_plan(&mut NoUser, &["歧义条目缺留痕".to_string()], 1)
            .unwrap();
        let msg = &seen.borrow()[0];
        assert!(
            msg.contains("ask-user-question"),
            "打回消息要告诉 agent 下一步问用户：{msg}"
        );
        assert!(
            msg.contains("用户裁决"),
            "打回消息要点名留痕字段：{msg}"
        );
    }

    #[test]
    fn plan_kickoff_carries_the_ask_contract() {
        // 错实现形态：把 kickoff 里的问询契约删掉 → 本用例必须变红
        let f = fixture(false);
        let kick = plan_kickoff(&f.ctx);
        assert!(kick.contains("ask-user-question"), "开工语要含问询锚点：{kick}");
        assert!(kick.contains("用户裁决"), "开工语要含留痕字段锚点：{kick}");
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
            f.ctx.blackboard.join("evidence/self-check.log"),
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
    fn coding_kickoff_without_context_has_no_failure_block() {
        // 错实现形态：无上下文时也注入空块（标题行 + 空明细）→ 本用例必须变红
        let f = fixture(false);
        let kick = coding_kickoff(&f.ctx, None);
        assert!(
            !kick.contains(crate::change::testing_gate::FAILURE_BLOCK_TITLE),
            "无上下文时不得注入送达块：{kick}"
        );
        assert!(!kick.contains("失败明细"), "无上下文时不得注入明细：{kick}");
        assert!(
            kick.starts_with("你是 change-coding-agent。change-id：`change-1`"),
            "开工语首行不能变：{kick}"
        );
        assert!(
            kick.ends_with("完成后输出 `<stage-complete>1</stage-complete>`。"),
            "开工语末句不能变：{kick}"
        );
    }

    #[test]
    fn coding_kickoff_with_context_carries_the_failure_details_verbatim() {
        // 错实现形态：送达块漏标题行 / 漏一条明细 / 剥掉 path:line → 本用例必须变红
        let f = fixture(false);
        let ctx = TestFailureContext {
            round: 1,
            issues: vec![
                "tests/change_ask_ring.rs:557 断言失败".to_string(),
                "接口返回 500".to_string(),
            ],
        };
        let kick = coding_kickoff(&f.ctx, Some(&ctx));
        assert!(
            kick.contains(crate::change::testing_gate::FAILURE_BLOCK_TITLE),
            "送达块要有标题行：{kick}"
        );
        assert!(
            kick.contains("tests/change_ask_ring.rs:557"),
            "送达块要逐字带失败明细：{kick}"
        );
        assert!(kick.contains("接口返回 500"), "送达块要带全部明细：{kick}");
        assert!(!kick.contains("请自己定位原因"), "{kick}");
        assert!(!kick.contains("本消息不提供文件位置"), "{kick}");
        assert!(!kick.contains("<位置略>"), "{kick}");
    }

    #[test]
    fn coding_kickoff_with_context_only_appends_the_block() {
        // 错实现形态：注入上下文时把开工语本体也改了（不再是「只追加」）→ 本用例必须变红
        let f = fixture(false);
        let ctx = TestFailureContext {
            round: 1,
            issues: vec!["a.rs:1 失败".to_string()],
        };
        let plain = coding_kickoff(&f.ctx, None);
        let with = coding_kickoff(&f.ctx, Some(&ctx));
        assert_eq!(
            with,
            format!(
                "{plain}\n\n{}",
                crate::change::testing_gate::coding_failure_block(&ctx)
            ),
            "有上下文时只应在开工语后追加送达块"
        );
    }

    #[test]
    fn the_placeholder_issue_text_is_delivered_verbatim() {
        // 错实现形态：空 issues 时合成的占位文案被丢弃/改写 → 本用例必须变红
        let f = fixture(true);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&["回归红了\n<route>coding</route>"])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let TestingTurn::RouteCoding { failures } = session.run_testing(&mut NoUser).unwrap() else {
            panic!("应回注 coding");
        };
        assert_eq!(
            failures,
            vec!["测试阶段判定失败（未给出 issues 明细）".to_string()],
            "空 issues 时合成的占位文案是唯一一条 issue"
        );
        let ctx = TestFailureContext {
            round: 1,
            issues: failures,
        };
        let kick = coding_kickoff(&f.ctx, Some(&ctx));
        assert!(
            kick.contains("测试阶段判定失败（未给出 issues 明细）"),
            "占位文案要原样送达：{kick}"
        );
    }

    #[test]
    fn coding_runs_gates_and_returns_changed_files() {
        let f = fixture(true);
        coding_ready(&f);
        let agent = Agent::new(&[
            "基线绿了\n<stage-complete>1</stage-complete>",
            "改完了\n<stage-complete>1</stage-complete>",
        ]);
        let seen = agent.seen.clone();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(ReviewFactory::new(&[PLAN_PASS])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
        assert!(turn.changed.contains(&"src/a.rs".to_string()), "{:?}", turn.changed);
        assert_eq!(turn.reviews, 1, "coding review 必须真跑一次");
        let seen = seen.borrow();
        assert_eq!(seen.len(), 2, "两轮：基线取证 → 改代码");
        assert!(
            seen[0].contains("不要改任何代码") && seen[0].contains("baseline-test.log"),
            "第一轮必须先要基线证据：{}",
            seen[0]
        );
        assert!(
            seen[0].contains(&f.ctx.repo.display().to_string()),
            "基线轮必须把目标仓告诉 agent（else 会在黑板目录里跑测试）：{}",
            seen[0]
        );
        assert!(seen[1].contains("impl-steps"), "第二轮才是真实 coding：{}", seen[1]);
    }

    #[test]
    fn coding_without_self_check_evidence_fails_before_review() {
        // 先证后报：Agent 发了 stage-complete，但黑板没有合规自证证据 → Quality，且不进审查
        let f = fixture(true);
        coding_ready(&f);
        std::fs::remove_file(f.ctx.blackboard.join("evidence/self-check.log")).unwrap();
        let agent = Agent::new(&[
            "基线绿了\n<stage-complete>1</stage-complete>",
            "改完了\n<stage-complete>1</stage-complete>",
        ]);
        let review = ReviewFactory::new(&[PLAN_PASS]);
        let seen_review = review.seen.clone();
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(review),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_coding(&mut NoUser, 0, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("evidence/self-check.log"), "{msg}");
        assert!(
            seen_review.borrow().is_empty(),
            "没有合规自证证据不得进 coding review"
        );
    }

    #[test]
    fn coding_with_self_check_evidence_enters_review() {
        // 有合规自证证据 → 正常进审查（轮次 1）
        let f = fixture(true);
        coding_ready(&f);
        let agent = Agent::new(&[
            "基线绿了\n<stage-complete>1</stage-complete>",
            "改完了\n<stage-complete>1</stage-complete>",
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(ReviewFactory::new(&[PLAN_PASS])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
        assert_eq!(turn.reviews, 1, "有合规自证证据才进 coding review");
    }

    #[test]
    fn red_baseline_blocks_before_coding_turn() {
        let f = fixture(true);
        std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
        std::fs::write(
            f.ctx.blackboard.join("evidence/baseline-test.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .unwrap();
        // 只给一轮回复：基线轮；若引擎错序，第二轮会被调用则脚本耗尽直接暴露
        let agent = Agent::new(&["跑了，红了\n<stage-complete>1</stage-complete>"]);
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
        let err = session.run_coding(&mut NoUser, 0, None).unwrap_err();
        assert!(err.to_string().contains("基线不是绿的"), "{err}");
        assert_eq!(seen.borrow().len(), 1, "红基线后不得再叫 agent 去改代码");
    }

    #[test]
    fn route_plan_without_marker_is_accepted() {
        // D008：越界申报只发 <route>plan</route> + <impact-gap>，不打 stage-complete；
        // 引擎必须先看 route，不能先要求标记
        let f = fixture(true);
        coding_ready(&f);
        let agent = Agent::new(&[
            "基线绿了\n<stage-complete>1</stage-complete>",
            "计划漏了\n<route>plan</route>\n<impact-gap>\n- 文件：src/extra.rs\n- 为什么必须改：接口在这\n- 证据：src/extra.rs:9\n</impact-gap>",
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(agent),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
        assert!(turn.reply.contains("<route>plan</route>"), "{}", turn.reply);
        assert_eq!(turn.reviews, 0, "申报轮不过 coding review");
    }

    #[test]
    fn coding_review_failure_fails_the_stage() {
        let f = fixture(true);
        coding_ready(&f);
        let fail = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 无法从计划找到依据：src/a.rs\n</issues>";
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[
                "<stage-complete>1</stage-complete>",
                "<stage-complete>1</stage-complete>",
            ])),
            coding_review: Box::new(ReviewFactory::new(&[fail])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_coding(&mut NoUser, 0, None).unwrap_err();
        assert!(err.to_string().contains("变更编码审查未通过"), "{err}");
        assert!(err.to_string().contains("src/a.rs"), "{err}");
    }

    #[test]
    fn testing_review_is_invoked_and_can_fail() {
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
        let fail = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 没有反向验证\n</issues>";
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            test_review: Box::new(ReviewFactory::new(&[fail])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_testing(&mut NoUser).unwrap_err();
        assert!(err.to_string().contains("变更测试审查未通过"), "{err}");
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
            test_review: Box::new(ReviewFactory::new(&[PLAN_PASS])),
            ctx: f.ctx.clone(),
        };
        match session.run_testing(&mut NoUser).unwrap() {
            TestingTurn::Pass { acceptance_done, .. } => {
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

    // ---------------- 审查三通道：违例重审 / 实例失败重试（验收 1-8） ----------------

    /// 按脚本返回结果的审查工厂（能回空回复 / 错误），并数开过几个实例。
    struct ScriptedResultFactory {
        replies: Rc<RefCell<VecDeque<Result<String>>>>,
        opened: Rc<RefCell<usize>>,
    }

    impl ScriptedResultFactory {
        fn new(replies: Vec<Result<String>>) -> (Self, Rc<RefCell<usize>>) {
            let opened = Rc::new(RefCell::new(0));
            (
                Self {
                    replies: Rc::new(RefCell::new(replies.into())),
                    opened: opened.clone(),
                },
                opened,
            )
        }
    }

    struct ScriptedResultReview {
        replies: Rc<RefCell<VecDeque<Result<String>>>>,
        opened: Rc<RefCell<usize>>,
    }

    impl ReviewSession for ScriptedResultReview {
        fn review(&mut self, _instruction: &str) -> Result<String> {
            *self.opened.borrow_mut() += 1;
            self.replies
                .borrow_mut()
                .pop_front()
                .expect("Review 脚本耗尽")
        }
    }

    impl ReviewSessionFactory for ScriptedResultFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            Ok(Box::new(ScriptedResultReview {
                replies: self.replies.clone(),
                opened: self.opened.clone(),
            }))
        }
    }

    fn ok(text: &str) -> Result<String> {
        Ok(text.to_string())
    }

    /// plan 轮：审查第一次协议违例、第二次 PASS → 换新实例重审，plan 轮判 Pass。
    #[test]
    fn plan_review_violation_retries_with_a_new_instance() {
        // 错实现形态：把协议违例当判负（直接 Quality 终止 run）→ 本用例必须变红
        let f = fixture(false);
        let (review, opened) = ScriptedResultFactory::new(vec![
            ok("我忘了输出协议块"),
            ok(PLAN_PASS),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_plan(&mut NoUser, 0).unwrap();
        assert_eq!(turn, PlanTurn::Pass, "违例重审通过后 plan 轮判 Pass");
        assert_eq!(*opened.borrow(), 2, "违例后必须开第二个全新审查实例");
    }

    /// plan 轮：连续 4 次协议违例 → 质量失败（不自动重启），错误文本含「协议违例」。
    #[test]
    fn four_plan_review_violations_are_a_quality_failure() {
        // 错实现形态：违例超限判成基础设施故障（会被自动重启）→ 本用例必须变红
        let f = fixture(false);
        let (review, _opened) = ScriptedResultFactory::new(vec![
            ok("缺块1"),
            ok("缺块2"),
            ok("缺块3"),
            ok("缺块4"),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_plan(&mut NoUser, 0).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("协议违例"), "{msg}");
        assert!(
            err.is_quality_failure(),
            "违例超限是质量失败（不自动重启）: {msg}"
        );
    }

    /// plan 轮：审查第一次空回复 → 换新实例重审（空回复不得被当成判负）。
    #[test]
    fn empty_review_reply_retries_with_a_new_instance() {
        // 错实现形态：把空回复当判负（Quality）或直接判死 run → 本用例必须变红
        let f = fixture(false);
        let (review, opened) =
            ScriptedResultFactory::new(vec![ok(""), ok(PLAN_PASS)]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_plan(&mut NoUser, 0).unwrap();
        assert_eq!(turn, PlanTurn::Pass, "空回复后重审通过 → plan 轮判 Pass");
        assert_eq!(*opened.borrow(), 2, "空回复后必须开第二个全新审查实例");
    }

    /// plan 轮：连续 4 次空回复 → 基础设施故障（System，可重启），错误文本含「基础设施故障」。
    #[test]
    fn four_empty_review_replies_are_an_infrastructure_failure() {
        // 错实现形态：把实例失败判成质量失败（不重启）/ 判负 → 本用例必须变红
        let f = fixture(false);
        let (review, _opened) = ScriptedResultFactory::new(vec![
            ok(""),
            ok(""),
            ok(""),
            ok(""),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let err = session.run_plan(&mut NoUser, 0).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("基础设施故障"), "{msg}");
        assert!(
            !err.is_quality_failure(),
            "实例失败耗尽是基础设施故障（可重启）: {msg}"
        );
    }

    /// plan 轮：审查第一次返回错误（模型通道错误）→ 换新实例重审，不判负也不直接判死。
    #[test]
    fn review_error_retries_with_a_new_instance() {
        // 错实现形态：把通道错误当判负 / 直接判死 run → 本用例必须变红
        let f = fixture(false);
        let (review, opened) = ScriptedResultFactory::new(vec![
            Err(DevforgeError::System("模型通道 500".into())),
            ok(PLAN_PASS),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            plan_review: Box::new(review),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_plan(&mut NoUser, 0).unwrap();
        assert_eq!(turn, PlanTurn::Pass, "通道错误后重审通过 → plan 轮判 Pass");
        assert_eq!(*opened.borrow(), 2, "通道错误后必须开第二个全新审查实例");
    }

    /// coding 轮：审查第一次 FAIL 空箱（协议违例）→ 换新实例重审，第二个 PASS → coding 轮正常推进。
    #[test]
    fn coding_review_violation_retries_with_a_new_instance() {
        // 错实现形态：把 FAIL 空箱当判负（终止 coding 轮 / 回注 coding agent）→ 本用例必须变红
        let f = fixture(true);
        coding_ready(&f);
        let (review, opened) = ScriptedResultFactory::new(vec![
            ok("<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>"),
            ok(PLAN_PASS),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[
                "<stage-complete>1</stage-complete>",
                "<stage-complete>1</stage-complete>",
            ])),
            coding_review: Box::new(review),
            test_agent: Box::new(Agent::new(&[])),
            test_review: Box::new(ReviewFactory::new(&[])),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
        assert_eq!(turn.reviews, 2, "审查轮次要计入重审（实际开过的实例数）");
        assert_eq!(*opened.borrow(), 2, "违例后必须开第二个全新审查实例");
    }

    /// test 轮：审查第一次 FAIL 空箱（协议违例）→ 换新实例重审，第二个 PASS → testing 轮判 Pass。
    #[test]
    fn test_review_violation_retries_with_a_new_instance() {
        // 错实现形态：把 FAIL 空箱当判负（终止 testing 轮 / 回注 coding agent）→ 本用例必须变红
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
        let (review, opened) = ScriptedResultFactory::new(vec![
            ok("<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>"),
            ok(PLAN_PASS),
        ]);
        let mut session = ProdChangeSession {
            plan_agent: Box::new(Agent::new(&[])),
            plan_review: Box::new(ReviewFactory::new(&[])),
            coding_agent: Box::new(Agent::new(&[])),
            coding_review: Box::new(ReviewFactory::new(&[])),
            test_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
            test_review: Box::new(review),
            ctx: f.ctx.clone(),
        };
        let turn = session.run_testing(&mut NoUser).unwrap();
        let TestingTurn::Pass { reviews, .. } = turn else {
            panic!("重审通过后 testing 轮应判 Pass，实际 {turn:?}");
        };
        assert_eq!(reviews, 2, "审查轮次要计入重审（实际开过的实例数）");
        assert_eq!(*opened.borrow(), 2, "违例后必须开第二个全新审查实例");
    }
}

