//! （ARC-R6 自 change_failure_context.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{guards::*, support::*};

/// 验收 1 · 新建 `ChangeState` 无失败上下文；`coding_kickoff(ctx, None)` **逐字**等于改动前的开工语。

#[test]
pub(super) fn a1_new_state_has_no_context_and_none_kickoff_is_verbatim_unchanged() {
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
    //    （计划批准的协议契约一句除外：它已在基线提交里（c3 场 step 5 引入），
    //    故两侧都剥掉这一句再比——常量刷到新基线后，基线那版本来就含它）
    let kick = coding_kickoff(&ctx, None);
    assert!(
        kick.contains(KICKOFF_PROTOCOL_CLAUSE),
        "本场批准的协议契约必须在开工语里：{kick}"
    );
    assert_eq!(
        kick.replace(KICKOFF_PROTOCOL_CLAUSE, ""),
        baseline_coding_kickoff(&ctx).replace(KICKOFF_PROTOCOL_CLAUSE, ""),
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
    // 复验 #6 演进：送达块后允许追加【重入纪律】段——但基座+送达块必须是前缀（只能追加不许改写）
    let suffix = with
        [kick.len() + format!("\n\n{}", devforge::change::testing_gate::coding_failure_block(&context(1))).len()..]
        .trim();
    assert!(
        suffix.starts_with("【重入纪律】"),
        "送达块之后只允许追加重入纪律段：{suffix:?}"
    );
}

/// 从基线提交里取出**改动前**的开工语（逐字比对用）。
pub(super) fn baseline_coding_kickoff(ctx: &ChangeCtx) -> String {
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
pub(super) fn a2_reroute_writes_the_failure_context_into_the_checkpoint() {
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
pub(super) fn a3_the_failure_context_survives_across_sessions() {
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
pub(super) fn a4_coding_kickoff_carries_the_title_and_the_issues_verbatim() {
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
pub(super) fn a5_coding_kickoff_has_no_zero_information_boilerplate() {
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
pub(super) fn a6_route_back_message_to_coding_keeps_the_location() {
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
pub(super) fn a7_route_back_message_to_plan_keeps_the_location() {
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
pub(super) fn a8_coding_success_clears_the_failure_context() {
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
pub(super) fn a9_second_reroute_overwrites_the_failure_context() {
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
pub(super) fn a10_testing_pass_clears_the_failure_context() {
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
pub(super) fn a11_reroute_over_the_limit_neither_writes_nor_clears() {
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
pub(super) fn a12_old_checkpoint_without_the_failure_key_still_loads() {
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

// ---------------- 验收 13：空箱拒收（不合成占位文案） ----------------

/// 验收 13 · test agent 回 `<route>coding</route>` 且不带 `<issues>` → 引擎**拒收**该回注
/// （不产生 `RouteCoding`、不合成占位文案），连追 3 次仍不带 → 质量失败（错误文本含「未给出 issues 明细」）。
#[test]
pub(super) fn a13_empty_issues_route_is_rejected_without_a_placeholder() {
    // 错实现形态：空箱被合成占位文案并当失败明细回注 → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx_in(tmp.path());

    // 真 session 侧：test agent 只回路由、不给 issues（每次都一样 → 连续拒收到判死）
    let reply = "<route>coding</route>".to_string();
    let mut prod = ProdChangeSession {
        plan_agent: Box::new(FixedAgent::new("<stage-complete>1</stage-complete>".into())),
        plan_review: Box::new(NoReview),
        coding_agent: Box::new(FixedAgent::new("<stage-complete>1</stage-complete>".into())),
        coding_review: Box::new(NoReview),
        test_agent: Box::new(FixedAgent::new(reply)),
        test_review: Box::new(NoReview),
        ctx: ctx.clone(),
        plan_review_instances: 0,
    };
    let mut ui = NoUser;
    let err = prod
        .run_testing(&mut ui)
        .expect_err("空箱回注不得被当成 RouteCoding 收下");
    let msg = err.to_string();
    assert!(
        msg.contains("未给出 issues 明细"),
        "错误文本要说明缺的是 issues 明细: {msg}"
    );
    assert!(
        !msg.contains(PLACEHOLDER_ISSUE),
        "空箱不得被合成占位文案: {msg}"
    );
    assert!(
        err.is_quality_failure(),
        "连续空箱是质量失败（不自动重启）: {msg}"
    );
}

/// 只会照抄一句回复的假 agent。
pub(super) struct FixedAgent {
    reply: String,
}

impl FixedAgent {
    pub(super) fn new(reply: String) -> Self {
        Self { reply }
    }
}

impl PlanAgentSession for FixedAgent {
    fn converse(&mut self, _message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        Ok(self.reply.clone())
    }
}

/// 审查会话工厂：不产出任何报告（占位，避免误触真链路）。
pub(super) struct NoReview;

impl ReviewSessionFactory for NoReview {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        Ok(Box::new(NoReviewSession))
    }
}

pub(super) struct NoReviewSession;

impl ReviewSession for NoReviewSession {
    fn review(&mut self, _instruction: &str) -> Result<String> {
        Ok("<stage-verdict>PASS</stage-verdict>\n<next>advance</next>".into())
    }
}

// ---------------- 验收 14：决策台账 D025 ----------------

/// 验收 14 · 台账含 `## D025 ·` 一节，写明字段形态 / 写入点 / 清除三规则 / 续跑一致性；D018 带旁注。
#[test]
pub(super) fn a14_decision_ledger_registers_d025_with_the_clearing_rules() {
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

