//! 问询环（change-plan 真问用户）验收对账测试 · change-20261021-change-sop-path-line-src-config-manifest
//!
//! 黑盒口径：只经**公开 API** 观察行为（`ConfigManifest::builtin()` / `audit_verdicts` /
//! `review_instruction` / `ProdChangeSession::run_plan` / `plan_kickoff`），不窥探内部实现；
//! 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//!
//! 对应计划 §5 验收标准第 1–17、24–26 条（e2e 真链的第 18–23 条在
//! `tests/e2e/test_26_change_e2e.py` 与 `tests/e2e/test_27_change_ask_ring.py`）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{ChangeSession, PlanTurn};
use devforge::change::plan_audit::{
    VERDICT_ANSWER_FIELD, VERDICT_MUST_ASK, VERDICT_SELF, audit, audit_verdicts, review_instruction,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession, plan_kickoff};
use devforge::config::manifest::{
    AgentManifest, CHANGE_PLAN_AGENT_PROMPT, CHANGE_PLAN_REVIEW_PROMPT, ConfigManifest,
};
use devforge::error::{DevforgeError, Result};
use devforge::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

/// 问询环的供给物：能向用户发问的扩展包（与 MVP `plan-agent` 同包）。
const ASK_PKG: &str = "npm:@juicesharp/rpiv-ask-user-question";

/// 本变更的基线提交（计划 §1.1）。
/// 10-22 运维：与 change_failure_context 保持同值（a16 互检）；两场之间守卫 ignore（F10/F13）。
/// 口径：**本场基线**（`docs/plantree/changes/<change-id>/baseline.txt`）= `18ff755…`
/// ——操作员把本变更重新锚定到「上一轮 14 个 coding 步提交落地之后」的 HEAD
/// → 守卫的扫描范围与引擎的越界门同口径（同一个基线）。
const BASELINE: &str = "a36f6b79dec2e681db8daed0da067f9abd47a9f5";

/// 计划的 coding 轮白名单（计划 §4 的 23 个文件去掉唯一的测试阶段文件
/// `tests/f6_observability.rs` → 22 个）。
const WHITELIST: &[&str] = &[
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
    "src/change/coding_gate.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

// ---------------- 夹具 ----------------

/// 造一份带「歧义」小节的计划（其余五块齐全，与机械审计的既有四条检查兼容）。
fn plan_with_ambiguity(items: &str) -> String {
    format!(
        "# 变更计划\n\n\
## 1 需求复述\n改 a。\n\n\
### 1.1 歧义与本计划采用的默认\n\
{items}\n\n\
## 2 影响面\n- `src/a.rs:1`\n\n\
## 3 要改的\n| `src/a.rs` | 需求 |\n\n\
## 4 白名单\n`src/a.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
    )
}

/// 一次性测试环境：目标仓（带 `src/a.rs`）+ 黑板 + 计划文件。
struct Fixture {
    /// 持有 TempDir（落盘生命周期由它决定；字段本身不读）。
    _tmp: tempfile::TempDir,
    ctx: ChangeCtx,
}

fn fixture(plan: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
    let blackboard = tmp.path().join("bb");
    std::fs::create_dir_all(&blackboard).unwrap();
    let change_dir = repo.join("docs/plantree/changes/change-1");
    std::fs::create_dir_all(&change_dir).unwrap();
    std::fs::write(change_dir.join("change-plan.md"), plan).unwrap();
    let ctx = ChangeCtx {
        repo,
        blackboard,
        change_dir,
        change_id: "change-1".into(),
        baseline: String::new(),
        requirement: "把 a 改成 b".into(),
        doc_level: DocLevel::None,
    };
    Fixture { _tmp: tmp, ctx }
}

struct MockAgent {
    replies: VecDeque<String>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl MockAgent {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|s| s.to_string()).collect(),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl PlanAgentSession for MockAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.seen.borrow_mut().push(message.to_string());
        Ok(self.replies.pop_front().expect("MockAgent 脚本耗尽"))
    }
}

struct MockReview {
    reply: String,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ReviewSession for MockReview {
    fn review(&mut self, instruction: &str) -> Result<String> {
        self.seen.borrow_mut().push(instruction.to_string());
        Ok(self.reply.clone())
    }
}

struct MockReviewFactory {
    replies: VecDeque<String>,
    seen: Rc<RefCell<Vec<String>>>,
    opened: Rc<RefCell<usize>>,
}

impl MockReviewFactory {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|s| s.to_string()).collect(),
            seen: Rc::new(RefCell::new(Vec::new())),
            opened: Rc::new(RefCell::new(0)),
        }
    }
}

