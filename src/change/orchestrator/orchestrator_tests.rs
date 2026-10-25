//! ChangeOrchestrator 编排测试（ARC-R6 自 orchestrator.rs mod tests 平移，纯移动）。

use super::*;
use crate::runstate::StateKey;
use std::collections::VecDeque;

fn store(root: &Path) -> StateStore {
    // 引擎审计修复 #7：Done 路径的 diff_stats 不再吞失败——测试环境升级为真 git 仓
    //（生产 repo 恒为 git 仓，非 git fixture 是人为环境）；基线 sha 落文件供 cfg 读取。
    let sha = init_git(root);
    std::fs::write(root.join(".git/head-sha"), &sha).unwrap();
    StateStore::new(
        root,
        StateKey::new("proj", "task", crate::change::state::CHANGE_SOP),
    )
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

struct NoUser;
impl crate::sop::planning::UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 脚本化 session：按队列吐出各阶段结果，并记录调用序列。
struct Scripted {
    plans: VecDeque<PlanTurn>,
    fix_plans: VecDeque<PlanTurn>,
    amends: VecDeque<PlanTurn>,
    codings: VecDeque<CodingTurn>,
    testings: VecDeque<TestingTurn>,
    coding_error: Option<String>,
    /// amend_plan 队列耗尽时的替代行为：有值则直接返回 System 错（复验 #9 回归用）。
    amend_errors: VecDeque<String>,
    plan_review_count: usize,
    /// 每轮 coding 收到的失败上下文（`None` = 无上下文）——送达面的观测点。
    seen_failures: Vec<Option<TestFailureContext>>,
    pub log: Vec<String>,
}

impl Scripted {
    fn new() -> Self {
        Self {
            plans: VecDeque::new(),
            fix_plans: VecDeque::new(),
            amends: VecDeque::new(),
            codings: VecDeque::new(),
            testings: VecDeque::new(),
            coding_error: None,
            amend_errors: VecDeque::new(),
            plan_review_count: 0,
            seen_failures: Vec::new(),
            log: Vec::new(),
        }
    }
}

impl ChangeSession for Scripted {
    fn run_plan(&mut self, _ui: &mut dyn crate::sop::planning::UserLink, rt: usize) -> Result<PlanTurn> {
        self.log.push(format!("plan(rt={rt})"));
        Ok(self.plans.pop_front().expect("plan 脚本耗尽"))
    }
    fn amend_plan(
        &mut self,
        _ui: &mut dyn crate::sop::planning::UserLink,
        gaps: &[ImpactGap],
        rt: usize,
    ) -> Result<PlanTurn> {
        self.log.push(format!("amend(rt={rt},gaps={})", gaps.len()));
        if let Some(msg) = self.amend_errors.pop_front() {
            return Err(DevforgeError::System(msg));
        }
        Ok(self.amends.pop_front().expect("amend 脚本耗尽"))
    }
    fn fix_plan(
        &mut self,
        _ui: &mut dyn crate::sop::planning::UserLink,
        issues: &[String],
        round: usize,
    ) -> Result<PlanTurn> {
        self.log
            .push(format!("fix(rt={round},issues={})", issues.len()));
        Ok(self.fix_plans.pop_front().expect("fix_plan 脚本耗尽"))
    }
    fn plan_reviews(&self) -> usize {
        self.plan_review_count
    }

    fn run_coding(
        &mut self,
        _ui: &mut dyn crate::sop::planning::UserLink,
        rt: usize,
        last_failure: Option<&TestFailureContext>,
    ) -> Result<CodingTurn> {
        self.log.push(format!("coding(rt={rt})"));
        self.seen_failures.push(last_failure.cloned());
        if let Some(msg) = &self.coding_error {
            return Err(DevforgeError::Quality(msg.clone()));
        }
        Ok(self.codings.pop_front().expect("coding 脚本耗尽"))
    }
    fn run_testing(&mut self, _ui: &mut dyn crate::sop::planning::UserLink) -> Result<TestingTurn> {
        self.log.push("testing".into());
        Ok(self.testings.pop_front().expect("testing 脚本耗尽"))
    }
}

fn in_boundary() -> CodingTurn {
    CodingTurn {
        reply: "干完了\n<stage-complete>1</stage-complete>".into(),
        changed: s(&["src/a.rs"]),
        reviews: 1,
    }
}

fn declared_gap() -> CodingTurn {
    declared_gap_file("src/extra.rs")
}

fn declared_gap_file(file: &str) -> CodingTurn {
    CodingTurn {
        reply: format!(
            "计划漏了\n<route>plan</route>\n<impact-gap>\n- 文件：{file}\n- 为什么必须改：接口在这\n- 证据：{file}:9\n</impact-gap>"
        ),
        changed: s(&["src/a.rs", file]),
        reviews: 0,
    }
}

fn pass_testing() -> TestingTurn {
    TestingTurn::Pass {
        acceptance_done: s(&["x=1 → 2"]),
        reviews: 1,
    }
}

fn cfg<'a>(store: &'a StateStore, repo: &'a Path, wl: &[&str], criteria: &[&str]) -> ChangeRun<'a> {
    // 基线 sha 从 init_git 落的文件读（store() 已初始化真 git 仓）；读不到（异常）
    // 才回退占位——diff_stats 会如实报错。
    let baseline_sha = std::fs::read_to_string(repo.join(".git/head-sha"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "base123".to_string());
    let leaked: &'a str = Box::leak(baseline_sha.into_boxed_str());
    ChangeRun {
        store,
        repo,
        change_id: "change-20261020-x",
        baseline_sha: leaked,
        criteria: s(criteria),
        whitelist: s(wl),
        usage: "1,000 tokens（in 900 / out 100）".into(),
        run_id: "run-1".into(),
        wall_secs: 12,
    }
}

/// 把 dir 初始化成真 git 仓（引擎审计修复 #7：Done 路径的 diff_stats 不再吞失败，
/// 非 git 仓的测试环境必须升级为真仓才能走到报告）。返回基线 sha。
fn init_git(dir: &Path) -> String {
    std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    run(&["init"]);
    run(&["add", "-A"]);
    run(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "init",
    ]);
    let head = run(&["rev-parse", "HEAD"]);
    String::from_utf8_lossy(&head.stdout).trim().to_string()
}

#[test]
fn happy_path_plan_coding_testing_report() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done);
    assert_eq!(out.report.roundtrips, 0);
    assert_eq!(out.report.review_rounds, 2, "coding 与 testing 各一轮审查要计入报告");
    assert_eq!(
        session.log,
        vec!["plan(rt=0)", "coding(rt=0)", "testing"],
        "顺序必须是 plan → coding → testing"
    );
    assert!(out.report.render().contains("验收对账: 通过"));
}

