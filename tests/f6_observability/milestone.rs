//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, fixture::*, guards::*, panic::*, runlog::*, support::*};

#[test]
pub(super) fn the_change_orchestrator_logs_the_phase_change_and_the_fix_pairs() {
    // 错实现形态：①阶段转换不落 phase.change（或缺 from/to）②修复轮不落 fix.start/fix.end
    // （或缺轮次）→ 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let store = StateStore::new(
        dir.path().join("plans"),
        StateKey::new("p", "t", devforge::change::state::CHANGE_SOP),
    );
    let mut session = ScriptedChangeSession { plan_calls: 0 };
    // 台账根在目标仓外（2026-10-25 反转 D003）
    let change_dir = dir.path().join("plantree-changes");
    let run = ChangeRun {
        store: &store,
        repo: &dir.path().join("repo"),
        change_dir: &change_dir,
        change_id: "change-f6",
        baseline_sha: "deadbeef",
        criteria: vec![],
        whitelist: vec!["src/a.rs".into()],
        usage: String::new(),
        run_id: "run-f6".into(),
        wall_secs: 1,
    };
    {
        let err = run_change(run, &mut session, &mut NoUser)
            .expect_err("测试回注后本轮以 Quality 结束");
        assert!(err.is_quality_failure(), "测试回注是质量失败：{err}");
    }
    let text = cap.lines();

    // 阶段转换：plan → coding
    let phases = lines_with(&text, "phase.change");
    let to_coding = phases
        .iter()
        .find(|l| l.contains("\"to\":\"coding\"") && l.contains("\"from\":\"plan\""))
        .unwrap_or_else(|| panic!("应有 plan → coding 的阶段转换：{text:?}"));
    assert!(to_coding.contains("\"from\":\"plan\""), "{to_coding}");
    // 阶段转换：testing → coding（回注）
    let back = phases
        .iter()
        .find(|l| l.contains("\"from\":\"testing\""))
        .unwrap_or_else(|| panic!("应有 testing → coding 的阶段转换：{text:?}"));
    assert!(back.contains("\"to\":\"coding\""), "{back}");

    // 修复轮：计划审查打回后的重写 + 测试回注 coding（各一对，含轮次）
    let starts = lines_with(&text, "fix.start");
    let ends = lines_with(&text, "fix.end");
    assert_eq!(starts.len(), 2, "计划修复轮 + 测试回注各一对：{text:?}");
    assert_eq!(ends.len(), 2, "计划修复轮 + 测试回注各一对：{text:?}");
    assert!(
        starts.iter().all(|l| l.contains("\"round\":1")),
        "修复轮要带轮次：{text:?}"
    );
    assert!(
        starts
            .iter()
            .any(|l| l.contains("计划修复轮开始")),
        "应有计划修复轮的 fix.start：{text:?}"
    );
    assert!(
        starts
            .iter()
            .any(|l| l.contains("测试回注 coding 开始")),
        "应有测试回注的 fix.start：{text:?}"
    );
    // 里程碑形状：级别 INFO + 带 component
    // 只查本用例关心的三类事件——全局日志缓冲里还有别的用例（含 WARN/ERROR）的行，
    // 按 "event": 泛指会把它们算进来（2026-10-25 并行 5 跑红 2 的根因）。
    let mut ours: Vec<&str> = Vec::new();
    for ev in ["phase.change", "fix.start", "fix.end"] {
        ours.extend(lines_with(&text, ev));
    }
    for line in ours {
        assert!(line.contains("\"level\":\"INFO\""), "里程碑必须是 INFO：{line}");
        assert!(line.contains("\"component\":"), "里程碑要带 component：{line}");
    }
}