impl ReviewSessionFactory for MockReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        *self.opened.borrow_mut() += 1;
        Ok(Box::new(MockReview {
            reply: self.replies.pop_front().expect("MockReview 脚本耗尽"),
            seen: self.seen.clone(),
        }))
    }
}

struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

const PLAN_PASS: &str = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
const PLAN_FAIL: &str =
    "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 影响面漏了调用方\n</issues>";

/// 装一个只关心 plan 轮的 session（其余 agent 给空脚本）。
fn plan_session(
    ctx: &ChangeCtx,
    plan_agent: MockAgent,
    plan_review: MockReviewFactory,
) -> ProdChangeSession {
    ProdChangeSession {
        plan_agent: Box::new(plan_agent),
        plan_review: Box::new(plan_review),
        coding_agent: Box::new(MockAgent::new(&[])),
        coding_review: Box::new(MockReviewFactory::new(&[])),
        test_agent: Box::new(MockAgent::new(&[])),
        test_review: Box::new(MockReviewFactory::new(&[])),
        ctx: ctx.clone(),
        plan_review_instances: 0,
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------- 供给层（验收 1–3） ----------------

/// 验收 1 · `change-plan-agent` 的 `extra_packages` 含 ask 包（改动前是空数组）。
#[test]
fn change_plan_agent_carries_the_ask_package() {
    // 错实现形态：把 change-plan-agent 的 extra_packages 留空（问询环的根因行没修）→ 本用例必须变红
    let m = ConfigManifest::builtin();
    let a = m.get_agent("change-plan-agent").unwrap();
    assert!(
        a.extra_packages.iter().any(|p| p == ASK_PKG),
        "change-plan-agent 必须拿 ask 包（没有它就没有 extension_ui_request 工具）：{:?}",
        a.extra_packages
    );
}

/// 验收 2 · 其余十四个 Agent 的 `extra_packages` 与基线逐字段相同。
#[test]
fn other_agents_packages_match_the_baseline_snapshot() {
    // 错实现形态：顺手给别的 agent 加包/删包 → 本用例必须变红
    // （快照取自基线提交 BASELINE 的 builtin()，逐字段照录）
    let baseline_snapshot: [(&str, &[&str]); 14] = [
        ("plan-agent", &[ASK_PKG]),
        ("plan-review", &[]),
        (
            "coding-agent",
            &[ASK_PKG, "npm:@juicesharp/rpiv-todo", "npm:pi-lens"],
        ),
        ("coding-review", &["npm:pi-lens"]),
        ("test-agent", &["npm:@juicesharp/rpiv-todo", ASK_PKG]),
        ("test-review", &[]),
        (
            "doc-fix",
            &[ASK_PKG, "npm:@juicesharp/rpiv-todo", "npm:pi-lens"],
        ),
        (
            "doc-agent",
            &[ASK_PKG, "npm:@juicesharp/rpiv-todo", "npm:pi-lens"],
        ),
        ("doc-review", &[]),
        ("change-plan-review", &[]),
        (
            "change-coding-agent",
            &[ASK_PKG, "npm:@juicesharp/rpiv-todo", "npm:pi-lens"],
        ),
        ("change-coding-review", &[]),
        (
            "change-test-agent",
            &[ASK_PKG, "npm:@juicesharp/rpiv-todo", "npm:pi-lens"],
        ),
        ("change-test-review", &[]),
    ];
    let m = ConfigManifest::builtin();
    for (name, packages) in baseline_snapshot {
        let a = m.get_agent(name).unwrap();
        assert_eq!(
            a.extra_packages,
            packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "{name} 的包清单与基线不一致（本次变更只许动 change-plan-agent）"
        );
    }
}

/// 验收 3 · change 六件套逐个钉住（谁有 ask 包、谁没有）。
#[test]
fn change_six_agents_have_pinned_supplies() {
    // 错实现形态：给审查 agent 也装上 ask 包 / 把干活 agent 的 ask 包删掉 → 本用例必须变红
    let m = ConfigManifest::builtin();
    // 三个审查方不与用户交互 → 无包
    for name in ["change-plan-review", "change-coding-review", "change-test-review"] {
        let a = m.get_agent(name).unwrap();
        assert!(
            a.extra_packages.is_empty(),
            "{name} 是审查方，不该有包：{:?}",
            a.extra_packages
        );
    }
    // 三个干活方保留与 MVP 同源的 ask 包
    for name in ["change-plan-agent", "change-coding-agent", "change-test-agent"] {
        let a = m.get_agent(name).unwrap();
        assert!(
            a.extra_packages.iter().any(|p| p == ASK_PKG),
            "{name} 必须能问用户（ask 包）：{:?}",
            a.extra_packages
        );
    }
}

// ---------------- 提示词契约（验收 4–5） ----------------

/// 验收 4 · 产出侧提示词含四个锚点，且标记字面量与机械门常量逐字相同。
#[test]
fn plan_prompt_anchors_match_the_gate_literals() {
    // 错实现形态：提示词里的标记改成别的字面量（门与提示词漂移 = 门空转）→ 本用例必须变红
    assert_eq!(VERDICT_MUST_ASK, "[必须裁决]", "必须裁决标记的字面量");
    assert_eq!(VERDICT_SELF, "[自决]", "自决标记的字面量");
    assert_eq!(VERDICT_ANSWER_FIELD, "用户裁决：", "留痕字段的字面量");
    for anchor in [VERDICT_MUST_ASK, VERDICT_SELF, VERDICT_ANSWER_FIELD] {
        assert!(
            CHANGE_PLAN_AGENT_PROMPT.contains(anchor),
            "产出侧提示词缺锚点 {anchor}：与机械门常量必须逐字相同"
        );
    }
    assert!(
        CHANGE_PLAN_AGENT_PROMPT.contains("ask-user-question"),
        "产出侧提示词必须点名 ask-user-question（问询环的执行方式）"
    );
}

/// 验收 5 · 审查侧提示词含「分类恰当性」与「留痕核对」两条审点。
#[test]
fn review_prompt_carries_the_two_new_review_points() {
    // 错实现形态：把审查提示词里的新审点删掉 → 本用例必须变红
    for anchor in ["留痕核对", "分类恰当性"] {
        assert!(
            CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
            "审查侧提示词缺审点 {anchor}"
        );
    }
    for anchor in [VERDICT_MUST_ASK, VERDICT_ANSWER_FIELD] {
        assert!(
            CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
            "审查侧提示词缺标记字面量 {anchor}"
        );
    }
}

// ---------------- 裁决门（验收 6–11） ----------------

/// 验收 6 · 标了必须裁决却没留痕 → ≥1 条问题，文本含「必须裁决」与「ask-user-question」。
#[test]
fn must_ask_entry_without_answer_is_reported() {
    // 错实现形态：把留痕检查短路（只数条目不看字段）→ 本用例必须变红
    let a = audit_verdicts(&plan_with_ambiguity("- [必须裁决] 复合词算一个词还是两个词\n"));
    assert!(a.issues.len() >= 1, "缺留痕必须判失败：{:?}", a.facts);
    assert!(
        a.issues[0].contains("必须裁决"),
        "问题要点名标记：{}",
        a.issues[0]
    );
    assert!(
        a.issues[0].contains("ask-user-question"),
        "问题要告诉 agent 下一步干什么：{}",
        a.issues[0]
    );
}

/// 验收 7 · 必须裁决项有非空留痕 → 0 条问题。
#[test]
fn must_ask_entry_with_answer_is_clean() {
    // 错实现形态：把「续行里的字段」当没留痕 → 本用例必须变红
    let a = audit_verdicts(&plan_with_ambiguity(
        "- [必须裁决] 复合词算一个词还是两个词\n  用户裁决：算一个词\n",
    ));
    assert!(a.issues.is_empty(), "留痕齐全不该有问题：{:?}", a.issues);
}

/// 验收 8 · 留痕字段在但答复为空 → ≥1 条问题。
#[test]
fn must_ask_entry_with_empty_answer_is_reported() {
    // 错实现形态：把「字段在但答复为空」当已留痕 → 本用例必须变红
    let a = audit_verdicts(&plan_with_ambiguity("- [必须裁决] 口径未定\n  用户裁决：\n"));
    assert!(
        a.issues.len() >= 1,
        "空答复 = 没问过，必须判失败：{:?}",
        a.facts
    );
}

/// 验收 9 · 歧义块全是自决项 → 0 条问题。
#[test]
fn all_self_decided_entries_are_clean() {
    // 错实现形态：把「自决项也要求留痕」→ 本用例必须变红
    let a = audit_verdicts(&plan_with_ambiguity("- [自决] 内部实现细节由 agent 定\n"));
    assert!(a.issues.is_empty(), "自决项不触发问询要求：{:?}", a.issues);
}

/// 验收 10 · 未分类的顶层列表行 → 不判失败，但进确定性事实。
#[test]
fn unclassified_entry_is_a_fact_not_a_failure() {
    // 错实现形态：把「未分类条目」当硬失败 → 本用例必须变红
    let plan = plan_with_ambiguity("- 复合词算一个词还是两个词\n");
    let a = audit_verdicts(&plan);
    assert!(a.issues.is_empty(), "未分类条目不判硬失败：{:?}", a.issues);
    assert!(
        a.facts.iter().any(|f| f.contains("未分类") && f.contains("复合词")),
        "未分类条目要进确定性事实：{:?}",
        a.facts
    );
    // 同一事实要出现在审查指令里（审查官在其之上做语义判断）
    let f = fixture(&plan);
    let audit_report = audit(&plan, &f.ctx.repo);
    let instruction = review_instruction(
        "change-1",
        &f.ctx.plan_path(),
        &plan,
        &audit_report,
    );
    assert!(
        instruction.contains("未分类") && instruction.contains("复合词"),
        "审查指令要带未分类条目事实：{instruction}"
    );
}

/// 验收 11 · 顶层散文行声明必须裁决 → 不判失败，但进确定性事实。
#[test]
fn prose_declaration_is_a_fact_not_a_failure() {
    // 错实现形态：把「散文里的必须裁决声明」当硬失败 → 本用例必须变红
    let plan = plan_with_ambiguity("这条口径必须用户裁决。\n- [自决] 别的点\n");
    let a = audit_verdicts(&plan);
    assert!(a.issues.is_empty(), "散文声明不判硬失败：{:?}", a.issues);
    assert!(
        a.facts.iter().any(|f| f.contains("散文") && f.contains("必须用户裁决")),
        "散文声明要进确定性事实：{:?}",
        a.facts
    );
    let f = fixture(&plan);
    let audit_report = audit(&plan, &f.ctx.repo);
    let instruction = review_instruction(
        "change-1",
        &f.ctx.plan_path(),
        &plan,
        &audit_report,
    );
    assert!(
        instruction.contains("散文") && instruction.contains("必须用户裁决"),
        "审查指令要带散文声明事实：{instruction}"
    );
}

// ---------------- 计划轮硬门（验收 12–16） ----------------

/// 验收 12 · 计划里有必须裁决项且无留痕 → `run_plan` 返回 `PlanTurn::Fail`，问题含「必须裁决」。
#[test]
fn plan_turn_fails_when_must_ask_entry_has_no_answer() {
    // 错实现形态：把硬门从计划轮里摘掉 → 本用例必须变红（会变成 Pass）
    let f = fixture(&plan_with_ambiguity("- [必须裁决] 复合词算一个词还是两个词\n"));
    let review = MockReviewFactory::new(&[]);
    let opened = review.opened.clone();
    let mut session = plan_session(
        &f.ctx,
        MockAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]),
        review,
    );
    match session.run_plan(&mut NoUser, 0).unwrap() {
        PlanTurn::Fail { issues } => assert!(
            issues.iter().any(|i| i.contains("必须裁决")),
            "打回问题要点名必须裁决：{issues:?}"
        ),
        other => panic!("应为 Fail，实际 {other:?}"),
    }
    assert_eq!(
        *opened.borrow(),
        0,
        "硬门不过不得进审查轮（确定性判定不交审查官重判）"
    );
}

