//! 回注失败上下文送达 coding agent —— 验收对账测试
//! change-20261021-coding-agent-c4-path-line-coding-src-cha（计划 §5 验收标准 1–17）
//!
//! 黑盒口径：只经**公开 API** 观察行为——`ChangeState` / `ChangeSession` / `run_change` /
//! `coding_kickoff` / `route_back_message` / 断点文件 / 仓内文档与常量——不窥探内部实现；
//! 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//!
//! 与仓内 `src/change/**` 的 `#[cfg(test)]` 单测相互独立：那一组在实现内部就地断言，
//! 本组从**仓外**（`tests/` 集成测试）看公开面，专门证伪「送达通道 / 清除语义 / 口径翻转」
//! 这三件事是否真的落到对外可观测的行为上。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use devforge::change::boundary::ImpactGap;
use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession, coding_kickoff};
use devforge::change::state::{
    CHANGE_SOP, ChangePhase, ChangeState, TestFailureContext, load, save,
};
use devforge::change::testing_gate::{FAILURE_BLOCK_TITLE, route_back_message};
use devforge::error::{DevforgeError, Result};
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

// ---------------- 常量 ----------------

/// 本变更的基线提交（计划 §1.1）。
const BASELINE: &str = "ad485549e0981b402bd53d64f2dc7e07c79e47b7";

/// 计划 §4 的白名单（coding 步的 18 个文件 = 19 个减去唯一的测试阶段文件
/// `tests/change_review_protocol.rs`）。
const WHITELIST: &[&str] = &[
    "src/sop/review_gate.rs",
    "src/sop/mod.rs",
    "src/sop/coding_protocol.rs",
    "src/sop/output.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/change/session.rs",
    "src/change/boundary.rs",
    "src/config/manifest.rs",
    "src/server/tests.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context.rs",
    "tests/e2e/fixtures/change_agents.json",
    "docs/plantree/plans/009-change-sop/decisions/README.md",
    "docs/plantree/plans/009-change-sop/topics/agents-change.md",
    "docs/plantree/plans/009-change-sop/topics/vs-mvp.md",
    "docs/plantree/plans/009-change-sop/roadmap.md",
];

/// 一条带 `path:line` 的失败明细（验收标准里的样例原文）。
const ISSUE_WITH_LOCATION: &str = "tests/change_ask_ring.rs:557 台账计数断言失败";
/// 一条不带位置的失败明细。
const ISSUE_PLAIN: &str = "接口返回 500";
/// 引擎在 test agent 不给 `<issues>` 时合成的占位文案（现状不动，属下一场协议硬化）。
const PLACEHOLDER_ISSUE: &str = "测试阶段判定失败（未给出 issues 明细）";

/// 三处「零信息 / 阉割痕迹」话术（验收标准 5、6、7 逐条点名）。
const ZERO_INFO_TRACES: &[&str] = &["请自己定位原因", "本消息不提供文件位置", "<位置略>"];

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

fn store(root: &Path) -> StateStore {
    StateStore::new(root, StateKey::new("proj", "task", CHANGE_SOP))
}

struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 脚本化 session：按队列吐出各阶段结果，并记录每轮 coding 收到的失败上下文。
struct Scripted {
    plans: VecDeque<PlanTurn>,
    amends: VecDeque<PlanTurn>,
    codings: VecDeque<CodingTurn>,
    testings: VecDeque<TestingTurn>,
    /// coding 轮直接报错（用来观察「coding 过门之前」的断点状态）。
    coding_error: Option<String>,
    /// testing 轮直接报错（用来观察「coding 过门那一刻」的断点状态）。
    testing_error: Option<String>,
    /// 每轮 coding 收到的失败上下文（`None` = 无上下文）——送达面的观测点。
    seen_failures: Vec<Option<TestFailureContext>>,
}

impl Scripted {
    fn new() -> Self {
        Self {
            plans: VecDeque::new(),
            amends: VecDeque::new(),
            codings: VecDeque::new(),
            testings: VecDeque::new(),
            coding_error: None,
            testing_error: None,
            seen_failures: Vec::new(),
        }
    }
}

impl ChangeSession for Scripted {
    fn run_plan(&mut self, _ui: &mut dyn UserLink, _rt: usize) -> Result<PlanTurn> {
        Ok(self.plans.pop_front().expect("plan 脚本耗尽"))
    }

