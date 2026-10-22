//! 审查输出协议硬化 —— 验收对账测试
//! change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r（计划 §5 验收标准 1–32）
//!
//! 黑盒口径：只经**公开 API** 观察行为——`ProdChangeSession` / `ChangeSession` /
//! `parse_verdict` / `parse_review_report` / `review_with_gate` / `CodingSopLoop` /
//! `PlanningLoop` / `classify` / 角色提示词常量 / 仓内文档与常量——不窥探内部实现；
//! 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//!
//! 与仓内 `src/**` 的 `#[cfg(test)]` 单测相互独立：那一组在实现内部就地断言，
//! 本组从**仓外**（`tests/` 集成测试）看公开面，专门证伪「三通道分层 / 空箱拒收 /
//! 尾部协议位」这三件事是否真的落到对外可观测的行为上。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use devforge::change::boundary::{BoundaryVerdict, classify};
use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession};
use devforge::change::state::CHANGE_SOP;
use devforge::config::manifest::{
    AgentManifest, CHANGE_CODING_AGENT_PROMPT, CHANGE_TEST_AGENT_PROMPT, ConfigManifest,
};
use devforge::error::{DevforgeError, Result};
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::coding::CodingSopLoop;
use devforge::sop::coding_protocol::parse_review_report;
use devforge::sop::output::{Verdict, parse_verdict};
use devforge::sop::planning::{
    PlanAgentSession, PlanningLoop, ReviewSession, ReviewSessionFactory, UserLink,
};
use devforge::sop::review_gate::{
    MAX_PROTOCOL_VIOLATIONS, MAX_REVIEW_INSTANCE_FAILURES, ReviewBudget, ReviewOutcome,
    review_with_gate,
};

// ---------------- 常量 ----------------

/// 本变更的基线提交（计划 §1.1）。
/// 口径：本场开工时的 HEAD（计划写的 `a0be947…` 之后操作员又落了两个提交，
/// 其中 `a36f6b7` 动了非 tests/ 文件 → 守卫扫整段 `BASELINE..HEAD` 时必红；
/// 台账 F10 的既定处置 =「常量刷新」到本变更提交集合的起点）。
const BASELINE: &str = "a36f6b79dec2e681db8daed0da067f9abd47a9f5";

/// 计划 §4 的白名单（**23 个**文件 = coding 步 22 个 + 测试阶段 1 个）。
const PLAN_WHITELIST: &[&str] = &[
    "src/observability/mod.rs",
    "src/observability/heartbeat.rs",
    "src/observability/tombstone.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/engine/retry.rs",
    "src/engine/client.rs",
    "src/server/mod.rs",
    "src/server/handlers.rs",
    "src/server/tests.rs",
    "src/change/orchestrator.rs",
    "src/change/session.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/sop/doc_from_zero.rs",
    "src/sop/doc_fix.rs",
    "tests/f6_observability.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context.rs",
    "tests/change_review_protocol.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

/// 计划 §4 白名单去掉唯一的测试阶段文件 = **coding 步 22 个**。
const CODING_WHITELIST: &[&str] = &[
    "src/observability/mod.rs",
    "src/observability/heartbeat.rs",
    "src/observability/tombstone.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/engine/retry.rs",
    "src/engine/client.rs",
    "src/server/mod.rs",
    "src/server/handlers.rs",
    "src/server/tests.rs",
    "src/change/orchestrator.rs",
    "src/change/session.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/sop/doc_from_zero.rs",
    "src/sop/doc_fix.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context.rs",
    "tests/change_review_protocol.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

/// 引擎**不再**合成的占位文案（裁决 B：空箱拒收 + 追补）。
const PLACEHOLDER_ISSUE: &str = "测试阶段判定失败（未给出 issues 明细）";
/// 空箱拒收的追补话术锚点。
const NUDGE_ANCHOR: &str = "请补齐失败明细后重发";

/// 审查协议块：PASS。
const PASS: &str = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
/// 审查协议块：PASS + 收工（MVP coding 循环用）。
const PASS_FINISH: &str = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
/// 审查协议块：FAIL + fix，**不带** `<issues>`（= 协议违例，判负通道拿不到它）。
const FAIL_EMPTY: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>";
/// 审查协议块：**缺** `<stage-verdict>`（= 协议违例）。
const NO_VERDICT: &str = "我看了，没发现问题。（没有输出协议块）";
/// 审查协议块：FAIL + 非空 issues（= 真判负）。
const FAIL_WITH_ISSUES: &str =
    "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- tests/change_review_protocol.rs:1 断言失败\n</issues>";

// ---------------- 夹具 ----------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 审查实例的一条脚本回复。
enum ScriptedReply {
    /// 正常文本。
    Text(String),
    /// 模型通道错误（审查调用返回 `Err`）。
    Fail(String),
}

impl ScriptedReply {
    fn text(s: &str) -> Self {
        ScriptedReply::Text(s.to_string())
    }
}

/// 脚本化的 agent 会话：按队列吐出回复，并记录收到的每条消息。
#[derive(Clone)]
struct ScriptedAgent {
    replies: Rc<RefCell<VecDeque<String>>>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ScriptedAgent {
    fn new(replies: &[&str]) -> Self {
        ScriptedAgent {
            replies: Rc::new(RefCell::new(
                replies.iter().map(|s| s.to_string()).collect(),
            )),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// 收到的消息（观测点：违例**不得**回注被审 agent）。
    fn seen(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

impl PlanAgentSession for ScriptedAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.seen.borrow_mut().push(message.to_string());
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("Agent 脚本耗尽"))
    }
}

/// 脚本化的审查实例：每 `open()` 一次 = 一个**全新实例**。
struct ScriptedReview {
    reply: Option<ScriptedReply>,
}

impl ReviewSession for ScriptedReview {
    fn review(&mut self, instruction: &str) -> Result<String> {
        let _ = instruction;
        match self.reply.take().expect("审查实例被用了两次") {
            ScriptedReply::Text(t) => Ok(t),
            ScriptedReply::Fail(m) => Err(DevforgeError::agent("change-review", m)),
        }
    }
}

/// 审查实例工厂：记录开过几个实例（= 审查轮次）。
#[derive(Clone)]
struct ScriptedReviewFactory {
    replies: Rc<RefCell<VecDeque<ScriptedReply>>>,
    opened: Rc<Cell<usize>>,
}

impl ScriptedReviewFactory {
    fn new(replies: Vec<ScriptedReply>) -> Self {
        ScriptedReviewFactory {
            replies: Rc::new(RefCell::new(replies.into())),
            opened: Rc::new(Cell::new(0)),
        }
    }

    /// 开过的实例数。
    fn opened(&self) -> usize {
        self.opened.get()
    }
}

impl ReviewSessionFactory for ScriptedReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        self.opened.set(self.opened.get() + 1);
        let reply = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("Review 脚本耗尽");
        Ok(Box::new(ScriptedReview {
            reply: Some(reply),
        }))
    }
}

/// 计划文本（六块齐全 + 一条验收标准，与计划 §5 的对账口径一致）。
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

/// 一次性夹具：目标仓（带 git 历史）+ 黑板（证据 / 步骤 / 对账表）+ 计划文件。
struct Fixture {
    /// 持有 TempDir（落盘生命周期由它决定；字段本身不读）。
    _tmp: tempfile::TempDir,
    ctx: ChangeCtx,
    /// 目标仓的基线提交。
    baseline: String,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();

    // git：基线提交 + 一步一提交（编码门要核对「步骤 ↔ 提交」）
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "baseline",
        ],
    );
    let baseline = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(repo.join("src/a.rs"), "// a changed\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "change: change-1 step 1 —— 改 a",
        ],
    );

    // 黑板：基线证据 / 自证证据 / 回归证据 / 实施步骤 / 验收对账表
    let blackboard = tmp.path().join("bb");
    std::fs::create_dir_all(blackboard.join("evidence")).unwrap();
    std::fs::create_dir_all(blackboard.join("testing")).unwrap();
    std::fs::write(
        blackboard.join("evidence/baseline-test.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        blackboard.join("evidence/self-check.log"),
        "command: cargo test --lib\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        blackboard.join("evidence/regression.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(blackboard.join("impl-steps.md"), "## 步骤 1 · 改 a\n").unwrap();
    std::fs::write(
        blackboard.join("testing/acceptance.md"),
        "- 给定 x=1 → 返回 2 → tests/change_review_protocol.rs:1\n",
    )
    .unwrap();

    // 计划文件（放在目标仓的 changes 目录里，审查轮要读它）
    let change_dir = repo.join("docs/plantree/changes/change-1");
    std::fs::create_dir_all(&change_dir).unwrap();
    std::fs::write(change_dir.join("change-plan.md"), plan_text()).unwrap();

    Fixture {
        _tmp: tmp,
        ctx: ChangeCtx {
            repo,
            blackboard,
            change_dir,
            change_id: "change-1".into(),
            baseline: baseline.clone(),
            requirement: "把 a 改成 b".into(),
            doc_level: DocLevel::None,
        },
        baseline,
    }
}

/// 造一个 `ProdChangeSession`（六个会话槽 + 上下文）。
#[allow(clippy::too_many_arguments)]
fn prod_session(
    ctx: ChangeCtx,
    plan_agent: ScriptedAgent,
    plan_review: ScriptedReviewFactory,
    coding_agent: ScriptedAgent,
    coding_review: ScriptedReviewFactory,
    test_agent: ScriptedAgent,
    test_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    ProdChangeSession {
        plan_agent: Box::new(plan_agent),
        plan_review: Box::new(plan_review),
        coding_agent: Box::new(coding_agent),
        coding_review: Box::new(coding_review),
        test_agent: Box::new(test_agent),
        test_review: Box::new(test_review),
        ctx,
        plan_review_instances: 0,
    }
}

/// 只给 plan 轮用的会话（其余槽给空脚本）。
fn plan_only(
    ctx: ChangeCtx,
    plan_agent: ScriptedAgent,
    plan_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        plan_agent,
        plan_review,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
    )
}

