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
    changed_files, check_baseline_log_fresh, check_self_check_fresh, check_step_commits,
    parse_baseline_log, read_steps, BASELINE_LOG,
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
use crate::sop::review_gate::{outcome_of_review_report, review_with_gate, ReviewBudget};

/// 一次 change run 的固定上下文。
#[derive(Debug, Clone)]
pub struct ChangeCtx {
    /// 目标仓（真代码）。
    pub repo: PathBuf,
    /// 黑板（`plans/<p>/<t>`，impl-steps / evidence 在这）。
    pub blackboard: PathBuf,
    /// 计划目录（台账根 `plantree/<project>/changes/<id>`；**不在目标仓内**，2026-10-25 反转 D003）。
    pub change_dir: PathBuf,
    pub change_id: String,
    pub baseline: String,
    /// 用户需求原文（plan kickoff 要用）。
    pub requirement: String,
    pub doc_level: DocLevel,
    /// 工作项契约（P012 挂账：plan 审查时校验白名单 ⊆ 契约 owned_paths；None = 无契约不校验）。
    pub contract: Option<crate::domain::contract::WorkItemContract>,
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
    /// plan 审查实际开过的实例数（= 报告里的「审查轮次」；违例重审与实例失败重试都计入）。
    pub plan_review_instances: usize,
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
只改计划白名单内的文件；越界必须输出 `<route>plan</route>` + `<impact-gap>`（**声明块放在回复末尾**，其后不要再写别的）。\n\
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
            "{base}\n\n{}\n\n【重入纪律】这是回注重写：impl-steps.md 必须**保留已完成步骤、从最后一号 +1 继续编号**，不得从 1 重写（否则步号对账会判「步骤被提交了不止一次」而失败）。",
            crate::change::testing_gate::coding_failure_block(f)
        ),
        None => base,
    }
}

/// 空箱拒收的追补上限/话术：已下沉到 [`crate::sop::review_gate`]（全仓审计 #7——
/// 同一纪律曾在 change 侧单点落地、MVP testing 侧缺失）。这里保留路径导出。
pub use crate::sop::review_gate::{ISSUES_NUDGE, MAX_EMPTY_ISSUES_NUDGES};

pub fn testing_kickoff(ctx: &ChangeCtx) -> String {
    format!(
        "你是 change-test-agent。change-id：`{}`。\n\
目标仓：{}\n验收标准来自计划：{}\n\
黑板（当前目录）：{}\n\n\
顺序：先跑既有全套回归并把命令与退出码写进 `{}`；再为变更点写测试；\
把对账表写到 `testing/acceptance.md`（每行 `- <验收标准原文> → <测试文件>:<行>`）；\
至少做一次反向验证。\n\
失败时输出 `<route>coding</route>`（计划问题用 `<route>plan</route>`），并用 `<issues>` 块逐条列失败明细\
（每条一行，`- ` 开头）；**路由块与 `<issues>` 块都放在回复末尾**，其后不要再写别的。\n\
通过时输出 `<stage-complete>1</stage-complete>`。",
        ctx.change_id,
        ctx.repo.display(),
        ctx.plan_path().display(),
        ctx.blackboard.display(),
        REGRESSION_LOG
    )
}

// ---------------- 内部小工具 ----------------

/// 证据文件里的退出码（门禁红路径要**按字段**带出退出码；读不到或解析不了就没有该字段）。
///
/// 复用门自己的解析口径（`parse_baseline_log`）→ 解析逻辑只有一份，不在这里另抄一份。
/// 读不到时返回 `None`：tracing 的 `Option` 字段为 `None` 时**整个字段不落**（不是落成 null）。
fn evidence_exit_code(path: &Path) -> Option<i32> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_baseline_log(&text).ok().map(|ev| ev.exit_code)
}

/// 标记追补上限：拒收 + 追补 ×3，与 c6 空箱拒收/重审裁决同口径。
const MAX_MARKER_NUDGES: usize = 3;

/// 标记追补话术：不重复已完成的工作，只补收尾标记。
const MARKER_NUDGE: &str = "你的上一条回复没有输出阶段完成标记。请补完当前阶段收尾，然后在回复末尾单独一行输出 <stage-complete>N（N 为阶段号）；不要重复已完成的工作。";