#[test]
pub(super) fn each_mvp_and_doc_loop_logs_its_own_fix_pair() {
    // 错实现形态：某个循环的打回/回注不落 fix.start/fix.end（或缺轮次）→ 本用例必须变红
    let mut all: Vec<String> = Vec::new();

    // MVP planning：判负回注
    {
        let cap = Capture::new();
        let plan = ScriptedAgent::new(&["好了\n<plan-complete>", "改好了\n<plan-complete>"]);
        let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_FAIL, PLAN_REVIEW_PASS]);
        devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
            .run(&mut NoUser)
            .expect("规划循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "planning 回注：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "planning 回注：{text:?}");
        assert!(
            lines_with(&text, "fix.start")[0].contains("\"round\":1"),
            "planning 回注要带轮次：{text:?}"
        );
        all.extend(text);
    }

    // MVP coding：打回重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&[
            "阶段 1 完成\n<stage-complete>1</stage-complete>",
            "修好了\n<stage-complete>1</stage-complete>",
        ]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("编码循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "coding 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "coding 打回：{text:?}");
        all.extend(text);
    }

    // MVP testing：打回重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&[
            "阶段 1 完成\n<stage-complete>1</stage-complete>",
            "修好了\n<stage-complete>1</stage-complete>",
        ]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::testing::TestingSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("测试循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "testing 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "testing 打回：{text:?}");
        all.extend(text);
    }

    // doc-from-zero：单元审查 FAIL → 回注重写
    {
        let cap = Capture::new();
        let dir = tempfile::tempdir().expect("临时目录");
        let store = StateStore::new(
            dir.path().join("plans"),
            StateKey::new("p", "t", "doc-from-zero"),
        );
        let agent = ScriptedAgent::new(&[
            "单元 1 写完\n<doc-complete>1</doc-complete>",
            "单元 1 改完\n<doc-complete>1</doc-complete>",
        ]);
        let factory = ScriptedFactory::new(&[DOC_REVIEW_FAIL, DOC_REVIEW_PASS]);
        devforge::sop::doc::run_pipeline(
            agent,
            std::sync::Arc::new(std::sync::Mutex::new(factory)),
            store,
            &mut NoUser,
            &dir.path().join("repo"),
            &["1".to_string()],
            "none",
            String::new(),
            None,
            None,
        )
        .expect("doc 单元循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "doc 回注：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "doc 回注：{text:?}");
        all.extend(text);
    }

    // doc-fix：审查打回 → 回注重写
    {
        let cap = Capture::new();
        let agent = ScriptedAgent::new(&["写好了", "改好了"]);
        let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
        devforge::sop::doc::fix::DocFixSopLoop::new(agent, &mut factory)
            .run(&mut NoUser, Path::new("/w"), "p", "t")
            .expect("doc-fix 循环应通过");
        let text = cap.lines();
        assert_eq!(lines_with(&text, "fix.start").len(), 1, "doc-fix 打回：{text:?}");
        assert_eq!(lines_with(&text, "fix.end").len(), 1, "doc-fix 打回：{text:?}");
        all.extend(text);
    }

    // 五个循环各一对
    assert_eq!(
        lines_with(&all, "fix.start").len(),
        5,
        "五个循环各落一条 fix.start：{all:?}"
    );
    assert_eq!(
        lines_with(&all, "fix.end").len(),
        5,
        "五个循环各落一条 fix.end：{all:?}"
    );
}

#[test]
pub(super) fn the_mvp_planning_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次，首审不是 1）→ 本用例必须变红
    let cap = Capture::new();
    let plan = ScriptedAgent::new(&["好了\n<plan-complete>"]);
    let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_PASS]);
    devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
        .run(&mut NoUser)
        .expect("规划循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"plan-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::planning\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
pub(super) fn the_mvp_coding_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("编码循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"coding-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::coding\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
pub(super) fn the_mvp_testing_review_open_carries_the_role_and_the_round() {
    // 错实现形态：审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::testing::TestingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("测试循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"test-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::testing\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
pub(super) fn the_doc_from_zero_review_open_round_is_the_bump_review_count() {
    // 错实现形态：doc-from-zero 的开庭轮次不是 bump_review 的 1-based 计数（首审不是 1）
    // → 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let store = StateStore::new(
        dir.path().join("plans"),
        StateKey::new("p", "t", "doc-from-zero"),
    );
    let agent = ScriptedAgent::new(&["单元 1 写完\n<doc-complete>1</doc-complete>"]);
    let factory = ScriptedFactory::new(&[DOC_REVIEW_PASS]);
    devforge::sop::doc::run_pipeline(
        agent,
        std::sync::Arc::new(std::sync::Mutex::new(factory)),
        store,
        &mut NoUser,
        &dir.path().join("repo"),
        &["1".to_string()],
        "none",
        String::new(),
        None,
        None,
    )
    .expect("doc 单元循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"doc-review\""), "{opens:?}");
    assert!(
        opens[0].contains("\"round\":1"),
        "轮次 = bump_review 的 1-based 计数（首审 = 1）：{opens:?}"
    );
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc::runner\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

#[test]
pub(super) fn the_doc_fix_review_open_round_equals_the_review_rounds_counter() {
    // 错实现形态：doc-fix 的开庭轮次写成 rounds + 1（首审记成 2）→ 本用例必须变红
    let cap = Capture::new();
    let agent = ScriptedAgent::new(&["写好了", "改好了"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_FAIL, STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::doc::fix::DocFixSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("doc-fix 循环应通过");
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 2, "两轮审查各一条：{text:?}");
    assert!(opens[0].contains("\"role\":\"doc-review\""), "{opens:?}");
    assert!(opens[0].contains("\"round\":1"), "首审 = 1：{opens:?}");
    assert!(opens[1].contains("\"round\":2"), "次审 = 2：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc::fix\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