/// 只给 coding 轮用的会话。
fn coding_only(
    ctx: ChangeCtx,
    coding_agent: ScriptedAgent,
    coding_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        coding_agent,
        coding_review,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
    )
}

/// 只给 testing 轮用的会话。
fn testing_only(
    ctx: ChangeCtx,
    test_agent: ScriptedAgent,
    test_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        test_agent,
        test_review,
    )
}

// ---------------- 验收 1：plan 审查违例 → 换新实例重审 → Pass ----------------

/// 验收 1 · plan 审查第一次缺 `<stage-verdict>`、第二次 PASS → 开过两个实例、plan 轮判 Pass。
#[test]
fn a1_plan_review_violation_retries_with_a_new_instance_and_passes() {
    // 错实现形态：把「审查输出缺协议块」当成判负直接终止 run（不换实例重审）→ 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(PASS),
    ]);
    let opened = plan_review.clone();
    let mut session = plan_only(f.ctx.clone(), plan_agent.clone(), plan_review);

    let turn = session.run_plan(&mut NoUser, 0).expect("违例不得终止 run");
    assert_eq!(turn, PlanTurn::Pass, "第二次 PASS → plan 轮判 Pass");
    assert_eq!(opened.opened(), 2, "违例后必须开**全新**审查实例重审");
    assert_eq!(
        plan_agent.seen().len(),
        1,
        "违例原因不得回注被审 agent（plan agent 只该收到开工语）"
    );
}

// ---------------- 验收 2：coding 审查 FAIL 空箱 → 换新实例重审 → 正常推进 ----------------

/// 验收 2 · coding 审查第一次 FAIL + fix 但不带 `<issues>` → 不终止、不回注，换实例重审后推进。
#[test]
fn a2_coding_review_fail_without_issues_is_a_violation_not_a_verdict() {
    // 错实现形态：把「FAIL 空箱」当判负（直接终止「变更编码审查未通过」）→ 本用例必须变红
    let f = fixture();
    let coding_agent = ScriptedAgent::new(&[
        "基线取证：cargo test\nexit_code: 0\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let coding_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(PASS),
    ]);
    let opened = coding_review.clone();
    let mut session = coding_only(f.ctx.clone(), coding_agent.clone(), coding_review);

    let turn = session
        .run_coding(&mut NoUser, 0, None)
        .expect("违例不得终止 coding 轮");
    assert_eq!(turn.reviews, 2, "审查轮次 = 实际开过的实例数（1 违例 + 1 判负）");
    assert_eq!(opened.opened(), 2, "违例后必须开**全新**审查实例重审");
    assert_eq!(
        coding_agent.seen().len(),
        2,
        "违例原因不得回注 coding agent（只该收到基线开工语 + coding 开工语）"
    );
    assert!(
        !coding_agent
            .seen()
            .iter()
            .any(|m| m.contains("变更编码审查未通过")),
        "违例不得以「审查未通过」的面貌回注 coding agent"
    );
}

// ---------------- 验收 3：test 审查 FAIL 空箱 → 换新实例重审 → Pass ----------------

/// 验收 3 · test 审查第一次 FAIL 空箱、第二次 PASS → 开两个实例、testing 轮判 Pass。
#[test]
fn a3_test_review_fail_without_issues_is_a_violation_not_a_verdict() {
    // 错实现形态：把 test 审查的 FAIL 空箱当判负（终止「变更测试审查未通过」）→ 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&["测完了\n<stage-complete>1</stage-complete>"]);
    let test_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(PASS),
    ]);
    let opened = test_review.clone();
    let mut session = testing_only(f.ctx.clone(), test_agent, test_review);

    let turn = session.run_testing(&mut NoUser).expect("违例不得终止 testing 轮");
    match turn {
        TestingTurn::Pass { reviews, .. } => {
            assert_eq!(reviews, 2, "审查轮次 = 实际开过的实例数（1 违例 + 1 判负）");
        }
        other => panic!("应为 Pass，实际 {other:?}"),
    }
    assert_eq!(opened.opened(), 2, "违例后必须开**全新**审查实例重审");
}