    fn amend_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _gaps: &[ImpactGap],
        _rt: usize,
    ) -> Result<PlanTurn> {
        Ok(self.amends.pop_front().expect("amend 脚本耗尽"))
    }

    fn fix_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _issues: &[String],
        _round: usize,
    ) -> Result<PlanTurn> {
        Err(DevforgeError::Quality("fix_plan 不该被调用".into()))
    }

    fn run_coding(
        &mut self,
        _ui: &mut dyn UserLink,
        _rt: usize,
        last_failure: Option<&TestFailureContext>,
    ) -> Result<CodingTurn> {
        self.seen_failures.push(last_failure.cloned());
        if let Some(msg) = &self.coding_error {
            return Err(DevforgeError::Quality(msg.clone()));
        }
        Ok(self.codings.pop_front().expect("coding 脚本耗尽"))
    }

    fn run_testing(&mut self, _ui: &mut dyn UserLink) -> Result<TestingTurn> {
        if let Some(msg) = &self.testing_error {
            return Err(DevforgeError::Quality(msg.clone()));
        }
        Ok(self.testings.pop_front().expect("testing 脚本耗尽"))
    }
}

fn in_boundary() -> CodingTurn {
    CodingTurn {
        reply: "干完了\n<stage-complete>1</stage-complete>".into(),
        changed: vec!["src/a.rs".into()],
        reviews: 1,
    }
}

fn pass_testing() -> TestingTurn {
    TestingTurn::Pass {
        acceptance_done: vec!["x=1 → 2".into()],
        reviews: 1,
    }
}

/// 造一个 change 上下文（目标仓 + 黑板都在临时目录里）。
fn ctx_in(tmp: &Path) -> ChangeCtx {
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let blackboard = tmp.join("bb");
    std::fs::create_dir_all(&blackboard).unwrap();
    let change_dir = repo.join("docs/plantree/changes/change-20261020-x");
    std::fs::create_dir_all(&change_dir).unwrap();
    ChangeCtx {
        repo,
        blackboard,
        change_dir,
        change_id: "change-20261020-x".into(),
        baseline: "base123".into(),
        requirement: "改 a".into(),
        doc_level: DocLevel::None,
    }
}

fn cfg<'a>(
    store: &'a StateStore,
    repo: &'a Path,
    wl: &[&str],
    criteria: &[&str],
) -> ChangeRun<'a> {
    ChangeRun {
        store,
        repo,
        change_id: "change-20261020-x",
        baseline_sha: "base123",
        criteria: criteria.iter().map(|s| s.to_string()).collect(),
        whitelist: wl.iter().map(|s| s.to_string()).collect(),
        usage: "1,000 tokens（in 900 / out 100）".into(),
        run_id: "run-1".into(),
        wall_secs: 12,
    }
}

fn issues() -> Vec<String> {
    vec![ISSUE_WITH_LOCATION.to_string(), ISSUE_PLAIN.to_string()]
}

fn context(round: usize) -> TestFailureContext {
    TestFailureContext {
        round,
        issues: issues(),
    }
}

// ---------------- 验收 1：新状态无上下文 + 无上下文时开工语逐字不变 ----------------

/// 验收 1 · 新建 `ChangeState` 无失败上下文；`coding_kickoff(ctx, None)` **逐字**等于改动前的开工语。
#[test]
fn a1_new_state_has_no_context_and_none_kickoff_is_verbatim_unchanged() {
    // 错实现形态：①新状态自带一份空上下文（空 Vec / 哨兵值）；②无上下文时也往开工语里塞空块
    // （或悄悄改了正文的措辞）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx_in(tmp.path());

    // ① 新建状态 = 无上下文
    let st = ChangeState::new("change-20261020-x", "base123");
    assert!(
        st.last_test_failure.is_none(),
        "新建状态必须没有失败上下文：{:?}",
        st.last_test_failure
    );

    // ② 无上下文时开工语逐字等于**基线提交里那版**的开工语
    let kick = coding_kickoff(&ctx, None);
    assert_eq!(
        kick,
        baseline_coding_kickoff(&ctx),
        "无上下文时的开工语必须逐字等于改动前那版"
    );
    assert!(
        !kick.contains(FAILURE_BLOCK_TITLE),
        "无上下文时不得注入标题行：{kick}"
    );
    assert!(!kick.contains("失败明细"), "无上下文时不得注入明细：{kick}");
    for issue in issues() {
        assert!(
            !kick.contains(&issue),
            "无上下文时不得出现 issue 原文（{issue}）：{kick}"
        );
    }
    assert!(!kick.ends_with("\n\n"), "无上下文时不得留空块尾巴：{kick:?}");

    // ③ 有上下文时正文逐字不变、只追加送达块（证明基座没被动过）
    let with = coding_kickoff(&ctx, Some(&context(1)));
    assert!(
        with.starts_with(&kick),
        "有上下文时开工语正文必须逐字保留基座"
    );
    assert_eq!(
        with,
        format!("{kick}\n\n{}", devforge::change::testing_gate::coding_failure_block(&context(1))),
        "有上下文时开工语 = 基座 + 送达块"
    );
}