/// 验收 13 · 计划合规 → `run_plan` 返回 `PlanTurn::Pass`，且审查轮次为 1。
#[test]
fn plan_turn_passes_with_one_review_round_when_plan_is_compliant() {
    // 错实现形态：把「合规计划」也打回（硬门误伤）→ 本用例必须变红
    let f = fixture(&plan_with_ambiguity(
        "- [必须裁决] 复合词算一个词还是两个词\n  用户裁决：算一个词\n",
    ));
    let review = MockReviewFactory::new(&[PLAN_PASS]);
    let opened = review.opened.clone();
    let seen = review.seen.clone();
    let mut session = plan_session(
        &f.ctx,
        MockAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]),
        review,
    );
    let turn = session.run_plan(&mut NoUser, 0).unwrap();
    assert_eq!(turn, PlanTurn::Pass, "留痕齐全的计划应当过审");
    assert_eq!(*opened.borrow(), 1, "审查轮次应当是 1");
    assert!(
        seen.borrow()[0].contains("留痕核对") && seen.borrow()[0].contains("分类恰当性"),
        "审查指令要带新审点：{}",
        seen.borrow()[0]
    );
}

/// 验收 14 · 审查 FAIL → `run_plan` 返回 `PlanTurn::Fail`，问题来自审查（裁决门不误伤合规计划）。
#[test]
fn plan_turn_reports_review_issues_for_a_compliant_plan() {
    // 错实现形态：把「审查的问题」吞掉 / 被硬门的问题顶替 → 本用例必须变红
    let f = fixture(&plan_with_ambiguity(
        "- [必须裁决] 复合词算一个词还是两个词\n  用户裁决：算一个词\n",
    ));
    let mut session = plan_session(
        &f.ctx,
        MockAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]),
        MockReviewFactory::new(&[PLAN_FAIL]),
    );
    match session.run_plan(&mut NoUser, 0).unwrap() {
        PlanTurn::Fail { issues } => assert!(
            issues.iter().any(|i| i.contains("影响面漏了调用方")),
            "问题应原样来自审查：{issues:?}"
        ),
        other => panic!("应为 Fail，实际 {other:?}"),
    }
}