#[test]
fn phase_changes_and_fix_rounds_are_logged_as_milestones() {
    // 错实现形态：①阶段转换不落 `phase.change`（或缺 from/to）
    // ②计划修复轮不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Fail { issues: s(&["补验收标准"]) });
    session.fix_plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui).unwrap();
    });

    // 阶段转换：plan → coding → testing → done（三次转换各一条）
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert_eq!(phases.len(), 3, "三次阶段转换应各落一条：{text}");
    assert!(
        phases[0].contains("\"from\":\"plan\"") && phases[0].contains("\"to\":\"coding\""),
        "第一次转换：plan → coding：{}",
        phases[0]
    );
    assert!(
        phases[1].contains("\"from\":\"coding\"") && phases[1].contains("\"to\":\"testing\""),
        "第二次转换：coding → testing：{}",
        phases[1]
    );
    assert!(
        phases[2].contains("\"from\":\"testing\"") && phases[2].contains("\"to\":\"done\""),
        "第三次转换：testing → done：{}",
        phases[2]
    );
    assert!(
        phases[0].contains("\"target\":\"devforge::change::orchestrator\""),
        "里程碑的 target 必须是发出它的模块：{}",
        phases[0]
    );
    // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
    for line in text.lines().filter(|l| l.contains("\"event\":")) {
        crate::observability::test_support::assert_milestone_shape(line);
    }

    // 计划修复轮：一对 fix.start / fix.end（含轮次）
    let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
    let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
    assert_eq!(starts.len(), 1, "计划修复轮应落一条 fix.start：{text}");
    assert_eq!(ends.len(), 1, "计划修复轮应落一条 fix.end：{text}");
    assert!(starts[0].contains("\"round\":1"), "fix.start 要带修复轮次：{}", starts[0]);
    assert!(ends[0].contains("\"round\":1"), "fix.end 要带修复轮次：{}", ends[0]);
}