/// 从基线提交里取出**改动前**的开工语（逐字比对用）。
fn baseline_coding_kickoff(ctx: &ChangeCtx) -> String {
    let src = git(&repo_root(), &["show", &format!("{BASELINE}:src/change/session.rs")]);
    let start = src
        .find("\"你是 change-coding-agent。change-id：")
        .expect("基线里应有 coding 开工语");
    let rest = &src[start + 1..];
    let end = rest.find("\",").expect("基线开工语的字符串字面量应有结尾");
    let literal = &rest[..end];
    // Rust 源码里的行继续符 `\` + 换行：字符串里不插入换行
    let joined = literal.replace("\\\n", "");
    // 源码里的 `\n` 是真换行
    let text = joined.replace("\\n", "\n");
    // 占位符按出现顺序填值（改动前的顺序：change_id / baseline / repo / plan / blackboard / 自证日志）
    let values = [
        ctx.change_id.clone(),
        ctx.baseline.clone(),
        ctx.repo.display().to_string(),
        ctx.plan_path().display().to_string(),
        ctx.blackboard.display().to_string(),
        devforge::change::coding_gate::SELF_CHECK_LOG.to_string(),
    ];
    let mut out = String::new();
    let mut rest_text = text.as_str();
    for (i, v) in values.iter().enumerate() {
        let Some(idx) = rest_text.find("{}") else {
            panic!("基线开工语的占位符数变了（填到第 {i} 个就没了）：{text}");
        };
        out.push_str(&rest_text[..idx]);
        out.push_str(v);
        rest_text = &rest_text[idx + 2..];
    }
    out.push_str(rest_text);
    out
}

// ---------------- 验收 2：回注写入断点 ----------------

/// 验收 2 · 编排器收到 `RouteCoding`（两条 failures）→ 断点里有失败上下文、轮次 1、issues 逐字。
#[test]
fn a2_reroute_writes_the_failure_context_into_the_checkpoint() {
    // 错实现形态：回注时不写失败上下文 / 写的轮次不是 1 / 剥了 path:line → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: issues(),
    });

    let mut ui = NoUser;
    let err = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap_err();
    assert!(err.is_quality_failure(), "回注是 Quality 终止：{err}");

    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.test_reroutes, 1, "回注计数必须是 1");
    assert_eq!(
        saved.last_test_failure,
        Some(context(1)),
        "断点里的失败上下文必须逐字等于两条原文（轮次 1）"
    );
    let ctx = saved.last_test_failure.expect("上下文必须在");
    assert_eq!(ctx.round, 1, "轮次必须是第 1 轮回注");
    assert_eq!(ctx.issues, issues(), "issues 必须是原文");
    assert!(
        ctx.issues[0].contains("tests/change_ask_ring.rs:557"),
        "带位置的明细不许被剥：{:?}",
        ctx.issues
    );
}

// ---------------- 验收 3：跨会话保留 ----------------

/// 验收 3 · 换一个会话重新加载同一黑板 → 失败上下文仍在、轮次仍 1、issues 逐字不变。
#[test]
fn a3_the_failure_context_survives_across_sessions() {
    // 错实现形态：上下文只活在内存里（换个会话就没了 / 冷启动清零）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: issues(),
    });
    let mut ui = NoUser;
    let _ = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    );

    // 新会话：新的 store 实例、新的 session 对象，同一个黑板路径
    let st2 = store(tmp.path());
    let reloaded = load(&st2, "change-20261020-x", "base123");
    assert_eq!(
        reloaded.last_test_failure,
        Some(context(1)),
        "跨会话重新加载后失败上下文必须逐字不变"
    );
    assert_eq!(reloaded.test_reroutes, 1, "跨会话回注计数不丢");

    // 再跑一次：新会话的 coding 轮必须收到**同一份**上下文（送达面跨会话一致）
    let mut session2 = Scripted::new();
    session2.coding_error = Some("故意在 coding 轮报错（观察送达）".into());
    let mut ui2 = NoUser;
    let _ = run_change(
        cfg(&st2, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session2,
        &mut ui2,
    );
    assert_eq!(
        session2.seen_failures,
        vec![Some(context(1))],
        "续跑时必须把断点里的同一份上下文送达 coding"
    );
    let after = load(&st2, "change-20261020-x", "base123");
    assert_eq!(
        after.last_test_failure,
        Some(context(1)),
        "coding 轮没走完时上下文不得被清"
    );
}