/// 阶段完成标记验收：无标记先追补（同会话再问一轮），连续 MAX_MARKER_NUDGES 次
/// 仍无标记才 Quality。标记由模型生成，长回合忘交是闪模型的结构性失误（F17：
/// resume6b coding 轮 / resume6c test 轮连续两死），不能一枪毙命。
fn require_marker_nudged(
    stage: &str,
    first_reply: String,
    mut redeliver: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let mut reply = first_reply;
    let mut nudges = 0usize;
    while parse_stage_complete(&reply).is_none() {
        nudges += 1;
        if nudges > MAX_MARKER_NUDGES {
            return Err(DevforgeError::Quality(format!(
                "{stage} 连续 {nudges} 次未输出 <stage-complete>N（已追补 {MAX_MARKER_NUDGES} 次），判定为质量失败（不自动重启）；最近一次回复尾巴：{}",
                crate::sop::coding::tail(&reply)
            )));
        }
        reply = redeliver(MARKER_NUDGE)?;
    }
    Ok(reply)
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
///
/// `role` = 审查角色名（「审查开庭」里程碑的字段；轮次按角色内序号计——共享门返回的
/// 实例数是**返回值**，调用前拿不到）。
fn review_once(
    factory: &mut dyn ReviewSessionFactory,
    role: &str,
    instruction: &str,
) -> Result<(crate::sop::coding_protocol::ReviewReport, usize)> {
    // 「审查开庭」里程碑：在**发起**那一刻落（不是等审完）
    let round = crate::observability::next_review_round(role);
    // 心跳的步骤字段：审查期间卡在哪个角色（死后据此回答「死在哪一步」）
    crate::observability::set_step(&format!("review:{role}"));
    tracing::info!(
        component = "change-engine",
        event = "review.open",
        role,
        round,
        "审查开庭"
    );
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
        let first = self.plan_agent.converse(message, ui)?;
        let reply = require_marker_nudged("change-plan-agent", first, |m| {
            self.plan_agent.converse(m, ui)
        })?;
        // 计划文在台账根（不在目标仓），审查子进程 cwd 是黑板：必须把路径与全文一起给审查官
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
        let audit =
            crate::change::plan_audit::audit_plan_file_with_contract(
                &plan_path,
                &self.ctx.repo,
                self.ctx.contract.as_ref(),
            )
            .map_err(|e| {
                DevforgeError::Quality(format!("计划文件不可读（{}）：{e}", plan_path.display()))
            })?;
        // plan 侧的审查实例数（= 审查轮次）落字段：报告里的轮次如实反映重审（D026 · Q12）。
        let (report, plan_instances) = review_once(
            &mut *self.plan_review,
            "change-plan-review",
            &review_instruction(&self.ctx.change_id, &plan_path, &plan_text, &audit),
        )?;
        self.plan_review_instances = plan_instances;
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
        // 本轮开始时间：基线门/自证门的证据 mtime 早于它 = 上一轮残留，拒收（复验 #6）。
        let turn_started = std::time::SystemTime::now();
        // ① 基线轮：先让 agent 只跑既有测试写证据（此时还没允许改代码）
        let baseline_reply = self.coding_agent.converse(&baseline_kickoff(&self.ctx), ui)?;
        let baseline_reply =
            require_marker_nudged("change-coding-agent（基线轮）", baseline_reply, |m| {
                self.coding_agent.converse(m, ui)
            })?;
        // 引擎不自己跑测试，只验证据：红/缺失就在这里拦住（coding 子进程不会再被叫去改代码）
        // 「基线门」里程碑：绿/红都落一条（红时**按字段**带出退出码）
        let baseline = check_baseline_log_fresh(&self.ctx.blackboard, turn_started);
        match &baseline {
            Ok(ev) => tracing::info!(
                component = "change-engine",
                event = "gate.baseline",
                passed = true,
                exit_code = ev.exit_code,
                "基线门通过"
            ),
            Err(e) => tracing::info!(
                component = "change-engine",
                event = "gate.baseline",
                passed = false,
                // 红时也要把退出码**按字段**带出来（计划 §5「红时含退出码」）：
                // 事后按字段过滤要看得见「退出码多少」，不能只埋在错误文本里。
                exit_code = evidence_exit_code(&self.ctx.blackboard.join(BASELINE_LOG)),
                error = %e,
                "基线门未通过"
            ),
        }
        baseline?;

        // ② coding 轮：真实改代码；越界申报按 D008 只发 <route>plan</route> + <impact-gap>，不打标记
        let mut reply = self
            .coding_agent
            .converse(&coding_kickoff(&self.ctx, last_failure), ui)?;
        if parse_route(&reply).as_deref() == Some("plan") {
            // 复验 #1（高危）：git 失败必须 Err 上抛——unwrap_or_default 会把基线失效/仓库损坏
            // 伪装成「无改动」，boundary 因 extra 为空直接放行，门禁被静默绕过。
            let changed = changed_files(&self.ctx.repo, &self.ctx.baseline)?;
            return Ok(CodingTurn {
                reply,
                changed,
                reviews: 0,
                review_issues: Vec::new(),
            });
        }
        reply = require_marker_nudged("change-coding-agent", reply, |m| {
            self.coding_agent.converse(m, ui)
        })?;
        // 先证后报：自证证据缺失或红 → Quality（不进审查）
        // 「自证门」里程碑：绿/红都落一条
        let self_check = check_self_check_fresh(&self.ctx.blackboard, turn_started);
        match &self_check {
            Ok(ev) => tracing::info!(
                component = "change-engine",
                event = "gate.self_check",
                passed = true,
                exit_code = ev.exit_code,
                "自证门通过"
            ),
            Err(e) => tracing::info!(
                component = "change-engine",
                event = "gate.self_check",
                passed = false,
                error = %e,
                "自证门未通过"
            ),
        }
        self_check?;
        // 实施步骤与按步提交必须对得上
        let steps = read_steps(&self.ctx.blackboard)?;
        // 「步提交对账」里程碑：含步骤数与判定结论
        let step_commits = check_step_commits(
            &self.ctx.repo,
            &self.ctx.baseline,
            &self.ctx.change_id,
            &steps,
        );
        match &step_commits {
            Ok(n) => tracing::info!(
                component = "change-engine",
                event = "gate.step_commits",
                passed = true,
                steps = steps.len(),
                commits = *n,
                "步提交对账通过"
            ),
            Err(e) => tracing::info!(
                component = "change-engine",
                event = "gate.step_commits",
                passed = false,
                steps = steps.len(),
                error = %e,
                "步提交对账未通过"
            ),
        }
        step_commits?;
        let changed = changed_files(&self.ctx.repo, &self.ctx.baseline)?;

        // ③ coding review：diff ↔ 计划对账（越界已由引擎先算，这里管语义）
        let (report, instances) = review_once(
            &mut *self.coding_review,
            "change-coding-review",
            &coding_review_instruction(&self.ctx, &changed),
        )?;
        match report.verdict {
            StageVerdict::Pass => Ok(CodingTurn {
                reply,
                changed,
                reviews: instances,
                review_issues: Vec::new(),
            }),
            // change #4：审查 FAIL 不再一刀切判 Quality——把明细随 turn 交回编排层，
            // 由它走有界回注循环（MAX_CODING_FIXES）。
            StageVerdict::Fail => Ok(CodingTurn {
                reply,
                changed,
                reviews: instances,
                review_issues: report.issues,
            }),
        }
    }

    fn run_testing(&mut self, ui: &mut dyn UserLink) -> Result<TestingTurn> {
        // 引擎审计修复 #6：入口先拍产品代码快照——出口对比，「不改产品代码」从
        // prompt 约束升级为机械门。
        let tracked_before =
            crate::change::testing_gate::snapshot_tracked_changes(&self.ctx.repo, &self.ctx.baseline)?;
        let turn_started = std::time::SystemTime::now();
        // 路由判定 + 空箱拒收：带路由但不带 `<issues>` 的回注不收（裁决 B），追一条补齐话术后重发
        let mut message = testing_kickoff(&self.ctx);
        let mut nudges = 0usize;
        let (reply, route, failures) = loop {
            let reply = self.test_agent.converse(&message, ui)?;
            let route = parse_route(&reply);
            match route.as_deref() {
                Some("coding") | Some("plan") => {
                    let failures = issues_of(&reply);
                    if !failures.is_empty() {
                        break (reply, route, failures);
                    }
                    // 空箱拒收：没有失败明细的回注不产生 RouteCoding/RoutePlan，也不合成占位文案
                    nudges += 1;
                    if nudges > MAX_EMPTY_ISSUES_NUDGES {
                        return Err(DevforgeError::Quality(format!(
                            "测试阶段连续 {nudges} 次回注路由但未给出 issues 明细（未给出 issues 明细），判定为质量失败（不自动重启）"
                        )));
                    }
                    message = ISSUES_NUDGE.to_string();
                }
                _ => break (reply, None, Vec::new()),
            }
        };
        if let Some(route) = route {
            return Ok(match route.as_str() {
                "coding" => TestingTurn::RouteCoding { failures },
                _ => TestingTurn::RoutePlan { failures },
            });
        }
        let reply = require_marker_nudged("change-test-agent", reply, |m| {
            self.test_agent.converse(m, ui)
        })?;
        // 声称通过 → 回归日志必须是绿的
        check_regression_log(&self.ctx.blackboard, true, turn_started)?;
        // 引擎审计修复 #6：出口对比——测试期间新增的产品代码改动（tests/ 与流水线
        // 产物之外）= Quality（机械门，不再依赖审查官人眼）。
        let tracked_after =
            crate::change::testing_gate::snapshot_tracked_changes(&self.ctx.repo, &self.ctx.baseline)?;
        crate::change::testing_gate::check_no_product_code_edits(&tracked_before, &tracked_after)?;
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
            "change-test-review",
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

    /// 编码审查打回后的回注（change #4）：只补审查指出的问题，不重跑基线、不重做已完成工作。
    fn fix_coding(&mut self, ui: &mut dyn UserLink, issues: &[String], round: usize) -> Result<()> {
        let message = format!(
            "编码审查第 {round} 轮打回，逐条修复后重交（不要重复已完成的工作，不要扩围）：\n{}\n\n\
             只改计划白名单内的文件；修完把本轮自证证据（命令 + 退出码）更新到黑板 `{}`，\
             然后输出 <stage-complete>1</stage-complete>。",
            issues
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n"),
            crate::change::coding_gate::SELF_CHECK_LOG
        );
        let first = self.coding_agent.converse(&message, ui)?;
        require_marker_nudged("change-coding-agent（回注轮）", first, |m| {
            self.coding_agent.converse(m, ui)
        })?;
        Ok(())
    }

    fn plan_reviews(&self) -> usize {
        self.plan_review_instances
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
mod session_tests;