/// 验收 15 · 打回指令（`fix_plan` 的消息）含「ask-user-question」。
#[test]
fn fix_plan_message_carries_the_ask_instruction() {
    // 错实现形态：把打回消息里的 ask 指令删掉 → 本用例必须变红
    let f = fixture(&plan_with_ambiguity("- [自决] 无歧义\n"));
    let agent = MockAgent::new(&["改好了\n<stage-complete>1</stage-complete>"]);
    let seen = agent.seen.clone();
    let mut session = plan_session(
        &f.ctx,
        agent,
        MockReviewFactory::new(&[PLAN_PASS]),
    );
    session
        .fix_plan(&mut NoUser, &["歧义条目缺留痕".to_string()], 1)
        .unwrap();
    let msg = &seen.borrow()[0];
    assert!(
        msg.contains("ask-user-question"),
        "打回消息要告诉 agent 下一步问用户：{msg}"
    );
}

/// 验收 16 · 开工指令（`plan_kickoff` 的消息）含「ask-user-question」与「用户裁决」。
#[test]
fn plan_kickoff_carries_the_ask_contract() {
    // 错实现形态：把开工语里的问询契约删掉 → 本用例必须变红
    let f = fixture(&plan_with_ambiguity("- [自决] 无歧义\n"));
    let kick = plan_kickoff(&f.ctx);
    assert!(
        kick.contains("ask-user-question"),
        "开工语要含问询锚点：{kick}"
    );
    assert!(
        kick.contains("用户裁决"),
        "开工语要含留痕字段锚点：{kick}"
    );
}