// ---------------- 验收 4：连续 4 次违例 → 质量失败 ----------------

/// 验收 4 · 审查连续 4 次协议违例 → 质量失败（不自动重启），错误文本含「协议违例」与轮次计数。
#[test]
fn a4_four_consecutive_violations_are_a_quality_failure() {
    // 错实现形态：把违例上限判成 System（会被自动重启）/ 或第一次违例就终止 → 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
    ]);
    let opened = plan_review.clone();
    let mut session = plan_only(f.ctx.clone(), plan_agent.clone(), plan_review);

    let err = session.run_plan(&mut NoUser, 0).unwrap_err();
    let msg = err.to_string();
    assert!(err.is_quality_failure(), "违例超限必须是质量失败：{msg}");
    assert!(!err.is_agent_failure(), "违例超限不是 agent 失败：{msg}");
    assert!(msg.contains("协议违例"), "错误文本要含「协议违例」：{msg}");
    assert!(msg.contains("4"), "错误文本要含连续轮次计数：{msg}");
    assert_eq!(opened.opened(), 4, "违例上限 = 3 次重审 + 第 4 次判死");
    assert_eq!(
        plan_agent.seen().len(),
        1,
        "违例原因不得回注被审 agent"
    );
}

// ---------------- 验收 5：空回复 → 换新实例重审 ----------------

/// 验收 5 · 审查实例第一次返回空回复 → 开第二个全新实例重审（空回复不得被当成判负）。
#[test]
fn a5_empty_review_reply_is_an_instance_failure_not_a_verdict() {
    // 错实现形态：把空回复当判负（直接终止 run）→ 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(""),
        ScriptedReply::text(PASS),
    ]);
    let opened = plan_review.clone();
    let mut session = plan_only(f.ctx.clone(), plan_agent, plan_review);

    let turn = session.run_plan(&mut NoUser, 0).expect("空回复不得判负");
    assert_eq!(turn, PlanTurn::Pass);
    assert_eq!(opened.opened(), 2, "空回复后必须开**全新**审查实例重审");
}

// ---------------- 验收 6：连续 4 次空回复 → 基础设施故障 ----------------

/// 验收 6 · 审查实例连续 4 次空回复 → 基础设施故障（System，可自动重启），错误文本含计数。
#[test]
fn a6_four_consecutive_empty_replies_are_an_infrastructure_failure() {
    // 错实现形态：把实例失败耗尽判成质量失败（不自动重启）/ 或第一次空回复就判死 → 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
    ]);
    let opened = plan_review.clone();
    let mut session = plan_only(f.ctx.clone(), plan_agent, plan_review);

    let err = session.run_plan(&mut NoUser, 0).unwrap_err();
    let msg = err.to_string();
    assert!(
        !err.is_quality_failure(),
        "实例失败耗尽是基础设施故障（可重启），不是质量失败：{msg}"
    );
    assert!(msg.contains("基础设施故障"), "错误文本要含「基础设施故障」：{msg}");
    assert!(msg.contains("4"), "错误文本要含连续轮次计数：{msg}");
    assert_eq!(opened.opened(), 4, "实例失败上限 = 3 次重试 + 第 4 次判死");
}

// ---------------- 验收 7：模型通道错误 → 换新实例重审 ----------------

/// 验收 7 · 审查实例第一次返回模型通道错误（`Err`）→ 换新实例重审，不判负、不判死 run。
#[test]
fn a7_channel_error_is_retried_with_a_new_instance() {
    // 错实现形态：把模型通道错误直接上抛判死 run（不换实例）→ 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::Fail("模型通道 500".into()),
        ScriptedReply::text(PASS),
    ]);
    let opened = plan_review.clone();
    let mut session = plan_only(f.ctx.clone(), plan_agent, plan_review);

    let turn = session.run_plan(&mut NoUser, 0).expect("通道错误不得判死 run");
    assert_eq!(turn, PlanTurn::Pass);
    assert_eq!(opened.opened(), 2, "通道错误后必须开**全新**审查实例重审");
}

// ---------------- 验收 8：test 审查真判负 → 仍然终止 ----------------

/// 验收 8 · test 审查 FAIL + 非空 issues（真判负）→ 仍判「变更测试审查未通过」并 Quality 终止。
#[test]
fn a8_test_review_fail_with_issues_still_fails_the_stage() {
    // 错实现形态：把真判负也当成「换实例重审」（判负通道被改软）→ 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&["测完了\n<stage-complete>1</stage-complete>"]);
    let test_review = ScriptedReviewFactory::new(vec![ScriptedReply::text(FAIL_WITH_ISSUES)]);
    let opened = test_review.clone();
    let mut session = testing_only(f.ctx.clone(), test_agent, test_review);

    let err = session.run_testing(&mut NoUser).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("变更测试审查未通过"), "判负文案要保持：{msg}");
    assert!(
        msg.contains("tests/change_review_protocol.rs:1 断言失败"),
        "错误文本要逐字含 issues 原文：{msg}"
    );
    assert!(err.is_quality_failure(), "判负 = 质量失败：{msg}");
    assert_eq!(opened.opened(), 1, "真判负不重审（判负通道不变）");
}

// ---------------- 验收 9：MVP coding 审查 FAIL 空箱 → 换新实例，不回注 ----------------

/// 验收 9 · MVP coding 审查 FAIL 空箱 → 开第二个全新实例重审，不回注 coding agent，issues_total 不累计。
#[test]
fn a9_mvp_coding_fail_without_issues_is_not_injected_into_the_agent() {
    // 错实现形态：把 FAIL 空箱当判负回注 coding agent（issues_total 累计、agent 收到回注）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let agent = ScriptedAgent::new(&["<stage-complete>1</stage-complete>"]);
    let review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(PASS_FINISH),
    ]);
    let opened = review.clone();
    let seen = agent.clone();
    let mut loop_ = CodingSopLoop::new(agent, review);

    let report = loop_
        .run(&mut NoUser, tmp.path(), "proj", "task")
        .expect("违例不得判死");
    assert_eq!(report.issues_total, 0, "违例不累计 issues_total");
    assert_eq!(report.review_rounds, 2, "审查轮次 = 实际开过的实例数");
    assert_eq!(opened.opened(), 2, "违例后必须开**全新**实例重审");
    assert_eq!(seen.seen().len(), 1, "违例不得回注 coding agent");
    assert!(
        !seen.seen().iter().any(|m| m.contains("审查未通过")),
        "违例不得以「审查未通过」的面貌回注 coding agent"
    );
}

