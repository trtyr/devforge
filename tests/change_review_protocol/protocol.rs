//! （ARC-R6 自 change_review_protocol.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{guards::*, support::*};

/// 验收 1 · plan 审查第一次缺 `<stage-verdict>`、第二次 PASS → 开过两个实例、plan 轮判 Pass。

#[test]
pub(super) fn a1_plan_review_violation_retries_with_a_new_instance_and_passes() {
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
pub(super) fn a2_coding_review_fail_without_issues_is_a_violation_not_a_verdict() {
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
pub(super) fn a3_test_review_fail_without_issues_is_a_violation_not_a_verdict() {
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
pub(super) fn a4_four_consecutive_violations_are_a_quality_failure() {
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
pub(super) fn a5_empty_review_reply_is_an_instance_failure_not_a_verdict() {
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
pub(super) fn a6_four_consecutive_empty_replies_are_an_infrastructure_failure() {
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
pub(super) fn a7_channel_error_is_retried_with_a_new_instance() {
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

/// 验收 8 · test 审查 FAIL + 非空 issues（真判负）→ 仍只开一个实例（不重审），
/// 但不再判死 run：明细逐字进回注 coding 的失败清单（change #4）。
#[test]
pub(super) fn a8_test_review_fail_with_issues_reroutes_to_coding_verbatim() {
    // 错实现形态：把真判负也当成「换实例重审」（判负通道被改软）→ 本用例必须变红；
    // 另一个错形态：FAIL 直接判死 run（旧行为）——现在应该走回注循环。
    let f = fixture();
    let test_agent = ScriptedAgent::new(&["测完了\n<stage-complete>1</stage-complete>"]);
    let test_review = ScriptedReviewFactory::new(vec![ScriptedReply::text(FAIL_WITH_ISSUES)]);
    let opened = test_review.clone();
    let mut session = testing_only(f.ctx.clone(), test_agent, test_review);

    match session.run_testing(&mut NoUser).unwrap() {
        TestingTurn::RouteCoding { failures } => {
            assert!(
                failures
                    .iter()
                    .any(|s| s.contains("tests/change_review_protocol.rs:1 断言失败")),
                "回注清单要逐字含 issues 原文：{failures:?}"
            );
        }
        other => panic!("真判负应回注 coding，实际：{other:?}"),
    }
    assert_eq!(opened.opened(), 1, "真判负不重审（判负通道不变）");
}

// ---------------- 验收 9：MVP coding 审查 FAIL 空箱 → 换新实例，不回注 ----------------

/// 验收 9 · MVP coding 审查 FAIL 空箱 → 开第二个全新实例重审，不回注 coding agent，issues_total 不累计。
#[test]
pub(super) fn a9_mvp_coding_fail_without_issues_is_not_injected_into_the_agent() {
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
pub(super) fn a10_mvp_coding_repeated_fail_without_issues_is_a_quality_failure() {
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
pub(super) fn a11_parse_verdict_fail_without_issues_is_a_protocol_violation() {
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
pub(super) fn a12_mvp_plan_review_missing_verdict_retries_without_injecting() {
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
pub(super) fn a13_mvp_plan_review_repeated_violations_are_a_quality_failure() {
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
pub(super) fn a14_mvp_plan_review_fail_with_issues_is_still_injected() {
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
pub(super) fn a15_empty_issues_route_is_rejected_and_nudged() {
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
pub(super) fn a16_four_consecutive_empty_routes_are_a_quality_failure() {
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
pub(super) fn a17_empty_then_issues_route_carries_the_details_verbatim() {
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
pub(super) fn a18_prose_route_mention_in_the_middle_is_not_a_route() {
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
pub(super) fn a19_tail_route_with_issues_is_a_route() {
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
pub(super) fn a20_tail_route_with_impact_gap_is_still_a_boundary_declaration() {
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
        BoundaryVerdict::RoutePlan { gaps, .. } => {
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
pub(super) fn a21_prose_route_mention_in_the_middle_is_not_a_boundary_declaration() {
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
pub(super) fn a22_parse_review_report_fail_without_issues_is_a_violation() {
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
pub(super) fn a23_parse_verdict_fail_without_issues_is_not_an_empty_fail_list() {
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
pub(super) fn a24_change_agent_prompts_carry_the_tail_protocol_contract() {
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