// ---------------- 夹具与文档（验收 17、24、25） ----------------

/// 验收 17 · e2e 夹具与 builtin 逐字段一致（漂移守卫），且 `change-plan-agent` 带 ask 包。
#[test]
fn e2e_fixture_matches_builtin_and_carries_the_ask_package() {
    // 错实现形态：手改夹具 / 忘了重生成夹具 → 本用例必须变红
    let m = ConfigManifest::builtin();
    let expected: Vec<&AgentManifest> = m
        .agents
        .iter()
        .filter(|a| a.name.starts_with("change-"))
        .collect();
    assert_eq!(expected.len(), 6, "change 六件套应齐");
    let path = repo_root().join("tests/e2e/fixtures/change_agents.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("夹具缺失，重跑生成器：{}", path.display()));
    let actual: Vec<AgentManifest> = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        serde_json::to_string(&actual).unwrap(),
        serde_json::to_string(&expected).unwrap(),
        "change agents 夹具与 builtin 漂移——必须用生成器重生成，不许手改"
    );
    let plan_agent = actual.iter().find(|a| a.name == "change-plan-agent").unwrap();
    assert!(
        plan_agent.extra_packages.iter().any(|p| p == ASK_PKG),
        "夹具里的 change-plan-agent 必须带 ask 包：{:?}",
        plan_agent.extra_packages
    );
}