// ---------------- 验收 10：MVP coding 连续 4 次 FAIL 空箱 → 质量失败 ----------------

/// 验收 10 · MVP coding 审查连续 4 次 FAIL 空箱 → 质量失败（不自动重启），错误文本含「协议违例」。
#[test]
fn a10_mvp_coding_repeated_fail_without_issues_is_a_quality_failure() {
    // 错实现形态：违例上限判成 System（会被自动重启）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let agent = ScriptedAgent::new(&["<stage-complete>1</stage-complete>"]);
    let review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(FAIL_EMPTY),
        ScriptedReply::text(FAIL_EMPTY),
    ]);
    let mut loop_ = CodingSopLoop::new(agent, review);

    let err = loop_
        .run(&mut NoUser, tmp.path(), "proj", "task")
        .unwrap_err();
    let msg = err.to_string();
    assert!(err.is_quality_failure(), "违例超限必须是质量失败：{msg}");
    assert!(msg.contains("协议违例"), "错误文本要含「协议违例」：{msg}");
    assert!(msg.contains("4"), "错误文本要含连续轮次计数：{msg}");
}

// ---------------- 验收 11：parse_verdict 的 FAIL 空箱 → 违例 ----------------

/// 验收 11 · `parse_verdict` 收到 `<verdict>FAIL</verdict>` 且无 `<issues>` → 判协议违例。
#[test]
fn a11_parse_verdict_fail_without_issues_is_a_protocol_violation() {
    // 错实现形态：产出 `Verdict::Fail { issues: [] }` 空清单（用空清单兼表违例）→ 本用例必须变红
    let v = parse_verdict("<verdict>FAIL</verdict>").expect("不再硬错误");
    assert!(
        matches!(v, Verdict::ProtocolViolation { .. }),
        "FAIL 空箱必须是协议违例，不是空清单的判负：{v:?}"
    );
    assert_ne!(v, Verdict::Fail { issues: vec![] });
}

// ---------------- 验收 12：MVP plan-review 缺 verdict → 换新实例，不回注 ----------------

/// 验收 12 · MVP plan-review 缺 `<verdict>` 块 → 判违例、换第二个全新实例重审，不回注规划 agent。
#[test]
fn a12_mvp_plan_review_missing_verdict_retries_without_injecting() {
    // 错实现形态：把缺 verdict 当 issue 回注规划 agent（issues_total 累计、agent 收到回注）→ 本用例必须变红
    let plan = ScriptedAgent::new(&["<plan-complete>"]);
    let review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text("没有任何块"),
        ScriptedReply::text("<verdict>PASS</verdict>"),
    ]);
    let opened = review.clone();
    let seen = plan.clone();
    let mut loop_ = PlanningLoop::new(plan, review);

    let report = loop_.run(&mut NoUser).expect("违例不得判死");
    assert_eq!(report.issues_total, 0, "违例不累计 issues_total");
    assert_eq!(report.rounds, 2, "审查轮次 = 实际开过的实例数");
    assert_eq!(opened.opened(), 2, "违例后必须开**全新**实例重审");
    assert_eq!(seen.seen().len(), 1, "违例不得回注规划 agent");
    assert!(
        !seen.seen().iter().any(|m| m.contains("Review 发现")),
        "违例不得以「Review 发现…」的面貌回注规划 agent"
    );
}

// ---------------- 验收 13：MVP plan-review 连续 4 次违例 → 质量失败 ----------------

/// 验收 13 · MVP plan-review 连续 4 次违例 → 质量失败（不自动重启），错误文本含「协议违例」。
#[test]
fn a13_mvp_plan_review_repeated_violations_are_a_quality_failure() {
    // 错实现形态：违例上限判成 System（会被自动重启）→ 本用例必须变红
    let plan = ScriptedAgent::new(&["<plan-complete>"]);
    let review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text("没有任何块"),
        ScriptedReply::text("没有任何块"),
        ScriptedReply::text("没有任何块"),
        ScriptedReply::text("没有任何块"),
    ]);
    let mut loop_ = PlanningLoop::new(plan, review);

    let err = loop_.run(&mut NoUser).unwrap_err();
    let msg = err.to_string();
    assert!(err.is_quality_failure(), "违例超限必须是质量失败：{msg}");
    assert!(msg.contains("协议违例"), "错误文本要含「协议违例」：{msg}");
    assert!(msg.contains("4"), "错误文本要含连续轮次计数：{msg}");
}

// ---------------- 验收 14：MVP plan-review 真判负 → 照旧回注 ----------------

/// 验收 14 · MVP plan-review FAIL + 非空 issues → 仍按现状回注规划 agent（逐条原文，清单非空）。
#[test]
fn a14_mvp_plan_review_fail_with_issues_is_still_injected() {
    // 错实现形态：把真判负也改成「只换实例不回注」（判负通道被改软）→ 本用例必须变红
    let plan = ScriptedAgent::new(&["<plan-complete>", "改好了"]);
    let review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text("<verdict>FAIL</verdict>\n<issues>\n- 规划说 Rust 又说 Python\n</issues>"),
        ScriptedReply::text("<verdict>PASS</verdict>"),
    ]);
    let seen = plan.clone();
    let mut loop_ = PlanningLoop::new(plan, review);

    let report = loop_.run(&mut NoUser).expect("判负后应重查并通过");
    assert_eq!(report.issues_total, 1, "真判负要累计 issues_total");
    let messages = seen.seen();
    assert_eq!(messages.len(), 2, "真判负要回注规划 agent 一次");
    assert!(
        messages[1].contains("规划说 Rust 又说 Python"),
        "回注要逐条带 issues 原文：{}",
        messages[1]
    );
}

// ---------------- 验收 15：空箱回注 → 拒收 + 追补 ----------------

/// 验收 15 · test agent 尾部 `<route>coding</route>` 不带 `<issues>` → 拒收、追补，不合成占位文案。
#[test]
fn a15_empty_issues_route_is_rejected_and_nudged() {
    // 错实现形态：把空箱回注收下（RouteCoding + 引擎合成的占位文案）→ 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&[
        "<route>coding</route>",
        "<route>coding</route>",
        "<route>coding</route>",
        "<route>coding</route>\n<issues>\n- 明细一\n- 明细二\n</issues>",
    ]);
    let mut session = testing_only(
        f.ctx.clone(),
        test_agent.clone(),
        ScriptedReviewFactory::new(Vec::new()),
    );

    let turn = session.run_testing(&mut NoUser).expect("第 4 次带明细应被接受");
    match turn {
        TestingTurn::RouteCoding { failures } => {
            assert_eq!(
                failures,
                vec!["明细一".to_string(), "明细二".to_string()],
                "接受的必须是 agent 自己的明细原文"
            );
        }
        other => panic!("应为 RouteCoding，实际 {other:?}"),
    }
    let messages = test_agent.seen();
    assert_eq!(messages.len(), 4, "前三次空箱各追补一次");
    for (i, m) in messages.iter().enumerate().skip(1) {
        assert!(
            m.contains(NUDGE_ANCHOR),
            "第 {i} 次追补要含「{NUDGE_ANCHOR}」：{m}"
        );
    }
    for m in &messages {
        assert!(
            !m.contains(PLACEHOLDER_ISSUE),
            "追补话术里不得出现占位文案：{m}"
        );
    }
}