#[test]
fn the_test_to_coding_reinjection_is_logged_as_a_fix_pair() {
    // 错实现形态：测试回注 coding 不落 fix.start/fix.end（或缺回注轮次）
    // → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding { failures: s(&["断言失败"]) });

    let mut ui = NoUser;
    let text = crate::observability::test_support::capture_jsonl(|| {
        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
            .unwrap_err();
        assert!(err.to_string().contains("已回注 coding"), "{err}");
    });
    let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
    let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
    assert_eq!(starts.len(), 1, "回注应落一条 fix.start：{text}");
    assert_eq!(ends.len(), 1, "回注应落一条 fix.end：{text}");
    assert!(starts[0].contains("\"round\":1"), "回注轮次要带上：{}", starts[0]);
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert!(
        phases.iter().any(|l| l.contains("\"to\":\"coding\"")),
        "回注同时要把阶段推到 coding：{text}"
    );
}

#[test]
fn coding_routes_back_to_plan_then_reenters_coding() {
    // 审计要求：单测重现「coding → route plan → 改边界 → 重进 coding」全回路
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(declared_gap());
    session.amends.push_back(PlanTurn::Pass);
    // 补完边界后重进 coding：这次改动全在白名单里
    session.codings.push_back(CodingTurn {
        reply: "补完了".into(),
        changed: s(&["src/a.rs", "src/extra.rs"]),
        reviews: 0,
    });
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done);
    assert_eq!(out.state.roundtrips, 1, "往返计数必须记下来");
    assert_eq!(
        session.log,
        vec![
            "plan(rt=0)",
            "coding(rt=0)",
            "amend(rt=1,gaps=1)",
            "coding(rt=1)",
            "testing"
        ],
        "补完边界必须重进 coding"
    );
    // 断点里往返计数已落盘
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.roundtrips, 1);
    // 往返历史已写进目标仓（T013）
    let hist = std::fs::read_to_string(crate::change::history::history_path(
        tmp.path(),
        "change-20261020-x",
    ))
    .unwrap();
    assert!(hist.contains("src/extra.rs"), "history 应记下越界文件：{hist}");
}

#[test]
fn out_of_boundary_without_declaration_fails_and_stays_coding() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(CodingTurn {
        reply: "顺手改的".into(),
        changed: s(&["src/a.rs", "src/rogue.rs"]),
        reviews: 0,
    });

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    assert!(err.to_string().contains("范围膨胀"), "{err}");
    assert!(err.is_quality_failure());
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.phase, ChangePhase::Coding, "不能把失败当推进");
    assert_eq!(saved.roundtrips, 0);
}

#[test]
fn fourth_roundtrip_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    // 连续四次「越界+申报」→ 第 4 次（roundtrips 3→4）必须报错；
    // 每次都报不同的文件，否则上一轮补进白名单后就不再越界了
    for i in 0..4 {
        session
            .codings
            .push_back(declared_gap_file(&format!("src/extra{i}.rs")));
    }
    for _ in 0..3 {
        session.amends.push_back(PlanTurn::Pass);
    }

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    assert!(err.to_string().contains("上限 3"), "{err}");
    assert!(err.is_quality_failure(), "往返超限不该自动重启");
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.roundtrips, 3, "停在第四次之前");
}

/// 复验 #9 回归：amend_plan 失败（Err 传播）时 roundtrip 计数必须已落盘——
/// 旧实现只有 Pass 分支 save，Err 路径重启后计数回退、上限可被反复绕过。
#[test]
fn roundtrip_count_survives_amend_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(declared_gap_file("src/extra0.rs"));
    session
        .amend_errors
        .push_back("amend 实例基础设施故障".into());

    let mut ui = NoUser;
    let _ = run(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap_err();
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(
        saved.roundtrips, 1,
        "amend 失败时 roundtrip 计数必须已保存（先落盘再 amend）"
    );
}