// ---------------- 验收 4 / 5：开工语送达块 ----------------

/// 验收 4 · 带上下文的开工语含标题行，且逐字含两条 issue 原文。
#[test]
fn a4_coding_kickoff_carries_the_title_and_the_issues_verbatim() {
    // 错实现形态：送达块缺标题行 / 把 issue 改写或截断 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx_in(tmp.path());
    let kick = coding_kickoff(&ctx, Some(&context(1)));
    assert!(
        kick.contains("上一轮测试失败明细如下（逐条原文）"),
        "开工语要含标题行「上一轮测试失败明细如下（逐条原文）」：{kick}"
    );
    for issue in issues() {
        assert!(
            kick.contains(&issue),
            "开工语必须逐字含 issue 原文（{issue}）：{kick}"
        );
    }
    assert!(
        kick.contains("tests/change_ask_ring.rs:557"),
        "带位置的明细不许在送达面被剥：{kick}"
    );
}

/// 验收 5 · 带上下文的开工语不含三处零信息 / 阉割痕迹。
#[test]
fn a5_coding_kickoff_has_no_zero_information_boilerplate() {
    // 错实现形态：送达块里还留着「请自己定位原因」这类话术 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx_in(tmp.path());
    let kick = coding_kickoff(&ctx, Some(&context(1)));
    for trace in ZERO_INFO_TRACES {
        assert!(
            !kick.contains(trace),
            "开工语不得含零信息话术「{trace}」：{kick}"
        );
    }
}

// ---------------- 验收 6 / 7：回注消息（两个路由目标同口径） ----------------

/// 验收 6 · `route_back_message("coding", …)` 逐字含 `path:line`，且不含零信息话术。
#[test]
fn a6_route_back_message_to_coding_keeps_the_location() {
    // 错实现形态：回注消息把 path:line 剥成 `<位置略>` / 写死「本消息不提供文件位置」→ 必须变红
    let msg = route_back_message("coding", &[ISSUE_WITH_LOCATION.to_string()]);
    assert!(
        msg.contains("tests/change_ask_ring.rs:557"),
        "回注消息必须逐字含 path:line：{msg}"
    );
    for trace in ZERO_INFO_TRACES {
        assert!(!msg.contains(trace), "回注消息不得含「{trace}」：{msg}");
    }
}

/// 验收 7 · `route_back_message("plan", …)` 与 coding 目标同口径。
#[test]
fn a7_route_back_message_to_plan_keeps_the_location() {
    // 错实现形态：只给 coding 目标保留定位、plan 目标仍剥掉 → 必须变红
    let msg = route_back_message("plan", &[ISSUE_WITH_LOCATION.to_string()]);
    assert!(
        msg.contains("tests/change_ask_ring.rs:557"),
        "plan 目标的回注消息也必须逐字含 path:line：{msg}"
    );
    for trace in ZERO_INFO_TRACES {
        assert!(!msg.contains(trace), "plan 目标不得含「{trace}」：{msg}");
    }
}

// ---------------- 验收 8：消费即清 ----------------

/// 验收 8 · coding 轮过门（阶段转 testing）→ 断点里的失败上下文为「无上下文」。
#[test]
fn a8_coding_success_clears_the_failure_context() {
    // 错实现形态：coding 过门后不清上下文（过期失败继续喂下一轮）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 预置断点：coding 阶段 + 一份失败上下文
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Coding;
    seeded.set_test_failure(1, issues());
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.codings.push_back(in_boundary());
    // testing 阶段直接报错：断点停在「coding 过门」那一刻（清除已生效、后续没再改）
    session.testing_error = Some("故意在 testing 轮报错（观察清除点）".into());
    let mut ui = NoUser;
    let _ = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    );

    let after = load(&st, "change-20261020-x", "base123");
    assert_eq!(
        after.phase,
        ChangePhase::Testing,
        "coding 过门后阶段应转 testing"
    );
    assert!(
        after.last_test_failure.is_none(),
        "消费即清：coding 过门后不得留失败上下文：{:?}",
        after.last_test_failure
    );
}

// ---------------- 验收 9：覆盖语义 ----------------