// ---------------- 验收 16：连续 4 次空箱 → 质量失败 ----------------

/// 验收 16 · test agent 连续 4 次回 `<route>coding</route>` 都不带 `<issues>` → 第 4 次判质量失败。
#[test]
fn a16_four_consecutive_empty_routes_are_a_quality_failure() {
    // 错实现形态：第 4 次仍不带明细时收下（RouteCoding + 占位文案）→ 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&[
        "<route>coding</route>",
        "<route>coding</route>",
        "<route>coding</route>",
        "<route>coding</route>",
    ]);
    let mut session = testing_only(
        f.ctx.clone(),
        test_agent.clone(),
        ScriptedReviewFactory::new(Vec::new()),
    );

    let err = session.run_testing(&mut NoUser).unwrap_err();
    let msg = err.to_string();
    assert!(err.is_quality_failure(), "空箱连续超限 = 质量失败：{msg}");
    assert!(msg.contains("未给出 issues 明细"), "错误文本要点名空箱：{msg}");
    assert!(msg.contains("4"), "错误文本要含连续计数：{msg}");
    for m in test_agent.seen() {
        assert!(
            !m.contains(PLACEHOLDER_ISSUE),
            "全程不得出现占位文案：{m}"
        );
    }
}

// ---------------- 验收 17：空箱后补齐 → 明细逐字送达 ----------------

/// 验收 17 · 第一次空箱、第二次带 `<issues>`（两条明细）→ 接受该回注，failures 逐字等于两条明细。
#[test]
fn a17_empty_then_issues_route_carries_the_details_verbatim() {
    // 错实现形态：把引擎合成的占位文案当失败明细送出去 → 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&[
        "<route>coding</route>",
        "<route>coding</route>\n<issues>\n- 明细一\n- 明细二\n</issues>",
    ]);
    let mut session = testing_only(
        f.ctx.clone(),
        test_agent,
        ScriptedReviewFactory::new(Vec::new()),
    );

    let turn = session.run_testing(&mut NoUser).expect("带明细的回注应被接受");
    match turn {
        TestingTurn::RouteCoding { failures } => {
            assert_eq!(
                failures,
                vec!["明细一".to_string(), "明细二".to_string()],
                "failures 必须逐字等于 agent 的两条明细"
            );
            assert!(
                !failures.iter().any(|x| x.contains(PLACEHOLDER_ISSUE)),
                "不得出现占位文案：{failures:?}"
            );
        }
        other => panic!("应为 RouteCoding，实际 {other:?}"),
    }
}

// ---------------- 验收 18：正文中间的散文提及路由 → 不判回退 ----------------

/// 验收 18 · test agent 正文中间散文提及 `<route>coding</route>`、末尾是完成标记 → 不判回退，进 test review。
#[test]
fn a18_prose_route_mention_in_the_middle_is_not_a_route() {
    // 错实现形态：取全篇**第一个** `<route>` 出现处（散文提及即命中）→ 本用例必须变红
    let f = fixture();
    let mention = "我先跑了既有全套 → exit 0，15 例全绿，未触发 `<route>coding</route>`。\n<stage-complete>1</stage-complete>";
    // 第二条是「真的回退」：实现若把散文提及当成路由，就会追补一次并接受这条回注 → 用例变红
    let test_agent = ScriptedAgent::new(&[
        mention,
        "<route>coding</route>\n<issues>\n- 明细一\n</issues>",
    ]);
    let test_review = ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]);
    let opened = test_review.clone();
    let seen = test_agent.clone();
    let mut session = testing_only(f.ctx.clone(), test_agent, test_review);

    let turn = session.run_testing(&mut NoUser).expect("散文提及不是回退");
    assert!(
        matches!(turn, TestingTurn::Pass { .. }),
        "散文提及不得判回退，应走通过路径：{turn:?}"
    );
    assert_eq!(opened.opened(), 1, "通过路径要照常进 test review");
    assert_eq!(
        seen.seen().len(),
        1,
        "散文提及不得触发回退分支（不得追补第二次）"
    );
}

// ---------------- 验收 19：尾部路由 + 明细 → 判回退 ----------------

/// 验收 19 · test agent 末尾是 `<route>coding</route>` + `<issues>` → 判回退（RouteCoding）。
#[test]
fn a19_tail_route_with_issues_is_a_route() {
    // 错实现形态：收紧成「只认末尾独立行」时把合法的尾部协议位也判没了 → 本用例必须变红
    let f = fixture();
    let test_agent = ScriptedAgent::new(&["<route>coding</route>\n<issues>\n- 明细一\n</issues>"]);
    let mut session = testing_only(
        f.ctx.clone(),
        test_agent,
        ScriptedReviewFactory::new(Vec::new()),
    );

    let turn = session.run_testing(&mut NoUser).expect("尾部协议位应判回退");
    match turn {
        TestingTurn::RouteCoding { failures } => {
            assert_eq!(failures, vec!["明细一".to_string()]);
        }
        other => panic!("应为 RouteCoding，实际 {other:?}"),
    }
}

// ---------------- 验收 20：尾部 `<route>plan</route>` + `<impact-gap>` → 越界申报照旧被识别 ----------------

/// 验收 20 · coding agent 末尾是 `<route>plan</route>` + `<impact-gap>`（三条目）→ 越界申报照旧被识别。
#[test]
fn a20_tail_route_with_impact_gap_is_still_a_boundary_declaration() {
    // 错实现形态：把「尾部协议位」判得过紧（合法的 route + impact-gap 组合判不出来）→ 本用例必须变红
    let declaration = "计划漏了三个文件。\n<route>plan</route>\n<impact-gap>\n\
- 文件：src/b.rs\n- 为什么必须改：需求依赖的接口在这里\n- 证据：src/b.rs:1\n\
- 文件：src/c.rs\n- 为什么必须改：同上\n- 证据：src/c.rs:1\n\
- 文件：src/d.rs\n- 为什么必须改：同上\n- 证据：src/d.rs:1\n\
</impact-gap>";
    let changed = vec![
        "src/b.rs".to_string(),
        "src/c.rs".to_string(),
        "src/d.rs".to_string(),
    ];
    let verdict = classify(&changed, &["src/a.rs".to_string()], declaration);
    match verdict {
        BoundaryVerdict::RoutePlan { gaps } => {
            assert_eq!(gaps.len(), 3, "三条申报都要被认出来：{gaps:?}");
        }
        other => panic!("越界申报应被打回 plan 补边界，实际 {other:?}"),
    }

    // 端到端：coding 轮走「申报分支」→ 不进审查轮（reviews = 0）
    let f = fixture();
    let coding_agent = ScriptedAgent::new(&[
        "基线取证：cargo test\nexit_code: 0\n<stage-complete>1</stage-complete>",
        declaration,
    ]);
    let coding_review = ScriptedReviewFactory::new(Vec::new());
    let opened = coding_review.clone();
    let mut session = coding_only(f.ctx.clone(), coding_agent, coding_review);
    let turn = session
        .run_coding(&mut NoUser, 0, None)
        .expect("申报分支不该报错");
    assert_eq!(turn.reviews, 0, "申报分支不进审查轮");
    assert_eq!(opened.opened(), 0, "申报分支不开审查实例");
}