#[test]
fn resume_from_checkpoint_skips_plan_and_keeps_roundtrips() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 预置断点：plan 已完成、coding 阶段、已往返 2 次
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Coding;
    seeded.bump_roundtrip();
    seeded.bump_roundtrip();
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(out.state.roundtrips, 2, "续跑不得清零往返计数");
    assert_eq!(out.report.roundtrips, 2);
    assert!(
        !session.log.iter().any(|l| l.starts_with("plan")),
        "plan 已完成，续跑不该重跑：{:?}",
        session.log
    );
}

#[test]
fn the_failure_context_reaches_the_coding_session() {
    // 错实现形态：编排层不把断点里的上下文传给 coding 会话（送达面断链）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    // 预置断点：coding 阶段 + 一份失败上下文
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Coding;
    seeded.set_test_failure(1, vec!["tests/test_a.py:12 断言失败".to_string()]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui).unwrap();
    assert_eq!(
        session.seen_failures,
        vec![Some(TestFailureContext {
            round: 1,
            issues: vec!["tests/test_a.py:12 断言失败".to_string()],
        })],
        "coding 会话必须收到断点里的失败上下文（送达面）"
    );
}

#[test]
fn testing_failure_routes_back_and_is_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&["tests/test_a.py:12 断言失败"]),
    });

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("回注 coding"), "{msg}");
    assert!(
        msg.contains("tests/test_a.py:12"),
        "回注必须带定位（失败明细逐条原文）：{msg}"
    );
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.phase, ChangePhase::Coding, "应回到 coding");
}

#[test]
fn reroute_writes_the_failure_context_into_the_checkpoint() {
    // 错实现形态：回注时不写失败上下文（下一轮 coding 拿不到明细）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&[
            "tests/change_ask_ring.rs:557 断言失败",
            "接口返回 500",
        ]),
    });

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("回注 coding"), "{msg}");
    assert!(
        msg.contains("tests/change_ask_ring.rs:557"),
        "失败原因文本要带明细原文（三出口同源）：{msg}"
    );
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.test_reroutes, 1, "回注计数要记 1");
    let ctx = saved.last_test_failure.expect("回注必须写入失败上下文");
    assert_eq!(ctx.round, 1, "轮次 = 第几轮回注");
    assert_eq!(
        ctx.issues,
        s(&[
            "tests/change_ask_ring.rs:557 断言失败",
            "接口返回 500"
        ]),
        "issues 要逐条原文（含 path:line）"
    );
}

#[test]
fn failure_context_survives_across_sessions() {
    // 错实现形态：上下文只活在本次调用的内存里 → 续跑时送达面断链，本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&["tests/change_ask_ring.rs:557 断言失败"]),
    });
    let mut ui = NoUser;
    let _ = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();

    // 新会话：同一黑板路径重新加载
    let st2 = StateStore::new(
        tmp.path(),
        StateKey::new("proj", "task", crate::change::state::CHANGE_SOP),
    );
    let resumed = load(&st2, "change-20261020-x", "base123");
    let ctx = resumed
        .last_test_failure
        .expect("跨会话必须保留失败上下文");
    assert_eq!(ctx.round, 1, "轮次不变");
    assert_eq!(
        ctx.issues,
        s(&["tests/change_ask_ring.rs:557 断言失败"]),
        "明细逐字不变"
    );
}

#[test]
fn coding_success_clears_the_failure_context() {
    // 错实现形态：coding 过门后不消费上下文（过期失败被继续喂给下一轮）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Coding;
    seeded.set_test_failure(1, vec!["tests/change_ask_ring.rs:557 断言失败".to_string()]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.codings.push_back(in_boundary());
    // testing 走 RoutePlan（不改上下文）→ 只观测 coding 过门后的清除
    session.testings.push_back(TestingTurn::RoutePlan {
        failures: s(&["计划问题"]),
    });
    let mut ui = NoUser;
    let _ = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();

    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(
        saved.last_test_failure, None,
        "coding 过门后上下文应被消费清空"
    );
}

#[test]
fn second_reroute_overwrites_the_failure_context() {
    // 错实现形态：新一次回注把 issues 追加到旧列表 → 已修好的旧失败又被喂回去
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.test_reroutes = 1;
    seeded.set_test_failure(1, vec!["旧失败".to_string()]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&["新失败"]),
    });
    let mut ui = NoUser;
    let _ = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();

    let saved = load(&st, "change-20261020-x", "base123");
    let ctx = saved.last_test_failure.expect("应有上下文");
    assert_eq!(ctx.round, 2, "轮次 = 第几轮回注");
    assert_eq!(ctx.issues, s(&["新失败"]), "新 issues 覆盖旧的，不追加");
}