/// 验收 9 · 第二次回注 → issues 等于新列表原文、不含旧 issues、轮次为 2。
#[test]
fn a9_second_reroute_overwrites_the_failure_context() {
    // 错实现形态：失败上下文是追加而不是覆盖（旧失败继续喂给 coding）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());

    // 第 1 轮回注
    let mut s1 = Scripted::new();
    s1.plans.push_back(PlanTurn::Pass);
    s1.codings.push_back(in_boundary());
    s1.testings.push_back(TestingTurn::RouteCoding {
        failures: issues(),
    });
    let mut ui = NoUser;
    let _ = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut s1,
        &mut ui,
    );

    // 第 2 轮回注（新的 failures 列表）
    let new_issue = "src/change/orchestrator.rs:173 送达面断链".to_string();
    let mut s2 = Scripted::new();
    s2.codings.push_back(in_boundary());
    s2.testings.push_back(TestingTurn::RouteCoding {
        failures: vec![new_issue.clone()],
    });
    let _ = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut s2,
        &mut ui,
    );

    let saved = load(&st, "change-20261020-x", "base123");
    let ctx = saved.last_test_failure.expect("第 2 轮回注后必须有上下文");
    assert_eq!(ctx.round, 2, "轮次必须是第 2 轮回注");
    assert_eq!(ctx.issues, vec![new_issue.clone()], "issues 必须是新列表原文");
    assert!(
        !ctx.issues.contains(&ISSUE_WITH_LOCATION.to_string()),
        "覆盖语义：旧 issues 不得残留：{:?}",
        ctx.issues
    );
    assert_eq!(saved.test_reroutes, 2, "回注计数必须是 2");

    // 覆盖语义的直接观测：断点里**已有**上下文时再回注一次，新列表必须覆盖旧的（不追加）
    let tmp2 = tempfile::tempdir().unwrap();
    let st2 = store(tmp2.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.test_reroutes = 1;
    seeded.set_test_failure(1, issues());
    save(&st2, &seeded).unwrap();

    let mut s3 = Scripted::new();
    s3.testings.push_back(TestingTurn::RouteCoding {
        failures: vec![new_issue.clone()],
    });
    let mut ui3 = NoUser;
    let _ = run_change(
        cfg(&st2, tmp2.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut s3,
        &mut ui3,
    );
    let saved2 = load(&st2, "change-20261020-x", "base123");
    let ctx2 = saved2.last_test_failure.expect("回注后必须有上下文");
    assert_eq!(
        ctx2.issues,
        vec![new_issue.clone()],
        "覆盖语义：新列表必须覆盖旧 issues（不追加）：{:?}",
        ctx2.issues
    );
    assert!(
        !ctx2.issues.contains(&ISSUE_WITH_LOCATION.to_string()),
        "覆盖语义：旧 issues 不得残留：{:?}",
        ctx2.issues
    );
    assert_eq!(ctx2.round, 2, "轮次必须是第 2 轮回注");
}

// ---------------- 验收 10：终态清 ----------------

/// 验收 10 · 测试通过（阶段转 done）→ 断点里的失败上下文为「无上下文」。
#[test]
fn a10_testing_pass_clears_the_failure_context() {
    // 错实现形态：run 到 done 不清上下文（收尾留过期状态）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 预置断点：testing 阶段 + 一份失败上下文
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.set_test_failure(1, issues());
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    let out = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done, "测试通过后应转 done");
    assert!(
        out.state.last_test_failure.is_none(),
        "终态清：done 时不得留失败上下文：{:?}",
        out.state.last_test_failure
    );
    let on_disk = load(&st, "change-20261020-x", "base123");
    assert!(
        on_disk.last_test_failure.is_none(),
        "终态清：落盘断点里也不得有失败上下文"
    );
}

// ---------------- 验收 11：回注超限既不写也不清 ----------------

