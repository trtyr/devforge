//! coding/testing 运行时与审查协议重试测试（ARC-R6 自 session.rs mod tests 平移，纯移动）。

use super::*;

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
        plan_review_instances: 0,
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
fn coding_gates_and_review_open_are_logged_as_milestones() {
    // 错实现形态：①三道机械门不落 gate.*（或缺判定结论）
    // ②审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let f = fixture(true);
    coding_ready(&f);
    let agent = Agent::new(&[
        "基线绿了\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let review = ReviewFactory::new(&[PLAN_PASS]);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(Agent::new(&[])),
        plan_review: Box::new(ReviewFactory::new(&[])),
        coding_agent: Box::new(agent),
        coding_review: Box::new(review),
        test_agent: Box::new(Agent::new(&[])),
        test_review: Box::new(ReviewFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    let text = crate::observability::test_support::capture_jsonl(|| {
        session.run_coding(&mut NoUser, 0, None).unwrap();
    });

    // 三道机械门各一条（含判定结论）
    for (event, needle) in [
        ("gate.baseline", "\"passed\":true"),
        ("gate.self_check", "\"passed\":true"),
        ("gate.step_commits", "\"passed\":true"),
    ] {
        let lines: Vec<&str> = text.lines().filter(|l| l.contains(event)).collect();
        assert_eq!(lines.len(), 1, "{event} 应落一条：{text}");
        assert!(lines[0].contains(needle), "{event} 要带判定结论：{}", lines[0]);
    }
    let steps_line = text
        .lines()
        .find(|l| l.contains("gate.step_commits"))
        .expect("步提交对账应落一条");
    assert!(
        steps_line.contains("\"steps\":1"),
        "步提交对账要带步骤数：{steps_line}"
    );

    // 审查开庭：恰好一条，含角色名与轮次
    let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
    assert_eq!(opens.len(), 1, "一次审查开庭应恰好一条：{text}");
    assert!(
        opens[0].contains("\"role\":\"change-coding-review\""),
        "开庭要带角色名：{}",
        opens[0]
    );
    assert!(opens[0].contains("\"round\":1"), "开庭要带轮次：{}", opens[0]);
    assert!(
        opens[0].contains("\"target\":\"devforge::change::session\""),
        "里程碑的 target 必须是发出它的模块：{}",
        opens[0]
    );
    // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
    for line in text.lines().filter(|l| l.contains("\"event\":")) {
        crate::observability::test_support::assert_milestone_shape(line);
    }
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
    // 红证据也要 fresh（mtime 晚于本轮开始）才能走到「基线红」分支而非残留拒收
    std::fs::File::options()
        .write(true)
        .open(f.ctx.blackboard.join("evidence/baseline-test.log"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
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
        plan_review_instances: 0,
    };
    let text = crate::observability::test_support::capture_jsonl(|| {
        let err = session.run_coding(&mut NoUser, 0, None).unwrap_err();
        assert!(err.to_string().contains("基线不是绿的"), "{err}");
    });
    assert_eq!(seen.borrow().len(), 1, "红基线后不得再叫 agent 去改代码");

    // 红基线也要落一条门禁里程碑：判定结论 + **退出码按字段带出**（计划 §5「红时含退出码」）
    let gates: Vec<&str> = text.lines().filter(|l| l.contains("gate.baseline")).collect();
    assert_eq!(gates.len(), 1, "基线门应落一条：{text}");
    assert!(
        gates[0].contains("\"passed\":false"),
        "判定结论要落红：{}",
        gates[0]
    );
    assert!(
        gates[0].contains("\"exit_code\":1"),
        "红基线要把退出码按字段带出来（不能只埋在错误文本里）：{}",
        gates[0]
    );
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
        plan_review_instances: 0,
    };
    let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
    assert!(turn.reply.contains("<route>plan</route>"), "{}", turn.reply);
    assert_eq!(turn.reviews, 0, "申报轮不过 coding review");
}

#[test]
fn coding_review_failure_returns_issues_instead_of_failing_the_stage() {
    // change #4：编码审查 FAIL 不再让 run_coding 直接 Err(Quality)——明细随 turn 交回，
    // 由编排层走有界回注循环（MAX_CODING_FIXES）。这里钉「会话层交得出明细」。
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
        plan_review_instances: 0,
    };
    let turn = session.run_coding(&mut NoUser, 0, None).unwrap();
    assert_eq!(turn.reviews, 1, "审查轮次照实计入");
    assert!(
        turn.review_issues.iter().any(|s| s.contains("src/a.rs")),
        "审查明细必须随 turn 交回：{:?}",
        turn.review_issues
    );
}

/// 引擎审计修复 #4：回归证据必须 fresh（mtime ≥ 本轮开始）——fixture 手写的
/// regression.log 要刷未来 mtime，否则被当「上一轮残留」拒收。
fn write_fresh_regression_log(f: &Fixture) {
    std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
    let path = f.ctx.blackboard.join("evidence/regression.log");
    std::fs::write(&path, "command: cargo test\nexit_code: 0\n").unwrap();
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::File::open(&path)
        .unwrap()
        .set_modified(future)
        .unwrap();
}

#[test]
fn testing_review_is_invoked_and_can_fail() {
    let f = fixture(true);
    std::fs::create_dir_all(f.ctx.blackboard.join("testing")).unwrap();
    write_fresh_regression_log(&f);
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
        plan_review_instances: 0,
    };
    let err = session.run_testing(&mut NoUser).unwrap_err();
    assert!(err.to_string().contains("变更测试审查未通过"), "{err}");
}

#[test]
fn testing_pass_reads_acceptance_table() {
    let f = fixture(true);
    std::fs::create_dir_all(f.ctx.blackboard.join("testing")).unwrap();
    write_fresh_regression_log(&f);
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
    write_fresh_regression_log(&f);
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
        plan_review_instances: 0,
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

/// plan 轮：审查发生过一次违例重审 → 报告里的「审查轮次」计入这次重审（实际开过的实例数）。
#[test]
fn plan_review_instances_count_the_retry() {
    // 错实现形态：审查轮次恒为 1（违例重审被藏进「1 轮」）→ 本用例必须变红
    let f = fixture(false);
    let (review, _opened) =
        ScriptedResultFactory::new(vec![ok("我忘了输出协议块"), ok(PLAN_PASS)]);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(Agent::new(&["<stage-complete>1</stage-complete>"])),
        plan_review: Box::new(review),
        coding_agent: Box::new(Agent::new(&[])),
        coding_review: Box::new(ReviewFactory::new(&[])),
        test_agent: Box::new(Agent::new(&[])),
        test_review: Box::new(ReviewFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    let turn = session.run_plan(&mut NoUser, 0).unwrap();
    assert_eq!(turn, PlanTurn::Pass);
    assert_eq!(
        session.plan_reviews(),
        2,
        "审查轮次要计入违例重审（实际开过的实例数，不是恒为 1）"
    );
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
    write_fresh_regression_log(&f);
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
        plan_review_instances: 0,
    };
    let turn = session.run_testing(&mut NoUser).unwrap();
    let TestingTurn::Pass { reviews, .. } = turn else {
        panic!("重审通过后 testing 轮应判 Pass，实际 {turn:?}");
    };
    assert_eq!(reviews, 2, "审查轮次要计入重审（实际开过的实例数）");
    assert_eq!(*opened.borrow(), 2, "违例后必须开第二个全新审查实例");
}