/// 引擎审计修复 #5 回归：testing→plan 回退必须把 issues 写进断点并计入修复额度——
/// 错实现形态：issues 丢弃 + 无 cap → resume 走 plan_kickoff 重新开局，
/// Plan→Coding→Testing→Plan 无界循环。
#[test]
fn testing_route_plan_persists_issues_and_counts_toward_fix_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(TestingTurn::RoutePlan {
        failures: s(&["验收标准无法落地：计划承诺了测不了的行为"]),
    });
    let mut ui = NoUser;
    let _ = run(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap_err();

    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.phase, ChangePhase::Plan, "回退后应回到 Plan 阶段");
    assert_eq!(
        saved.last_plan_issues,
        s(&["验收标准无法落地：计划承诺了测不了的行为"]),
        "testing→plan 的 issues 必须进断点（resume 走 fix_plan 的依据）"
    );
    assert_eq!(
        saved.plan_fix_rounds, 1,
        "回退计入计划修复额度（MAX_PLAN_FIXES cap 生效的基础）"
    );
}

/// 引擎审计修复 #5 回归：testing→plan 回退后的 resume 必须走 fix_plan(issues)，
/// 不得重新 plan_kickoff（丢弃上下文重开局）。
#[test]
fn resume_after_testing_route_plan_goes_to_fix_plan_not_kickoff() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Plan;
    seeded.plan_fix_rounds = 1;
    seeded.last_plan_issues = s(&["验收标准无法落地"]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.fix_plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    let out = run(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done);
    assert!(
        session.log[0].starts_with("fix(rt=1,issues=1)"),
        "resume 首步必须是 fix_plan 且带上 issues：{:?}",
        session.log
    );
    assert!(
        !session.log.iter().any(|l| l.starts_with("plan(rt=0)")),
        "不得重新 plan_kickoff：{:?}",
        session.log
    );
}

#[test]
fn testing_pass_clears_the_failure_context() {
    // 错实现形态：run 到 done 不清上下文（收尾留过期状态）→ 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.set_test_failure(1, vec!["tests/change_ask_ring.rs:557 断言失败".to_string()]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done);
    assert_eq!(out.state.last_test_failure, None, "终态清：run 结束不留上下文");
}

#[test]
fn reroute_over_the_limit_keeps_the_failure_context_untouched() {
    // 错实现形态：超限分支也写/也清上下文 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.test_reroutes = MAX_TEST_REROUTES;
    seeded.set_test_failure(MAX_TEST_REROUTES, vec!["旧失败".to_string()]);
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&["新失败"]),
    });
    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    assert!(err.to_string().contains("上限 3"), "{err}");
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.test_reroutes, MAX_TEST_REROUTES + 1);
    assert_eq!(
        saved.last_test_failure,
        Some(TestFailureContext {
            round: MAX_TEST_REROUTES,
            issues: vec!["旧失败".to_string()],
        }),
        "超限分支既不写也不清：超限前的上下文原样保留"
    );
}

#[test]
fn mock_chain_produces_report_and_done_state() {
    // 接口级（进程内）全链：plan→coding→testing→report，不碰真 pi
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());

    let mut ui = NoUser;
    let mut out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(out.state.phase, ChangePhase::Done);

    let finalized = crate::server::change_engine::finalize(
        &mut out,
        &tmp.path().join("sessions"),
        tmp.path(),
        std::time::Instant::now(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&finalized.report_path).unwrap();
    assert!(text.contains("验收对账: 通过"), "{text}");
    assert!(text.contains("用量:"), "{text}");
    // 断点也落盘：重新读取仍是 done
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.phase, ChangePhase::Done);
}