/// 验收 11 · 回注次数超上限 → 报错含「上限 3」，失败上下文既没被写入新失败、也没被清除。
#[test]
fn a11_reroute_over_the_limit_neither_writes_nor_clears() {
    // 错实现形态：超限分支顺手写了新上下文 / 顺手清了旧上下文 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 预置断点：testing 阶段、回注计数已达上限、**已有一份**上下文
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.test_reroutes = 3;
    seeded.set_test_failure(1, issues());
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: vec!["新一轮的失败".into()],
    });
    let mut ui = NoUser;
    let err = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("上限 3"),
        "报错文本要含「上限 3」：{err}"
    );
    let after = load(&st, "change-20261020-x", "base123");
    assert_eq!(
        after.last_test_failure,
        Some(context(1)),
        "超限分支既不写也不清：上下文必须与超限前逐字相同"
    );

    // 超限前**没有**上下文 → 超限后仍为「无上下文」
    let tmp2 = tempfile::tempdir().unwrap();
    let st2 = store(tmp2.path());
    let mut seeded2 = ChangeState::new("change-20261020-x", "base123");
    seeded2.phase = ChangePhase::Testing;
    seeded2.test_reroutes = 3;
    save(&st2, &seeded2).unwrap();
    let mut session2 = Scripted::new();
    session2.testings.push_back(TestingTurn::RouteCoding {
        failures: vec!["新一轮的失败".into()],
    });
    let mut ui2 = NoUser;
    let _ = run_change(
        cfg(&st2, tmp2.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session2,
        &mut ui2,
    );
    let after2 = load(&st2, "change-20261020-x", "base123");
    assert!(
        after2.last_test_failure.is_none(),
        "超限前没有上下文 → 超限后仍为「无上下文」：{:?}",
        after2.last_test_failure
    );
}

// ---------------- 验收 12：旧断点缺键安全 ----------------

/// 验收 12 · 旧版断点文件（`change` 段里没有失败上下文这个键）→ 加载成功、上下文为无、计数逐字不变。
#[test]
fn a12_old_checkpoint_without_the_failure_key_still_loads() {
    // 错实现形态：新字段不是 Option / 缺 `#[serde(default)]` 导致缺键报错 → 断点被静默冷启动（计数全丢）
    // → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 手写一份「旧版」断点：change 段里没有 last_test_failure 这个键
    let legacy = serde_json::json!({
        "schema_version": 1,
        "sop": "change",
        "project": "proj",
        "task": "task",
        "updated_at_ms": 1,
        "attempt": 0,
        "data": {
            "change": {
                "phase": "coding",
                "baseline_sha": "base123",
                "roundtrips": 2,
                "impl_steps_done": [1, 2],
                "acceptance_done": ["x=1 → 2"],
                "review_rounds": 3,
                "test_reroutes": 1,
                "change_id": "change-20261020-x"
            }
        }
    });
    let path = st.path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();

    let loaded = load(&st, "change-20261020-x", "base123");
    assert_eq!(loaded.phase, ChangePhase::Coding, "旧断点必须加载成功");
    assert!(
        loaded.last_test_failure.is_none(),
        "旧断点没有该键 → 上下文为「无上下文」"
    );
    assert_eq!(loaded.roundtrips, 2, "往返计数不得丢");
    assert_eq!(loaded.test_reroutes, 1, "回注计数不得丢");
    assert_eq!(loaded.impl_steps_done, vec![1, 2], "已完成步骤不得丢");
    assert_eq!(loaded.acceptance_done, vec!["x=1 → 2"], "已对账验收项不得丢");
    assert_eq!(loaded.review_rounds, 3, "审查轮次不得丢");
}

// ---------------- 验收 13：占位文案照原样送达 ----------------

/// 验收 13 · test agent 回 `<route>coding</route>` 且不带 `<issues>` → 引擎合成占位文案，
/// 该占位文案作为**一条** issue 原样进上下文，并被逐字送进 kickoff。
#[test]
fn a13_placeholder_issue_text_is_delivered_verbatim() {
    // 错实现形态：占位文案被改写 / 被当成多条 / 没进上下文 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx_in(tmp.path());

    // 真 session 侧：test agent 只回路由、不给 issues
    let reply = "<route>coding</route>\n<stage-complete>1</stage-complete>".to_string();
    let mut prod = ProdChangeSession {
        plan_agent: Box::new(FixedAgent::new("<stage-complete>1</stage-complete>".into())),
        plan_review: Box::new(NoReview),
        coding_agent: Box::new(FixedAgent::new("<stage-complete>1</stage-complete>".into())),
        coding_review: Box::new(NoReview),
        test_agent: Box::new(FixedAgent::new(reply)),
        test_review: Box::new(NoReview),
        ctx: ctx.clone(),
    };
    let mut ui = NoUser;
    let turn = prod.run_testing(&mut ui).unwrap();
    let TestingTurn::RouteCoding { failures } = turn else {
        panic!("test agent 回 <route>coding</route> 时应得到 RouteCoding");
    };
    assert_eq!(
        failures,
        vec![PLACEHOLDER_ISSUE.to_string()],
        "引擎仍合成占位文案（现状不动）"
    );

    // 编排侧：占位文案作为一条 issue 进上下文
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: failures.clone(),
    });
    let mut ui2 = NoUser;
    let _ = run_change(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui2,
    );
    let saved = load(&st, "change-20261020-x", "base123");
    let failure = saved.last_test_failure.clone().expect("占位文案必须进上下文");
    assert_eq!(failure.round, 1);
    assert_eq!(
        failure.issues,
        vec![PLACEHOLDER_ISSUE.to_string()],
        "占位文案作为一条 issue 原样入上下文"
    );

    // 送达面：逐字送进 kickoff
    let kick = coding_kickoff(&ctx, saved.last_test_failure.as_ref());
    assert!(
        kick.contains(PLACEHOLDER_ISSUE),
        "占位文案必须逐字送进 kickoff：{kick}"
    );
    assert!(
        kick.contains(FAILURE_BLOCK_TITLE),
        "占位文案照样走标题行送达块：{kick}"
    );
}