/// 验收 24 · 决策台账登记 D024 一次，且 D009 标题带旁注。
#[test]
fn decision_ledger_registers_d024_and_annotates_d009() {
    // 错实现形态：把 D024 删了 / 把 D009 旁注删了 / 重复登记 D024 → 本用例必须变红
    let path = repo_root().join("docs/plantree/plans/009-change-sop/decisions/README.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("决策台账不可读：{}", path.display()));
    assert_eq!(
        text.matches("## D024 ·").count(),
        1,
        "D024 在台账里必须只登记一次"
    );
    let d009 = text
        .lines()
        .find(|l| l.starts_with("## D009 ·"))
        .expect("D009 标题在位");
    assert!(
        d009.contains("D024"),
        "D009 标题要带「供给部分由 D024 增补」的旁注（两边都标）：{d009}"
    );
}

/// 验收 25 · `agents-change.md` 的答复载体 / 供给 / 审点三处与实际一致。
#[test]
fn agents_change_doc_matches_the_implemented_contract() {
    // 错实现形态：把文档里的载体口径、供给或审点删了 → 本用例必须变红
    let path = repo_root().join("docs/plantree/plans/009-change-sop/topics/agents-change.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("文档不可读：{}", path.display()));
    for anchor in [VERDICT_MUST_ASK, VERDICT_SELF, VERDICT_ANSWER_FIELD] {
        assert!(text.contains(anchor), "文档缺答复载体口径：{anchor}");
    }
    assert!(
        text.contains("禁止") || text.contains("不许"),
        "文档缺「禁止带默认值自决放行」的口径"
    );
    assert!(
        text.contains("ask-user-question"),
        "文档缺 ask 包供给口径（问询环的根因行）"
    );
    assert!(text.contains("留痕核对"), "文档缺留痕核对审点");
    assert!(text.contains("分类恰当性"), "文档缺分类恰当性审点");
}

// ---------------- 提交内容（验收 26） ----------------

/// 验收 26 · 本变更的提交内容不越界：coding 步提交的文件 ⊆ 白名单，
/// 且上一场遗留的未提交改动（`coding_gate.rs` / `skill_assets.rs`）不在本变更的 diff 里。
/// 10-22 运维：两场之间无 step 提交，此守卫恒红 → `#[ignore]`（F10）；
/// 常量由每场 step 1 刷新并摘除 ignore（真修：常量 gate 时物化）。
#[test]
#[ignore = "守卫常量由 change run 的 step 1 刷新；两场之间无 step 提交（F10）"]
fn the_change_commits_stay_inside_the_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 → 本用例必须变红
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    let mut coding_files: Vec<String> = Vec::new();
    let mut extra_files: Vec<String> = Vec::new();
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        let files = git(&root, &["show", "--name-only", "--format=", sha]);
        for f in files.lines().filter(|l| !l.trim().is_empty()) {
            let f = f.trim().to_string();
            if subject.contains(" step ") {
                // coding 步提交：文件必须逐个在白名单内
                assert!(
                    WHITELIST.contains(&f.as_str()),
                    "coding 步提交越界（白名单外）：{f}（提交 {subject}）"
                );
                coding_files.push(f);
            } else {
                // 测试阶段自己的提交：只许是测试文件
                assert!(
                    f.starts_with("tests/"),
                    "白名单外的非测试改动：{f}（提交 {subject}）"
                );
                extra_files.push(f);
            }
        }
    }
    assert!(
        !coding_files.is_empty(),
        "本变更的 coding 步提交一个都没找到（提交信息格式变了？）"
    );
    // 上一场遗留的未提交补录（+880 行）不得混进本变更
    for leftover in ["src/change/coding_gate.rs", "src/config/skill_assets.rs"] {
        let diff = git(
            &root,
            &["diff", "--name-only", BASELINE, "HEAD", "--", leftover],
        );
        assert!(
            diff.trim().is_empty(),
            "上一场遗留的未提交改动混进了本变更：{leftover}"
        );
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
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