#[test]
fn test_reroutes_are_bounded_across_sessions() {
    // 回注次数记在断点里：跳会话累计，不会被重启清零
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut seeded = ChangeState::new("change-20261020-x", "base123");
    seeded.phase = ChangePhase::Testing;
    seeded.test_reroutes = MAX_TEST_REROUTES;
    save(&st, &seeded).unwrap();

    let mut session = Scripted::new();
    session.testings.push_back(TestingTurn::RouteCoding {
        failures: s(&["回归红了"]),
    });

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("上限 3"), "{msg}");
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.test_reroutes, MAX_TEST_REROUTES + 1);
    assert_eq!(
        saved.last_test_failure, None,
        "超限前没有上下文就仍为「无上下文」"
    );
}

#[test]
fn acceptance_missing_one_item_fails_run() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(TestingTurn::Pass {
        acceptance_done: s(&["x=1 → 2"]),
        reviews: 0,
    });

    let mut ui = NoUser;
    let err = run(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2", "空输入报错"]),
        &mut session,
        &mut ui,
    )
    .unwrap_err();
    assert!(err.to_string().contains("缺 1 条"), "{err}");
    assert!(err.to_string().contains("空输入报错"), "{err}");
}

#[test]
fn plan_reviews_are_counted_in_report() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plan_review_count = 1;
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap();
    assert_eq!(
        out.report.review_rounds, 3,
        "plan + coding + testing 三轮审查都要计入报告"
    );
}

#[test]
fn red_baseline_stops_before_coding_agent_work() {
    // 会话层在发 kickoff 前就拦（基线红），编排层直接失败、不回 plan、不重试
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.coding_error = Some("基线不是绿的（exit_code=1，命令：cargo test）——不许在红基线上动代码".into());

    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui)
        .unwrap_err();
    assert!(err.to_string().contains("基线不是绿的"), "{err}");
    assert_eq!(session.log, vec!["plan(rt=0)", "coding(rt=0)"], "不得回 plan、不得重试");
    let saved = load(&st, "change-20261020-x", "base123");
    assert_eq!(saved.phase, ChangePhase::Coding, "停在 coding 入口，等人处理");
}

#[test]
fn checkpoint_path_is_change_state_on_blackboard() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session, &mut ui).unwrap();
    let path = st.path();
    assert!(
        path.ends_with("proj/task/change/state.json"),
        "change 断点必须按 SOP 隔离落在黑板：{}",
        path.display()
    );
    assert!(path.is_file());
}

#[test]
fn plan_rejection_is_quality_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plans.push_back(PlanTurn::Fail {
        issues: vec!["影响面缺调用方".into()],
    });
    // 修复上限 3 轮：连续 4 次 FAIL 才 Quality
    for _ in 0..MAX_PLAN_FIXES {
        session.fix_plans.push_back(PlanTurn::Fail {
            issues: vec!["仍缺".into()],
        });
    }
    let mut ui = NoUser;
    let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x"]), &mut session, &mut ui)
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("连续 4 轮未通过"), "{msg}");
    assert!(err.is_quality_failure());
    assert_eq!(
        session
            .log
            .iter()
            .filter(|l| l.starts_with("fix("))
            .count(),
        MAX_PLAN_FIXES,
        "修复循环恰好 3 轮：{:?}",
        session.log
    );
}

#[test]
fn plan_fix_loop_recovers_after_one_rejection() {
    // 打回一次 → 回注重写重审 → PASS → 正常进 coding（对齐 MVP 规划循环）
    let tmp = tempfile::tempdir().unwrap();
    let st = store(tmp.path());
    let mut session = Scripted::new();
    session.plan_review_count = 1;
    session.plans.push_back(PlanTurn::Fail {
        issues: vec!["path:line 要全路径".into()],
    });
    session.fix_plans.push_back(PlanTurn::Pass);
    session.codings.push_back(in_boundary());
    session.testings.push_back(pass_testing());
    let mut ui = NoUser;
    let out = run(
        cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]),
        &mut session,
        &mut ui,
    )
    .unwrap();
    assert_eq!(
        session.log[0],
        "plan(rt=0)",
        "首跑是 plan，第二跳才是 fix：{:?}",
        session.log
    );
    assert!(session.log[1].starts_with("fix(rt=1"), "{log:?}", log = session.log);
    assert_eq!(out.report.review_rounds, 4, "plan 段两轮 + coding + testing");
}