/// 只会照抄一句回复的假 agent。
struct FixedAgent {
    reply: String,
}

impl FixedAgent {
    fn new(reply: String) -> Self {
        Self { reply }
    }
}

impl PlanAgentSession for FixedAgent {
    fn converse(&mut self, _message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        Ok(self.reply.clone())
    }
}

/// 审查会话工厂：不产出任何报告（占位，避免误触真链路）。
struct NoReview;

impl ReviewSessionFactory for NoReview {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        Ok(Box::new(NoReviewSession))
    }
}

struct NoReviewSession;

impl ReviewSession for NoReviewSession {
    fn review(&mut self, _instruction: &str) -> Result<String> {
        Ok("<stage-verdict>PASS</stage-verdict>\n<next>advance</next>".into())
    }
}

// ---------------- 验收 14：决策台账 D025 ----------------

/// 验收 14 · 台账含 `## D025 ·` 一节，写明字段形态 / 写入点 / 清除三规则 / 续跑一致性；D018 带旁注。
#[test]
fn a14_decision_ledger_registers_d025_with_the_clearing_rules() {
    // 错实现形态：台账没登记 D025 / 只登记一半（缺清除规则或缺续跑一致性）/ D018 没加旁注 → 必须变红
    let path = repo_root().join("docs/plantree/plans/009-change-sop/decisions/README.md");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("台账不可读：{}", path.display()));
    assert_eq!(
        text.matches("## D025 ·").count(),
        1,
        "D025 在台账里必须只登记一次"
    );
    let body = section_body(&text, "## D025 ·");
    // 字段形态
    assert!(
        body.contains("TestFailureContext"),
        "D025 要写明失败上下文的字段形态：{body}"
    );
    assert!(body.contains("round"), "D025 要写明轮次字段：{body}");
    assert!(body.contains("issues"), "D025 要写明 issues 字段：{body}");
    // 写入点
    assert!(
        body.contains("写入") || body.contains("写进断点"),
        "D025 要写明失败上下文的写入点：{body}"
    );
    assert!(body.contains("断点"), "D025 要写明上下文住在断点里：{body}");
    // 清除语义三条规则
    for rule in ["覆盖", "消费即清", "终态清"] {
        assert!(
            body.contains(rule),
            "D025 要写明清除语义的一条规则「{rule}」：{body}"
        );
    }
    // 续跑一致性
    assert!(body.contains("续跑"), "D025 要写明续跑一致性：{body}");
    // D018 旁注
    let d018 = text
        .lines()
        .find(|l| l.starts_with("## D018 ·"))
        .expect("D018 标题在位");
    assert!(
        d018.contains("D025"),
        "D018 标题要带「定位口径由 D025 改写」的旁注（两边都标）：{d018}"
    );
}