// ---------------- 验收 21：中间提及路由 → 不判越界申报 ----------------

/// 验收 21 · coding agent 中间提及 `<route>plan</route>`、末尾是完成标记 → 引擎不判越界申报。
#[test]
fn a21_prose_route_mention_in_the_middle_is_not_a_boundary_declaration() {
    // 错实现形态：取全篇第一个 `<route>`（散文提及即命中 → 越界申报被凭空认出来）→ 本用例必须变红
    let reply = "我按计划改了（计划里写过 `<route>plan</route>` 这个词）。\n<stage-complete>1</stage-complete>";
    let verdict = classify(
        &["src/extra.rs".to_string()],
        &["src/a.rs".to_string()],
        reply,
    );
    assert!(
        matches!(verdict, BoundaryVerdict::OutOfBoundary { .. }),
        "散文提及不是申报 → 越界改动应判范围膨胀：{verdict:?}"
    );

    // 端到端：coding 轮走正常路径（进审查轮），不是申报分支
    let f = fixture();
    let coding_agent = ScriptedAgent::new(&[
        "基线取证：cargo test\nexit_code: 0\n<stage-complete>1</stage-complete>",
        reply,
    ]);
    let coding_review = ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]);
    let opened = coding_review.clone();
    let mut session = coding_only(f.ctx.clone(), coding_agent, coding_review);
    let turn = session
        .run_coding(&mut NoUser, 0, None)
        .expect("正常路径不该报错");
    assert_eq!(turn.reviews, 1, "正常路径照常进审查轮");
    assert_eq!(opened.opened(), 1, "正常路径开一个审查实例");
}

// ---------------- 验收 22：coding_protocol 解析器的 FAIL 空箱 → 违例 ----------------

/// 验收 22 · `parse_review_report` 收到 FAIL + fix 但无 issues → `protocol_violation = true` 且 issues 非空。
#[test]
fn a22_parse_review_report_fail_without_issues_is_a_violation() {
    // 错实现形态：空箱仍产出 `protocol_violation = false` + 空清单 → 本用例必须变红
    let report = parse_review_report(FAIL_EMPTY).expect("不再硬错误");
    assert!(
        report.protocol_violation,
        "FAIL 不带 issues 是协议违例：{report:?}"
    );
    assert!(
        !report.issues.is_empty(),
        "违例原因要进 issues 字段（供错误文本用）：{report:?}"
    );
}

// ---------------- 验收 23：output 解析器的 FAIL 空箱 → 违例 ----------------

/// 验收 23 · `parse_verdict` 收到 `<verdict>FAIL</verdict>`（无 issues）→ 判违例，不是空清单的 FAIL。
#[test]
fn a23_parse_verdict_fail_without_issues_is_not_an_empty_fail_list() {
    // 错实现形态：产出 `Verdict::Fail { issues: [] }`（空清单兼表违例）→ 本用例必须变红
    let v = parse_verdict("<verdict>FAIL</verdict>").expect("不再硬错误");
    match &v {
        Verdict::ProtocolViolation { reason } => {
            assert!(!reason.is_empty(), "违例原因要能进错误文本：{reason}");
        }
        other => panic!("必须判协议违例（不是空清单的 FAIL）：{other:?}"),
    }
    assert!(
        !matches!(&v, Verdict::Fail { issues } if issues.is_empty()),
        "不得用空清单兼表违例：{v:?}"
    );
}

// ---------------- 验收 24：生产者侧提示词契约 + 夹具漂移 ----------------

/// 验收 24 · change coding / test 两个角色提示词写明「放末尾」与「`<issues>` 块」，夹具与 builtin 逐字段一致。
#[test]
fn a24_change_agent_prompts_carry_the_tail_protocol_contract() {
    // 错实现形态：提示词没写「放末尾 / 用 `<issues>` 块」（契约缺失）或夹具与 builtin 漂移 → 本用例必须变红
    assert!(
        CHANGE_CODING_AGENT_PROMPT.contains("声明块放在回复末尾"),
        "change coding 提示词要写明「声明块放在回复末尾」"
    );
    assert!(
        CHANGE_CODING_AGENT_PROMPT.contains("<impact-gap>"),
        "change coding 提示词要保留 `<impact-gap>` 申报块"
    );
    assert!(
        CHANGE_TEST_AGENT_PROMPT.contains("<issues>"),
        "change test 提示词要写明用 `<issues>` 块逐条列失败明细"
    );
    assert!(
        CHANGE_TEST_AGENT_PROMPT.contains("回复末尾"),
        "change test 提示词要写明路由块与明细放在回复末尾"
    );
    assert!(
        CHANGE_TEST_AGENT_PROMPT.contains("逐条"),
        "change test 提示词要写明「逐条」列失败明细"
    );

    // 夹具漂移：与 builtin 逐字段一致
    let m = ConfigManifest::builtin();
    let expected: Vec<&AgentManifest> = m.agents.iter().filter(|a| a.name.starts_with("change-")).collect();
    assert_eq!(expected.len(), 6, "change agents 应有 6 个");
    let path = repo_root().join("tests/e2e/fixtures/change_agents.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("夹具缺失：{}", path.display()));
    let actual: Vec<AgentManifest> = serde_json::from_str(&raw).expect("夹具可解析");
    assert_eq!(
        serde_json::to_string(&actual).unwrap(),
        serde_json::to_string(&expected).unwrap(),
        "change agents 夹具与 builtin 漂移——重跑 dump_change_agents_fixture"
    );
}

// ---------------- 验收 25：源码守卫钉住 Quality / System 分层 ----------------

/// 验收 25 · 协议违例上限判 Quality、审查实例失败上限判 System（守卫不得空转）。
#[test]
fn a25_source_guard_pins_quality_for_violations_and_system_for_instance_failures() {
    // 错实现形态：把违例上限写成 System（会被自动重启）/ 把实例失败写成 Quality → 本用例必须变红
    let gate = include_str!("../src/sop/review_gate.rs");
    let q = gate
        .find("DevforgeError::Quality")
        .expect("共享门里应有 Quality 构造");
    let q_near = &gate[q..(q + 200).min(gate.len())];
    assert!(
        q_near.contains("输出协议违例"),
        "协议违例上限必须判 Quality（不自动重启）：{q_near}"
    );
    let s = gate
        .find("DevforgeError::System")
        .expect("共享门里应有 System 构造");
    let s_near = &gate[s..(s + 200).min(gate.len())];
    assert!(
        s_near.contains("判定为基础设施故障"),
        "审查实例失败上限必须判 System（可重启）：{s_near}"
    );
    let server = include_str!("../src/server/tests.rs");
    assert!(
        server.contains("review_gate.rs"),
        "源码守卫必须扩到新模块（否则绿但没守任何东西）"
    );

    // 行为面：同一个共享门里，违例 → Quality，实例失败 → System
    let mut factory = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        |text| {
            let report = parse_review_report(text)?;
            Ok(ReviewOutcome {
                report: report.clone(),
                violation: report.protocol_violation.then(|| report.issues.join("；")),
            })
        },
    )
    .unwrap_err();
    assert!(
        err.is_quality_failure(),
        "违例上限 = 质量失败（不自动重启）：{err}"
    );

    let mut factory = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        |text| {
            let report = parse_review_report(text)?;
            Ok(ReviewOutcome {
                report: report.clone(),
                violation: report.protocol_violation.then(|| report.issues.join("；")),
            })
        },
    )
    .unwrap_err();
    assert!(
        !err.is_quality_failure(),
        "实例失败上限 = 基础设施故障（可重启）：{err}"
    );
    assert_eq!(
        MAX_PROTOCOL_VIOLATIONS, 3,
        "违例重审上限常量是单一事实源（3）"
    );
    assert_eq!(
        MAX_REVIEW_INSTANCE_FAILURES, 3,
        "实例失败重试上限常量是单一事实源（3）"
    );
}

