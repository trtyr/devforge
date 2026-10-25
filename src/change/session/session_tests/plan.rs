//! plan 段会话测试（ARC-R6 自 session.rs mod tests 平移，纯移动）。

use super::*;

#[test]
fn marker_nudge_redelivers_once_then_accepts() {
    let mut calls = 0usize;
    let reply = require_marker_nudged("x", "干了一半没交卷".into(), |m| {
        calls += 1;
        assert!(m.contains("<stage-complete>N"), "追补话术要带标记样板：{m}");
        Ok("补交\n<stage-complete>3</stage-complete>".to_string())
    })
    .unwrap();
    assert_eq!(calls, 1, "第一次追补就该收到");
    assert!(reply.contains("<stage-complete>3"));
}

#[test]
fn marker_nudge_gives_up_after_three_redeliveries() {
    let mut calls = 0usize;
    let err = require_marker_nudged("x", "还是没有".into(), |_| {
        calls += 1;
        Ok("还是没有".to_string())
    })
    .unwrap_err();
    assert_eq!(calls, 3, "追补恰好三次");
    assert!(err.to_string().contains("连续 4 次"), "{err}");
    assert!(err.to_string().contains("已追补 3 次"), "{err}");
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
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
        plan_review_instances: 0,
    };
    match session.run_plan(&mut NoUser, 0).unwrap() {
        PlanTurn::Fail { issues } => assert!(issues.iter().any(|i| i.contains("调用方"))),
        other => panic!("应为 Fail，实际 {other:?}"),
    }
}

#[test]
fn plan_without_marker_is_quality_failure() {
    // F17 追补口径：无标记先追补 ×3（每次都发追补话术），第四次仍无标记才 Quality
    let f = fixture(false);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(Agent::new(&[
            "就是不发标记",
            "还是不发",
            "仍然不发",
            "追补三次后依旧不发",
        ])),
        plan_review: Box::new(ReviewFactory::new(&[])),
        coding_agent: Box::new(Agent::new(&[])),
        coding_review: Box::new(ReviewFactory::new(&[])),
        test_agent: Box::new(Agent::new(&[])),
        test_review: Box::new(ReviewFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    let err = session.run_plan(&mut NoUser, 0).unwrap_err();
    assert!(err.to_string().contains("stage-complete"), "{err}");
    assert!(err.to_string().contains("已追补 3 次"), "{err}");
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
        plan_review_instances: 0,
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
    // 只追加语义：base 不变，追加送达块 + 重入纪律（复验 #6 续编号要求）
    assert!(with.starts_with(&plain), "开工语本体不得被改写");
    assert!(
        with.contains(&crate::change::testing_gate::coding_failure_block(&ctx)),
        "送达块必须逐字在"
    );
    assert!(with.contains("重入纪律"), "回注必须带续编号要求");
}

#[test]
fn empty_issues_route_is_rejected_then_accepted_verbatim() {
    // 错实现形态：空箱被合成占位文案（「测试阶段判定失败（未给出 issues 明细）」）并当失败明细回注
    // → 本用例必须变红
    let f = fixture(true);
    let agent = Agent::new(&[
        "回归红了\n<route>coding</route>",
        "补齐了\n<route>coding</route>\n<issues>\n- tests/a_test.rs:10 断言失败\n- tests/b_test.rs:20 断言失败\n</issues>",
    ]);
    let seen = agent.seen.clone();
    let mut session = ProdChangeSession {
        plan_agent: Box::new(Agent::new(&[])),
        plan_review: Box::new(ReviewFactory::new(&[])),
        coding_agent: Box::new(Agent::new(&[])),
        coding_review: Box::new(ReviewFactory::new(&[])),
        test_agent: Box::new(agent),
        test_review: Box::new(ReviewFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    let turn = session.run_testing(&mut NoUser).unwrap();
    let TestingTurn::RouteCoding { failures } = turn else {
        panic!("补齐明细后应回注 coding，实际 {turn:?}");
    };
    assert_eq!(
        failures,
        vec![
            "tests/a_test.rs:10 断言失败".to_string(),
            "tests/b_test.rs:20 断言失败".to_string()
        ],
        "失败明细逐条原文（无占位文案、无引擎合成文案）"
    );
    // 被拒收的那一轮要追一条「请补齐失败明细后重发」
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2, "被拒收后要追一次");
    assert!(
        seen[1].contains("请补齐失败明细后重发"),
        "追补话术要带补齐要求：{}",
        seen[1]
    );
}

#[test]
fn repeated_empty_issues_routes_fail_fast() {
    // 错实现形态：空箱被合成占位文案当失败明细回注（不 Quality）→ 本用例必须变红
    let f = fixture(true);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(Agent::new(&[])),
        plan_review: Box::new(ReviewFactory::new(&[])),
        coding_agent: Box::new(Agent::new(&[])),
        coding_review: Box::new(ReviewFactory::new(&[])),
        test_agent: Box::new(Agent::new(&[
            "回归红了\n<route>coding</route>",
            "回归红了\n<route>coding</route>",
            "回归红了\n<route>coding</route>",
            "回归红了\n<route>coding</route>",
        ])),
        test_review: Box::new(ReviewFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    let err = session.run_testing(&mut NoUser).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("未给出 issues 明细"), "{msg}");
    assert!(msg.contains("4 次"), "错误文本要带连续计数: {msg}");
    assert!(
        !msg.contains("测试阶段判定失败（未给出 issues 明细）"),
        "全程不得出现占位文案: {msg}"
    );
}