/// 取某节正文（从标题行到下一个 `## ` 标题之前）。
fn section_body(text: &str, title_prefix: &str) -> String {
    let start = text
        .find(title_prefix)
        .unwrap_or_else(|| panic!("台账里没有 {title_prefix} 这一节"));
    let rest = &text[start..];
    let end = rest[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

// ---------------- 验收 15：三处旧口径改写 ----------------

/// 验收 15 · `agents-change.md` 的 :172 / :211 与 `vs-mvp.md` 的 :24 不再写旧口径。
#[test]
fn a15_the_three_old_wording_places_are_rewritten() {
    // 错实现形态：只改了两处 / 改写时漏了「不定位」这半句 → 本用例必须变红
    let agents = repo_root().join("docs/plantree/plans/009-change-sop/topics/agents-change.md");
    let agents_text = std::fs::read_to_string(&agents)
        .unwrap_or_else(|_| panic!("文档不可读：{}", agents.display()));
    let vs_mvp = repo_root().join("docs/plantree/plans/009-change-sop/topics/vs-mvp.md");
    let vs_mvp_text = std::fs::read_to_string(&vs_mvp)
        .unwrap_or_else(|_| panic!("文档不可读：{}", vs_mvp.display()));

    for (name, text, line_no) in [
        ("agents-change.md", &agents_text, 172usize),
        ("agents-change.md", &agents_text, 211),
        ("vs-mvp.md", &vs_mvp_text, 24),
    ] {
        let line = text
            .lines()
            .nth(line_no - 1)
            .unwrap_or_else(|| panic!("{name} 没有第 {line_no} 行"));
        assert!(
            line.contains("失败明细逐条原文"),
            "{name}:{line_no} 要写「失败明细逐条原文」：{line}"
        );
        for old in ["不做定位", "不定位、不分析原因"] {
            assert!(
                !line.contains(old),
                "{name}:{line_no} 还在写旧口径「{old}」：{line}"
            );
        }
    }
    // 全文不得残留旧口径的措辞
    for (name, text) in [
        ("agents-change.md", &agents_text),
        ("vs-mvp.md", &vs_mvp_text),
    ] {
        for old in ["不做定位", "不定位、不分析原因"] {
            assert!(
                !text.contains(old),
                "{name} 全文还残留旧口径「{old}」"
            );
        }
        assert!(
            text.contains("失败明细逐条原文"),
            "{name} 要写明失败明细逐条原文送达"
        );
    }
}

// ---------------- 验收 16：白名单守卫的两个常量 ----------------

/// 验收 16 · `tests/change_ask_ring.rs` 的 `BASELINE` / `WHITELIST` 与本场一致，且扫描通过。
#[test]
fn a16_the_whitelist_constants_match_this_change() {
    // 错实现形态：常量没刷新（还指着上一场的基线 / 上一场的白名单）→ 守卫会把本场提交判成越界
    // → 本用例必须变红
    let path = repo_root().join("tests/change_ask_ring.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("测试文件不可读：{}", path.display()));
    assert!(
        text.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "守卫的 BASELINE 必须等于本场基线 sha {BASELINE}"
    );
    let listed = parse_whitelist(&text);
    assert_eq!(
        listed.len(),
        WHITELIST.len(),
        "守卫的 WHITELIST 必须逐个列出本场白名单的 {} 个文件（实际 {listed:?}）",
        WHITELIST.len()
    );
    for f in WHITELIST {
        assert!(
            listed.iter().any(|x| x == f),
            "守卫的 WHITELIST 缺 {f}（实际 {listed:?}）"
        );
    }
    // 扫描必须通过：本场 coding 步提交的每个文件都在白名单内
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    let mut coding_files: Vec<String> = Vec::new();
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        if !subject.contains(" step ") {
            continue;
        }
        for f in git(&root, &["show", "--name-only", "--format=", sha])
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let f = f.trim();
            assert!(
                WHITELIST.contains(&f),
                "coding 步提交越界（白名单外）：{f}（提交 {subject}）"
            );
            coding_files.push(f.to_string());
        }
    }
    assert!(
        !coding_files.is_empty(),
        "本变更的 coding 步提交一个都没找到（提交信息格式变了？）"
    );
}

/// 从 `tests/change_ask_ring.rs` 的源码里解析白名单数组。
fn parse_whitelist(src: &str) -> Vec<String> {
    let start = src
        .find("const WHITELIST: &[&str] = &[")
        .expect("守卫文件里应有 WHITELIST 常量");
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

// ---------------- 验收 17：提交内容不越界 ----------------

/// 验收 17 · 本变更的提交内容白名单外无改动；未跟踪产物一律没被 `git add`。
#[test]
fn a17_the_change_commits_stay_inside_the_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来
    // → 本用例必须变红
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
            }
        }
    }
    // 白名单里的 10 个文件都要真的被本场改到（不是「少改了」也算过）
    for f in WHITELIST {
        assert!(
            coding_files.iter().any(|x| x == f),
            "白名单文件 {f} 没被本场任何 coding 步提交改到"
        );
    }
    // 未跟踪产物一律不得被 git add 进本变更
    for artifact in [
        ".pi/goals/",
        "tests/e2e/results/",
        "docs/plantree/changes/",
    ] {
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