// ---------------- 验收 26：审查轮次计入重审 ----------------

/// 验收 26 · change 侧发生过一次违例重审 → 报告里的「审查轮次」计入这次重审（不是恒为 1）。
#[test]
fn a26_review_rounds_count_the_re_review() {
    // 错实现形态：把审查轮次硬编码成 1（重审被藏起来）→ 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(PASS),
    ]);
    let mut session = plan_only(f.ctx.clone(), plan_agent, plan_review);
    session.run_plan(&mut NoUser, 0).expect("违例后应通过");
    assert_eq!(
        session.plan_reviews(),
        2,
        "plan 审查轮次 = 实际开过的实例数（违例重审要计入）"
    );

    // 端到端：整条 run 的报告里，审查轮次 = plan(2) + coding(1) + test(1)
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(PASS),
    ]);
    let coding_agent = ScriptedAgent::new(&[
        "基线取证：cargo test\nexit_code: 0\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let test_agent = ScriptedAgent::new(&["测完了\n<stage-complete>1</stage-complete>"]);
    let mut session = prod_session(
        f.ctx.clone(),
        plan_agent,
        plan_review,
        coding_agent,
        ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]),
        test_agent,
        ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]),
    );
    let store = StateStore::new(f.ctx.blackboard.join("state"), StateKey::new("proj", "task", CHANGE_SOP));
    let run = ChangeRun {
        store: &store,
        repo: &f.ctx.repo,
        change_id: "change-1",
        baseline_sha: &f.baseline,
        criteria: Vec::new(),
        whitelist: Vec::new(),
        usage: "1,000 tokens".into(),
        run_id: "run-1".into(),
        wall_secs: 1,
    };
    let outcome = run_change(run, &mut session, &mut NoUser).expect("整条 run 应走通");
    assert_eq!(
        outcome.report.review_rounds, 4,
        "审查轮次 = 实际开过的实例数（plan 2 + coding 1 + test 1），不是恒为 1"
    );
}

// ---------------- 验收 27：决策台账 D026 ----------------

/// 验收 27 · 决策台账含 `## D026 ·` 一节（三裁决 + 三通道 + 尾部协议位 + 遗留），既有条目一字不动。
#[test]
fn a27_decision_ledger_registers_d026_without_touching_the_old_entries() {
    // 错实现形态：把 D026 写成 D024/D025（撞号）/ 删掉既有条目 → 本用例必须变红
    let path = repo_root().join("docs/plantree/plans/009-change-sop/decisions/README.md");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("台账不可读：{}", path.display()));
    assert_eq!(text.matches("## D026 ·").count(), 1, "D026 只能登记一次");
    assert_eq!(text.matches("## D024 ·").count(), 1, "D024 仍在且只登记一次");
    assert_eq!(text.matches("## D025 ·").count(), 1, "D025 仍在且只登记一次");

    let d026 = text
        .split("## D026 ·")
        .nth(1)
        .expect("台账里有 D026 一节");
    for anchor in [
        "三通道",
        "判负",
        "协议违例",
        "实例失败",
        "空箱必拒收",
        "请补齐失败明细后重发",
        "尾部协议位",
        "遗留",
    ] {
        assert!(d026.contains(anchor), "D026 缺登记项：{anchor}");
    }
}

// ---------------- 验收 28：topic 文档口径 ----------------

/// 验收 28 · agents-change.md 改正旧口径 + vs-mvp.md 改正差异四。
#[test]
fn a28_topic_docs_match_the_new_contract() {
    // 错实现形态：文档仍写「测试 review FAIL → `<route>coding</route>`」/「判基础设施故障」→ 本用例必须变红
    let agents = std::fs::read_to_string(
        repo_root().join("docs/plantree/plans/009-change-sop/topics/agents-change.md"),
    )
    .expect("agents-change.md 可读");
    assert!(
        !agents.contains("测试 review FAIL → `<route>coding</route>`"),
        "旧口径（与代码不符）必须改正"
    );
    for anchor in ["判负", "协议违例", "实例失败", "尾部协议位", "<issues>"] {
        assert!(agents.contains(anchor), "agents-change.md 缺新口径：{anchor}");
    }

    let vs = std::fs::read_to_string(
        repo_root().join("docs/plantree/plans/009-change-sop/topics/vs-mvp.md"),
    )
    .expect("vs-mvp.md 可读");
    assert!(
        !vs.contains("连续 3 次无标记 → 第 4 次判基础设施故障"),
        "差异四的旧口径（与代码不符）必须改正"
    );
    assert!(
        vs.contains("连续 3 次无标记 → 第 4 次判**质量失败**"),
        "差异四要写明「判质量失败」"
    );
    assert!(
        vs.contains("重审有界（≤3）"),
        "差异四要补「审查输出违例的重审有界（≤3）」"
    );
}

// ---------------- 验收 29/30/31：白名单常量与提交内容 ----------------

/// 验收 29 · 守卫文件的两个常量 = 本场基线 + coding 白名单 22 个文件，提交扫描通过。
#[test]
fn a29_guard_constants_point_at_this_change() {
    // 错实现形态：常量没刷新（还指着上一场的基线 / 上一场的白名单）→ 本用例必须变红
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("守卫文件可读");
    assert!(
        guard.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "守卫的 BASELINE 必须等于本场基线 sha {BASELINE}"
    );
    let listed = parse_whitelist(&guard);
    assert_eq!(
        listed.len(),
        CODING_WHITELIST.len(),
        "守卫的 WHITELIST 必须逐个列出 coding 白名单的 {} 个文件（实际 {listed:?}）",
        CODING_WHITELIST.len()
    );
    for f in CODING_WHITELIST {
        assert!(listed.iter().any(|x| x == f), "守卫的 WHITELIST 缺 {f}");
    }
    assert!(
        !listed.iter().any(|x| x == "tests/f6_observability.rs"),
        "测试阶段文件不得进 coding 步白名单常量"
    );

    // 提交扫描：coding 步提交的文件都在白名单内；其他提交只许是 tests/ 下的文件
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    let mut coding_files: Vec<String> = Vec::new();
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        for f in git(&root, &["show", "--name-only", "--format=", sha])
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let f = f.trim().to_string();
            if subject.contains(" step ") {
                assert!(
                    CODING_WHITELIST.contains(&f.as_str()),
                    "coding 步提交越界（白名单外）：{f}（提交 {subject}）"
                );
                coding_files.push(f);
            } else {
                assert!(
                    f.starts_with("tests/"),
                    "白名单外的非测试改动：{f}（提交 {subject}）"
                );
            }
        }
    }
    for f in CODING_WHITELIST {
        assert!(
            coding_files.iter().any(|x| x == f),
            "白名单文件 {f} 没被本场任何 coding 步提交改到"
        );
    }
}

/// 验收 30 · 上一场验收测试文件的两个常量与守卫文件逐字一致。
#[test]
fn a30_acceptance_file_constants_match_the_guard() {
    // 错实现形态：两份常量不同步（a16 比对失败）→ 本用例必须变红
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("守卫文件可读");
    let acc = std::fs::read_to_string(repo_root().join("tests/change_failure_context.rs"))
        .expect("验收测试文件可读");
    assert!(
        acc.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "验收测试文件的 BASELINE 必须与守卫一致"
    );
    assert_eq!(
        parse_whitelist(&acc),
        parse_whitelist(&guard),
        "两份 WHITELIST 常量必须逐字一致"
    );
}

/// 验收 31 · 本变更的提交内容 ⊆ 计划 §4 白名单（23 个文件），未跟踪产物不入库。
#[test]
fn a31_the_change_commits_stay_inside_the_plan_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来 → 本用例必须变红
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        for f in git(&root, &["show", "--name-only", "--format=", sha])
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let f = f.trim();
            assert!(
                PLAN_WHITELIST.contains(&f),
                "提交内容越界（计划 §4 白名单外）：{f}（提交 {subject}）"
            );
        }
    }
    for artifact in [".pi/goals/", "tests/e2e/results/", "docs/plantree/changes/"] {
        let touched = git(
            &root,
            &[
                "log",
                "--format=%H",
                "--name-only",
                &format!("{BASELINE}..HEAD"),
                "--",
                artifact,
            ],
        );
        assert!(
            touched.trim().is_empty(),
            "未跟踪产物 {artifact} 被提交进来了：{touched}"
        );
    }
}

/// 验收 31（续）· 计划 §4 白名单 = 23 个文件（coding 22 + 测试阶段 1）。
#[test]
fn a32_the_plan_whitelist_has_twenty_three_files() {
    // 错实现形态：白名单数目写错（21/24）→ 本用例必须变红
    assert_eq!(PLAN_WHITELIST.len(), 23, "计划 §4 白名单 = 23 个文件");
    assert_eq!(CODING_WHITELIST.len(), 22, "coding 步白名单 = 22 个文件");
    assert!(
        PLAN_WHITELIST.contains(&"tests/f6_observability.rs"),
        "计划 §4 白名单要含唯一的测试阶段文件"
    );
    assert!(
        !CODING_WHITELIST.contains(&"tests/f6_observability.rs"),
        "测试阶段文件不得进 coding 步白名单"
    );
}

// ---------------- 验收 32：结构体字段与构造点 ----------------

/// 验收 32 · `ProdChangeSession` 新增 `plan_review_instances`，全仓字面量构造点都补齐。
#[test]
fn a33_every_prod_change_session_literal_sets_the_new_field() {
    // 错实现形态：某个字面量构造点漏补 `plan_review_instances` → 编译不过（这里逐点扫出来）→ 本用例必须变红
    let root = repo_root();
    let mut checked = 0usize;
    for rel in [
        "src/change/session.rs",
        "src/server/change_engine.rs",
        "tests/change_failure_context.rs",
        "tests/change_ask_ring.rs",
    ] {
        let text = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|_| panic!("不可读：{rel}"));
        let mut rest = text.as_str();
        while let Some(pos) = rest.find("ProdChangeSession {") {
            let head = &rest[..pos];
            // `pub struct ProdChangeSession {` 是定义、`impl ProdChangeSession {` /
            // `impl ChangeSession for ProdChangeSession {` 是方法块，都不是构造点
            let is_decl = ["struct ", "impl ", "for ", "enum ", "trait "]
                .iter()
                .any(|k| head.ends_with(k));
            let body = brace_body(rest, pos + "ProdChangeSession {".len() - 1);
            if !is_decl {
                assert!(
                    body.contains("plan_review_instances"),
                    "{rel} 的 ProdChangeSession 字面量漏补 plan_review_instances：\n{body}"
                );
                checked += 1;
            }
            rest = &rest[pos + "ProdChangeSession {".len()..];
        }
    }
    assert!(checked >= 4, "至少要扫到 4 个构造点（实际 {checked}）");
}

/// 取 `{` 起的配对花括号内容（结构体字面量的正文）。按**字节**扫描：花括号是 ASCII，
/// 多字节 UTF-8 序列里不会出现 ASCII 字节，按字节扫安全。
fn brace_body(src: &str, open_at: usize) -> String {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut end = bytes.len();
    for (i, c) in bytes.iter().enumerate().skip(open_at) {
        match c {
            b'{' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    String::from_utf8_lossy(&bytes[open_at..end]).to_string()
}

// ---------------- 工具 ----------------

/// 从守卫 / 验收测试文件的源码里解析白名单数组。
fn parse_whitelist(src: &str) -> Vec<String> {
    let start = src
        .find("const WHITELIST: &[&str] = &[")
        .expect("文件里应有 WHITELIST 常量");
    let rest = &src[start..];
    let end = rest.find("];").expect("WHITELIST 数组应有结尾");
    rest[..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix('"')?.strip_suffix("\",")
        })
        .map(|s| s.to_string())
        .collect()
}
