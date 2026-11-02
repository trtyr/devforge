//! run_pipeline 行为测试（DR-14 流水线：重叠/fix 轮/断路器/里程碑/审计注入/协议重试）。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::sop::doc::mocks::{
    MockAgent, NoUser, PipeAgent, PipeReviewFactory, drive_gates, gate_pair, no_gates,
    pipe_factory, pipe_factory_with_submissions, state_path, submit_unit, submit_verdict,
};
use crate::sop::doc::prompts::{doc_kickoff, doc_review_instruction};
use crate::sop::doc::runner::run_pipeline;
use crate::sop::doc::state::done_units;

/// 行为类测试的 pipeline 调用（闸门/注入类用例用全参数 run_pipeline）
macro_rules! run_pipe {
    ($agent:expr, $f:expr, $tmp:expr, $units:expr) => {
        run_pipeline(
            $agent,
            Arc::new(Mutex::new($f)),
            state_path(&$tmp),
            &mut NoUser,
            $tmp.path(),
            &$units,
            "complete",
            String::new(),
            None,
            None,
        )
    };
}

#[test]
fn two_units_pass_and_checkpoint_written() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "写完 A\n<doc-complete>unit-a-01</doc-complete>",
        "写完 B\n<doc-complete>unit-b-02</doc-complete>",
    ]);
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>",
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let rep = run_pipe!(agent, f, tmp, units).unwrap();
    assert_eq!(rep.units_done, units);
    assert_eq!(rep.review_rounds, 2);
    let saved = state_path(&tmp).load();
    assert_eq!(done_units(saved.state().expect("断点应已落盘")), units);
}

#[test]
fn empty_review_reply_is_retried_with_fresh_instance() {
    // 2026-10-20 实测：doc-review 返回空回复（模型一字未出），旧行为把整个 run 判死
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory(&["", "<doc-verdict>PASS</doc-verdict>"]);
    let instructions = f.instructions.clone();
    let opens = f.opens.clone();
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(rep.units_done, vec!["unit-a-01"]);
    assert_eq!(instructions.lock().unwrap().len(), 2, "空回复应换实例重试一次");
    // 自审 major：重试契约必须观测 open() 次数——review 调两次不等于换了实例。
    assert_eq!(*opens.lock().unwrap(), 2, "空回复重试必须 open 新实例（恰好一次重试）");
}

#[test]
fn repeated_empty_reviews_fail_fast() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory(&["", "", "", ""]);
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("空回复"), "{msg}");
}

#[test]
fn missing_marker_is_bounded_not_infinite() {
    // 零上限的旧口径在这里就是无限空转：连续不发标记必须报错
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["想想", "再想想", "还在想", "就是不写"]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    let msg = format!("{err}");
    assert!(err.is_quality_failure(), "{msg}");
    assert!(msg.contains("未交卷"), "{msg}");
}

#[test]
fn wrong_unit_number_is_bounded_not_infinite() {
    // 自审 critical（2026-10-26）：错号重问分支不计上限 = 无界死循环（每圈真调一次 LLM）。
    // 修复后错号计入 nudges，同款上限兜底。
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "A\n<doc-complete>unit-zzz</doc-complete>",
        "B\n<doc-complete>unit-zzz</doc-complete>",
        "C\n<doc-complete>unit-zzz</doc-complete>",
        "D\n<doc-complete>unit-zzz</doc-complete>",
    ]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    assert!(
        err.is_quality_failure(),
        "错号空转是 agent 不配合 → Quality：{err}"
    );
    assert!(format!("{err}").contains("单元号"), "{err}");
}

#[test]
fn first_unit_overdue_warns_exactly_once() {        // DR-4：开跑超阈值仍无任何单元完成 → doc.first_unit_overdue 告警一次
    // （模拟假活：deadline 注入 0；agent 永不交卷）
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["想想", "再想想", "还在想", "就是不写"]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let log = crate::observability::test_support::capture_jsonl(|| {
        // mock 回合微秒级完成：不给时钟前进的机会就永远到不了 deadline
        std::thread::sleep(std::time::Duration::from_millis(5));
        let err = run_pipeline(
            agent,
            Arc::new(Mutex::new(f)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &["unit-a-01".to_string()],
            "complete",
            String::new(),
            None,
            Some(0),
        );
        assert!(err.is_err(), "agent 永不交卷应报错收场");
    });
    assert!(log.contains("doc.first_unit_overdue"), "应告警: {log}");
    assert_eq!(
        log.matches("doc.first_unit_overdue").count(),
        1,
        "只告警一次，不刷屏: {log}"
    );
}

#[test]
fn kickoff_carries_no_discipline_constants() {
    // AF-4（2026-10-24 拍板）：纪律常量上移 AGENTS.md（唯一权威），kickoff 定稿纯变量工单——
    // 副本漂移病根拔除：同一规则不再有两个生效层。
    let k = doc_kickoff(Path::new("/repo"), "unit-x", "complete", None);
    for banned in ["穿插推进", "批量写出", "30K tokens", "压轴最后写", "codegraph-first", "doc-contract"] {
        assert!(!k.contains(banned), "kickoff 不再携带纪律常量「{banned}」: {k}");
    }
    assert!(k.contains("AGENTS.md"), "要点名纪律唯一权威位置: {k}");
    assert!(
        k.contains("<doc-complete>unit-x</doc-complete>"),
        "完成协议提醒要在: {k}"
    );
    // 变量照旧：单元号/档位/仓库/产出目录都在（事实不是纪律）
    assert!(k.contains("unit-x") && k.contains("complete") && k.contains("/repo"));
    assert!(k.contains("docs/contexts"), "产出目录（路径事实）要在: {k}");
}

#[test]
fn kickoff_points_at_workbook_when_ready_falls_back_to_inventory_when_not() {
    // AF-1：作业本就绪 → 指针指向作业本，且不再带清单全量指针（上下文税不再交第二遍）
    let wb = Path::new("/repo/docs/contexts/_meta/units/unit-x.md");
    let k = doc_kickoff(Path::new("/repo"), "unit-x", "complete", Some(wb));
    assert!(k.contains("_meta/units/unit-x.md"), "要带作业本确切路径: {k}");
    assert!(k.contains("先读它"), "作业本指针要点名先读: {k}");
    assert!(
        !k.contains("inventory.json"),
        "作业本就绪时不再全量指针清单（34 单元交 34 遍上下文税的病根）: {k}"
    );
    // 降级：作业本缺位 → 回退清单指针（宁降级不炸）
    let d = doc_kickoff(Path::new("/repo"), "unit-x", "complete", None);
    assert!(d.contains("inventory.json"), "降级要有清单指针: {d}");
    assert!(!d.contains("_meta/units/"), "降级不带作业本路径: {d}");
}

#[test]
fn tool_submission_submits_without_text_marker() {
    // AF-5 正道：submit_unit 工具提交即交卷信号——正文可以没有任何文本标记
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::with_submissions(
        &["正文写完了，没有任何标记。"],
        vec![vec![submit_unit("unit-a-01")]],
    );
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(rep.units_done, vec!["unit-a-01".to_string()]);
}

#[test]
fn bad_tool_submission_counts_as_miss_and_nudge_names_the_tool() {
    // 坏参（空 unit）= 拒收重试不静默错：nudge 计数照走，话术点名 submit_unit；
    // 连续不交 = 质量失败（上限纪律不变）。
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::with_submissions(
        &["想想", "再想想", "还是不交", "就是不交"],
        vec![vec![submit_unit("")], vec![], vec![], vec![]],
    );
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let received = agent.received.clone();
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    assert!(err.is_quality_failure(), "连续不交卷 = 质量失败: {err}");
    let received = received.borrow();
    assert!(
        received.iter().any(|m| m.contains("submit_unit")),
        "nudge 话术要点名工具: {:?}",
        received
    );
}

#[test]
fn text_fallback_warns_once_and_still_settles() {
    // 降级路径：无工具提交、文本魔法块交卷 → 照常成功，但 WARN 恰一条（每单元一次）
    let out: std::sync::Arc<std::sync::Mutex<Option<usize>>> = Default::default();
    let captured = {
        let out = out.clone();
        crate::observability::test_support::capture_jsonl(move || {
            let tmp = tempfile::tempdir().unwrap();
            let agent = MockAgent::new(&["正文\n<doc-complete>unit-a-01</doc-complete>"]);
            let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
            let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
            *out.lock().unwrap() = Some(rep.units_done.len());
        })
    };
    assert_eq!(out.lock().unwrap().unwrap(), 1, "降级照常交卷成功");
    let warn_count = captured
        .lines()
        .filter(|l| l.contains("按文本协议解析交卷"))
        .count();
    assert_eq!(warn_count, 1, "降级 WARN 每单元恰一条: {captured}");
}

#[test]
fn review_instruction_puts_verdict_first() {
    // AF-5：交卷工具上位——裁决走 submit_verdict 参数（截断伤不到裁决），文本协议降级；
    // DR-12：不读码条款
    let ins = doc_review_instruction(std::path::Path::new("/tmp/doc-workspace"), "unit-x", 1);
    assert!(ins.contains("submit_verdict"), "要优先交卷工具: {ins}");
    assert!(ins.contains("工具参数"), "裁决走参数的说明要在: {ins}");
    assert!(ins.contains("<doc-verdict>"), "降级文本协议说明要在: {ins}");
    assert!(ins.contains("4000 字"), "要带叙述预算: {ins}");
    assert!(ins.contains("stage-verdict"), "双输出禁令要在: {ins}");
    assert!(ins.contains("不读代码"), "DR-12 不读码条款要在: {ins}");
}

#[test]
fn empty_reply_is_infra_not_quality() {
    // DR-8：空响应 = provider 侧失败 → System（可重试），非 Quality；不计入 doc-complete nudges
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["", "", "", ""]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    assert!(!err.is_quality_failure(), "空响应不得判质量失败: {err}");
    assert!(format!("{err}").contains("基础设施故障"), "{err}");
}

// DR-9 L3 glue 计时（engine.glue）在流水线路径尚未接线（TODO DR-10③），专项用例随串行 runner 退役

#[test]
fn pipeline_runs_units_with_overlapped_reviews() {
    // DR-14 lead=1：审查 a 在途期间，b 的 kickoff 必须先行（写审重叠）；
    // 串行退化 = 闸门死等 10s 后强放 + 时长断言失败
    let tmp = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let (agent, received) = PipeAgent::new(&[
        "A\n<doc-complete>unit-a-01</doc-complete>",
        "B\n<doc-complete>unit-b-02</doc-complete>",
    ]);
    let (tx1, rx1) = gate_pair();
    let received_w = received.clone();
    let factory = PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            [
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1]))),
        deadlines: Arc::new(Mutex::new(Vec::new())),
        submissions: Arc::new(Mutex::new(Default::default())),
    };
    drive_gates(
        vec![(
            Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
            2,
        )],
        vec![tx1],
    );
    let report = run_pipeline(
        agent,
        Arc::new(Mutex::new(factory)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &["unit-a-01".to_string(), "unit-b-02".to_string()],
        "complete",
        String::new(),
        None,
        None,
    )
    .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "写审应重叠（串行退化会死等闸门到死线）：{:?}",
        started.elapsed()
    );
    let got = received.lock().unwrap().join("|||");
    assert!(got.contains("unit-a-01"), "收到的消息: {got}");
    assert_eq!(report.units_done, vec!["unit-a-01", "unit-b-02"]);
}

#[test]
fn pipeline_fix_round_requeues_failed_unit() {
    // DR-14 lead=1 语义：a 写完即开审（在途），b 立刻 kickoff（写超前 1）；
    // a 的 FAIL 裁决在下一个空闲点（b 交卷后）以 fix 优先送达——fix 不打断在写单元
    let tmp = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let (agent, received) = PipeAgent::new(&[
        "W1\n<doc-complete>unit-a-01</doc-complete>",
        "W2\n<doc-complete>unit-b-02</doc-complete>",
        "FIX1\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let (tx1, rx1) = gate_pair();
    let (tx2, rx2) = gate_pair();
    let (tx3, rx3) = gate_pair();
    let instructions = Arc::new(Mutex::new(Vec::new()));
    let (received_w, instr_w2, instr_w3) = (received.clone(), instructions.clone(), instructions.clone());
    let factory = PipeReviewFactory {
        replies: Arc::new(Mutex::new(VecDeque::from(vec![
            "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 描述与行为不符\n</issues>"
                .to_string(),
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>".to_string(),
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>".to_string(),
        ]))),
        instructions: instructions.clone(),
        opens: Arc::new(Mutex::new(0)),
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1, rx2, rx3]))),
        deadlines: Arc::new(Mutex::new(Vec::new())),
        submissions: Arc::new(Mutex::new(Default::default())),
    };
    drive_gates(
        vec![
            (
                Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                2,
            ),
            (
                Box::new(move || instr_w2.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                2,
            ),
            (
                Box::new(move || instr_w3.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                3,
            ),
        ],
        vec![tx1, tx2, tx3],
    );
    let report = run_pipeline(
        agent,
        Arc::new(Mutex::new(factory)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &["unit-a-01".to_string(), "unit-b-02".to_string()],
        "complete",
        String::new(),
        None,
        None,
    )
    .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "写审应重叠：{:?}",
        started.elapsed()
    );
    let mut done = report.units_done.clone();
    done.sort();
    assert_eq!(
        done,
        vec!["unit-a-01", "unit-b-02"],
        "修复后的 unit 1 与 unit 2 都应过审（过审标记顺序随裁决到达序）"
    );
    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 3, "消息序应为 [kickoff a, kickoff b, fix a]: {msgs:?}");
    assert!(
        msgs[1].contains("unit-b-02"),
        "第 2 条应是 unit 2 的 kickoff（审 a 期间写 b）: {msgs:?}"
    );
    assert!(
        msgs[2].contains(crate::sop::doc::prompts::DOC_REMEDIATION_PREFIX),
        "FAIL 裁决在 b 交卷后的空闲点送达 fix: {msgs:?}"
    );
}

#[test]
fn pipeline_breaker_stops_after_two_consecutive_fails() {
    // DR-14 断路器：连续 2 FAIL → andon 熔断（Quality），不再无限重跑
    let tmp = tempfile::tempdir().unwrap();
    let (agent, _) = PipeAgent::new(&[
        "W1\n<doc-complete>unit-a-01</doc-complete>",
        "FIX1\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let factory = PipeReviewFactory {
        replies: Arc::new(Mutex::new(VecDeque::from(vec![
            "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 问题一\n</issues>"
                .to_string(),
            "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 问题依旧\n</issues>"
                .to_string(),
        ]))),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: no_gates(),
        deadlines: Arc::new(Mutex::new(Vec::new())),
        submissions: Arc::new(Mutex::new(Default::default())),
    };
    let err = run_pipeline(
        agent,
        Arc::new(Mutex::new(factory)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &["unit-a-01".to_string()],
        "complete",
        String::new(),
        None,
        None,
    )
    .unwrap_err();
    assert!(err.is_quality_failure(), "断路器应判质量失败: {err}");
    assert!(format!("{err}").contains("断路器"), "{err}");
}

#[test]
fn pass_with_finish_route_does_not_end_the_run() {
    // 2026-10-19 实测：审查官对第一个单元就回 PASS+finish，旧逻辑直接 return，
    // 第二个单元压根没写，run 却报 Settled。
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "写完 A\n<doc-complete>unit-a-01</doc-complete>",
        "写完 B\n<doc-complete>unit-b-02</doc-complete>",
    ]);
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let rep = run_pipe!(agent, f, tmp, units).unwrap();
    assert_eq!(rep.units_done, units, "finish 只收口本单元，不得结束整个 run");
    assert_eq!(rep.review_rounds, 2);
}

#[test]
fn fail_message_contains_route_and_issues() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "v1\n<doc-complete>unit-a-01</doc-complete>",
        "v2\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let received = agent.received.clone();
    let f = pipe_factory(&[
        "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>",
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    let msgs = received.borrow();
    assert!(msgs.len() >= 2);
    assert!(msgs[1].contains("<route>phase1</route>"));
    assert!(msgs[1].contains("事实错"));
}

#[test]
fn review_open_fix_round_and_unit_advance_are_logged_as_milestones() {
    // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
    // ②单元判负后的重写不落 fix.start/fix.end ③单元推进不落 phase.change → 本用例必须变红
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "v1\n<doc-complete>unit-a-01</doc-complete>",
        "v2\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let f = pipe_factory(&[
        "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>",
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    let units = vec!["unit-a-01".to_string()];
    let text = crate::observability::test_support::capture_jsonl(|| {
        run_pipe!(agent, f, tmp, units).unwrap();
    });

    // 审查开庭：两轮各一条，角色名与轮次（首审 = 1，轮次 = bump_review 的 1-based 计数）
    let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
    assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
    assert!(
        opens[0].contains("\"role\":\"doc-review\""),
        "开庭要带角色名：{}",
        opens[0]
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
    assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
    assert!(
        opens[0].contains("\"target\":\"devforge::sop::doc::runner\""),
        "里程碑的 target 必须是发出它的模块：{}",
        opens[0]
    );
    // 里程碑条目：级别 INFO + 带 component 字段（计划 §5）。
    // 自审 major：不得对任意 "event" 行断言形状——WARN 级事件（如 agent.stall，
    // 受 DEVFORGE_STALL_WARN_MS 影响）也走 event 字段，宽口径断言随环境抖动。
    for line in text.lines().filter(|l| {
        l.contains("\"event\":\"review.open\"")
            || l.contains("\"event\":\"fix.start\"")
            || l.contains("\"event\":\"fix.end\"")
            || l.contains("\"event\":\"phase.change\"")
    }) {
        crate::observability::test_support::assert_milestone_shape(line);
    }

    // 单元判负后的重写：一对 fix.start / fix.end（含轮次）
    let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
    let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
    assert_eq!(starts.len(), 1, "判负重写应落一条 fix.start：{text}");
    assert_eq!(ends.len(), 1, "判负重写应落一条 fix.end：{text}");
    assert!(starts[0].contains("\"round\":1"), "重写轮次要带上：{}", starts[0]);
    assert!(ends[0].contains("\"round\":1"), "重写轮次要带上：{}", ends[0]);

    // 单元推进：start → unit-a-01
    let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
    assert_eq!(phases.len(), 1, "一个单元应落一条推进：{text}");
    assert!(
        phases[0].contains("\"from\":\"start\"")
            && phases[0].contains("\"to\":\"unit-a-01\""),
        "单元推进要从 start 到 unit-a-01：{}",
        phases[0]
    );
}

#[test]
fn audit_text_is_injected_into_review_instruction() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    let instructions = f.instructions.clone();
    run_pipeline(
        agent,
        Arc::new(Mutex::new(f)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &["unit-a-01".to_string()],
        "complete",
        String::new(),
        Some(Box::new(|unit: &str| {
            format!("覆盖审计：单元 {unit} 缺 2 条")
        })),
        None,
    )
    .unwrap();
    let ins = instructions.lock().unwrap();
    assert!(ins[0].contains("引擎审计（确定性，勿质疑）"));
    assert!(ins[0].contains("覆盖审计：单元 unit-a-01 缺 2 条"));
}

#[test]
fn protocol_violation_retries_review_without_touching_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
    let received = agent.received.clone();
    let f = pipe_factory(&[
        "我审完了，写得不错。", // 缺协议块 → 违例（worker 层就地重跑，不重新开庭）
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(rep.units_done.len(), 1);
    assert_eq!(rep.review_rounds, 1, "违例重跑在同一轮内（round = 开庭次数）");
    assert_eq!(received.borrow().len(), 1, "agent 只被调用一次（不回注违例）");
}

#[test]
fn violation_cap_aborts_run() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory(&["无协议块", "无协议块", "无协议块", "无协议块"]);
    let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
    assert!(err.to_string().contains("协议违例"), "{err}");
}

#[test]
fn unit_mismatch_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "写完\n<doc-complete>unit-wrong-99</doc-complete>",
        "改正\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let received = agent.received.clone();
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
    run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert!(received.borrow()[1].contains("不得自行改号"));
}

/// 引擎审计修复 #2 回归：审查官 PASS 但机械门不达标（覆盖缺失）→ 单元不 mark_done、
/// 进 fix 队列；连续 2 轮不达标触发断路器——错实现形态：PASS 直通 mark_done 假绿收工。
#[test]
fn mechanical_gate_rejects_pass_without_coverage() {
    let tmp = tempfile::tempdir().unwrap();
    // 门 0 真清单：源码 fixture → build_inventory → 落 _meta/inventory.json
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/core.py"),
        "def create_note(title):\n    return title\n",
    )
    .unwrap();
    let inv = crate::docgen::inventory::build_inventory(tmp.path()).unwrap();
    let meta = tmp.path().join("docs/contexts/_meta");
    std::fs::create_dir_all(&meta).unwrap();
    inv.save(&meta.join("inventory.json")).unwrap();
    let units: Vec<String> = inv
        .entries
        .iter()
        .map(|e| e.unit_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(!units.is_empty(), "fixture 应产生至少一个单元");

    // agent 写的“正文”不含任何代码符号名 → 覆盖缺失；审查官照样 PASS → 机械门拦
    let u0 = units[0].clone();
    let m0 = format!("正文一（不含符号名）\n<doc-complete>{u0}</doc-complete>");
    let m1 = format!("正文二（仍不含符号名）\n<doc-complete>{u0}</doc-complete>");
    let agent = MockAgent::new(&[m0.as_str(), m1.as_str(), "想想", "再想想"]);
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>",
        "<doc-verdict>PASS</doc-verdict>",
    ]);
    let err = run_pipe!(agent, f, tmp, units).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("机械门不达标"),
        "连续两轮不达标应触发断路器：{msg}"
    );
}

/// converse 时真写文件的 agent：A 写 a.md，B 写 b.md——快照差异可断言
/// （引擎审计修复 #3/#5 的两个回归共用）。
struct WritingAgent {
    turns: VecDeque<String>,
    docs_root: std::path::PathBuf,
    received: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl crate::sop::planning::PlanAgentSession for WritingAgent {
    fn converse(
        &mut self,
        _message: &str,
        _ui: &mut dyn crate::sop::planning::UserLink,
    ) -> crate::error::Result<String> {
        let marker = self.turns.pop_front().expect("轮次耗尽");
        let fname = if marker.contains("unit-a") { "a.md" } else { "b.md" };
        std::fs::write(self.docs_root.join(fname), format!("# {marker}\n")).unwrap();
        self.received.lock().unwrap().push(marker.clone());
        Ok(marker)
    }
}

/// 引擎审计修复 #3 回归：lead=1 下 B 的写不得冲掉 A 的写前快照——A 的 PASS
/// 迟到（gate 卡到 B 交卷后放行）时，mark_done(A) 的文件名单必须只含 A 的产出。
/// 错实现形态：unit_before 单变量被 B 覆盖，A 的名单混入 B 的文件。
#[test]
fn pass_uses_own_units_snapshot_not_latest() {
    use crate::sop::doc::state::done_units;
    use std::collections::VecDeque;

    let tmp = tempfile::tempdir().unwrap();
    let docs_root = tmp.path().join("docs/contexts");
    std::fs::create_dir_all(&docs_root).unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let agent = WritingAgent {
        turns: VecDeque::from(vec![
            "写A\n<doc-complete>unit-a-01</doc-complete>".into(),
            "写B\n<doc-complete>unit-b-02</doc-complete>".into(),
        ]),
        docs_root: docs_root.clone(),
        received: received.clone(),
    };

    // gate 卡住 A 的审查裁决，直到 B 交卷（received.len() == 2）才放行——
    // 保证「B 的写前快照已覆盖」发生在 A 的 PASS 处理之前
    let (tx1, rx1) = gate_pair();
    let received_w = received.clone();
    let factory = PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            [
                "<doc-verdict>PASS</doc-verdict>",
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1]))),
        deadlines: Arc::new(Mutex::new(Vec::new())),
        submissions: Arc::new(Mutex::new(Default::default())),
    };
    drive_gates(
        vec![(
            Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
            2,
        )],
        vec![tx1],
    );

    run_pipeline(
        agent,
        Arc::new(Mutex::new(factory)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &["unit-a-01".to_string(), "unit-b-02".to_string()],
        "complete",
        String::new(),
        None,
        None,
    )
    .unwrap();

    let saved = state_path(&tmp).load();
    let state = saved.state().expect("断点应已落盘");
    assert_eq!(done_units(state).len(), 2);
    let files = state.data.get("files").expect("files 映射应在");
    let a_files = files
        .get("unit-a-01")
        .and_then(|v| v.as_array())
        .expect("A 的文件名单应在");
    let names: Vec<&str> = a_files.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"a.md") && !names.contains(&"b.md"),
        "A 的名单不得混入 B 的产出（快照被后续单元覆盖）：{names:?}"
    );
    let b_files = files
        .get("unit-b-02")
        .and_then(|v| v.as_array())
        .expect("B 的文件名单应在");
    let b_names: Vec<&str> = b_files.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        b_names.contains(&"b.md"),
        "B 的名单应含自己的产出：{b_names:?}"
    );
}

/// 引擎审计修复 #5 回归：一单元一提交——真 git 仓里单元 PASS 后 docs/contexts
/// 产出落成独立 commit（每单元一条）；非 git 仓降级 WARN 不阻断（其余用例即降级路径）。
#[test]
fn unit_pass_commits_docs_per_unit_in_git_repo() {
    use std::collections::VecDeque;

    let tmp = tempfile::tempdir().unwrap();
    let docs_root = tmp.path().join("docs/contexts");
    std::fs::create_dir_all(&docs_root).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(tmp.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["init", "-q"]).status.success());
    assert!(run(&["config", "user.email", "t@t"]).status.success());
    assert!(run(&["config", "user.name", "t"]).status.success());

    let agent = WritingAgent {
        turns: VecDeque::from(vec![
            "写A\n<doc-complete>unit-a-01</doc-complete>".into(),
            "写B\n<doc-complete>unit-b-02</doc-complete>".into(),
        ]),
        docs_root: docs_root.clone(),
        received: Arc::new(Mutex::new(Vec::new())),
    };
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>",
        "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
    ]);
    let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let _rep = run_pipe!(agent, f, tmp, units).unwrap();
    let log = String::from_utf8(run(&["log", "--format=%s"]).stdout).unwrap();
    assert!(
        log.contains("unit-a-01") && log.contains("unit-b-02"),
        "每单元应有独立提交：{log}"
    );
    let count = log.lines().filter(|l| l.contains("doc-from-zero")).count();
    assert_eq!(count, 2, "两单元应正好两条自动提交：{log}");
}

/// R6-P56/P66：工具交卷路径端到端——审查会话只交 submit_verdict 工具参数
/// （正文无任何 <doc-verdict> 块），工具 PASS 必须被结构化消费。
/// 旧口径（P56）：worker 解析出工具裁决却只回传文本，主循环按文本再解析
/// → 「工具 PASS」被读成「文本 FAIL」（protocol_violation）进 fix 队列。
#[test]
fn tool_verdict_pass_is_consumed_structurally() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["写完 A\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory_with_submissions(
        &["正文没有裁决块。"], // 文本协议下必 FAIL——工具 PASS 若被文本再解析此处即红
        vec![vec![submit_verdict("PASS", &[])]],
    );
    // deadlines 的 Arc 先 clone（run_pipe! 会 move f）
    let deadlines = f.deadlines.clone();
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(
        rep.units_done,
        vec!["unit-a-01".to_string()],
        "工具 PASS 必须结构化生效（不被文本再解析覆盖）"
    );
    // R6-P59：审查必须带显式墙钟（review_with_deadline 传入 Some(deadline)）——
    // 旧口径裸 review() 绕过统一墙钟，审查实例挂死 = run 永挂。
    let deadlines = deadlines.lock().unwrap();
    assert!(
        deadlines.iter().all(|d| d.is_some()),
        "每次 review 都必须传入墙钟（None = 裸调用）：{deadlines:?}"
    );
}

/// R6-P56：工具 FAIL 同样结构化生效——issues 回注 fix 队列（不是 protocol_violation）。
#[test]
fn tool_verdict_fail_feeds_fix_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "写 A\n<doc-complete>unit-a-01</doc-complete>",
        "修 A\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let f = pipe_factory_with_submissions(
        &["正文没有裁决块。", "正文没有裁决块。"],
        vec![
            vec![submit_verdict("FAIL", &["覆盖缺失：unit-a-01"])],
            vec![submit_verdict("PASS", &[])],
        ],
    );
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(rep.units_done, vec!["unit-a-01".to_string()], "FAIL 修复后 PASS 收官");
    assert!(rep.issues_total >= 1, "工具 FAIL 的 issues 应入账");
}

/// R6-P66 补充维度：工具 FAIL 无 issues = 协议违例（parse_tool_verdict 拒收）
/// → 有界重试后换合规工具 PASS 收官——violation 重试链在工具形态下同样工作。
#[test]
fn tool_verdict_fail_without_issues_is_violation_then_retry_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&[
        "写 A\n<doc-complete>unit-a-01</doc-complete>",
        "修 A\n<doc-complete>unit-a-01</doc-complete>",
    ]);
    let f = pipe_factory_with_submissions(
        &["正文没有裁决块。", "正文没有裁决块。"],
        vec![
            vec![submit_verdict("FAIL", &[])], // FAIL 无 issues = 协议违例（拒收重试）
            vec![submit_verdict("PASS", &[])], // 重试后合规 PASS
        ],
    );
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(
        rep.units_done,
        vec!["unit-a-01".to_string()],
        "违例重试后应恢复并收官"
    );
    // 违例在 worker 内消化（violations 计数，不上送）——主循环只见到合规 PASS，
    // 裁决轮数 = 1（正确行为：违例不占裁决轮，断路器只数真裁决）
    assert_eq!(rep.review_rounds, 1, "违例不占裁决轮（worker 内消化）");
}

/// R6-P64：fix 队列与失败计数落盘回放——record + save 后经 load restore 一致
/// （断路器文案承诺的「resume 可续」由断点真实承载）。
#[test]
fn fix_queue_and_fails_roundtrip_via_store() {
    use crate::sop::doc::state::{
        consecutive_fails, record_fix_queue, restore_fix_queue, set_consecutive_fails,
    };
    let store = state_path(&tempfile::tempdir().unwrap());
    let mut state = store.fresh_state();
    record_fix_queue(
        &mut state,
        &[(
            "unit-a".to_string(),
            "phase2".to_string(),
            vec!["覆盖缺失：unit-a".to_string()],
            1,
        )],
    );
    set_consecutive_fails(&mut state, 1);
    store.save(&mut state).unwrap();

    // 重启：load 回放
    let loaded = store.load();
    let reloaded = loaded.state().expect("断点应可回放");
    let q = restore_fix_queue(reloaded);
    assert_eq!(q.len(), 1, "fix 队列应从断点恢复");
    assert_eq!(q[0].0, "unit-a");
    assert_eq!(q[0].2, vec!["覆盖缺失：unit-a"]);
    assert_eq!(q[0].3, 1);
    assert_eq!(consecutive_fails(reloaded), 1, "失败计数应从断点恢复");
}

/// R7-P67 端到端：lead=1 写超前下，N+1 的半成品（缺节文档已落盘）不得把 N 的
/// 机械门判死——lint 按本单元产出文件收窄（写前→交卷快照对），全局问题归终审面。
/// 旧病灶形态：全树 lint 在 u0 裁决时扫到 b.md 半成品 → u0 被 FAIL 进 fix 队列
/// → 连续两轮不达标触发断路器，整场 run 炸掉。
#[test]
fn mechanical_gate_lint_is_scoped_to_unit_files() {
    use std::collections::VecDeque;

    let tmp = tempfile::tempdir().unwrap();
    // 门 0 真清单：两个目录各一源文件（unit 按目录划分）→ 至少两个单元
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("lib")).unwrap();
    std::fs::write(
        tmp.path().join("src/core.py"),
        "def create_note(title):\n    return title\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("lib/users.py"),
        "def list_users():\n    return []\n",
    )
    .unwrap();
    let inv = crate::docgen::inventory::build_inventory(tmp.path()).unwrap();
    let meta = tmp.path().join("docs/contexts/_meta");
    std::fs::create_dir_all(&meta).unwrap();
    inv.save(&meta.join("inventory.json")).unwrap();
    let units: Vec<String> = inv
        .entries
        .iter()
        .map(|e| e.unit_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(units.len() >= 2, "fixture 应产生至少两个单元：{units:?}");
    let (ua, ub) = (units[0].clone(), units[1].clone());

    // 真写文件的写手（按自身轮次序号驱动，不用静态计数——并行测试互不污染）：
    // 0=u0 写合规 a.md；1=u1 写超前落半成品 b.md（缺节）；2=u1 fix 重写 b.md 合规
    struct ScopedWritingAgent {
        docs_root: std::path::PathBuf,
        received: Arc<Mutex<Vec<String>>>,
        n: usize,
        unit_a: String,
        unit_b: String,
    }
    impl crate::sop::planning::PlanAgentSession for ScopedWritingAgent {
        fn converse(
            &mut self,
            message: &str,
            _ui: &mut dyn crate::sop::planning::UserLink,
        ) -> crate::error::Result<String> {
            let turn = self.n;
            self.n += 1;
            // 交卷单元名从指令里抽（kickoff/fix 指令都点名 active 单元）——
            // 预填固定名会在 fix 轮张冠李戴制造 mismatch
            let active = if message.contains(&self.unit_a) {
                self.unit_a.clone()
            } else {
                self.unit_b.clone()
            };
            let marker = format!("轮{turn}\n<doc-complete>{active}</doc-complete>");
            // 文档内容按单元名路由符号（单元名含目录段）：覆盖门的提及必须落在
            // 该单元自己的符号上，否则 coverage 会烧多余的 fix 轮污染时序断言
            // Module 条目的覆盖判定要路径/模块名词边界命中——文档必须提及
            // 自己的源码路径（如 `lib/users.py`），只写函数名会缺 Module 覆盖
            let good_doc = |sym: &str, label: &str, ex: &str, mod_path: &str| {
                // Module 条目覆盖判定：text 提路径 +（自称 module 或标题命中模块名）——
                // 标题带目录段（`lib 模块：…`）即中 titled 分支，type 保持 entry
                let dir = mod_path.split('/').next().unwrap_or(mod_path);
                format!(
                    "---\ntype: entry\n---\n\n# {dir} 模块：{label}\n\n## 用途\n`{sym}` 完成对应操作，模块源码见 `{mod_path}`。\n\n## 输入\n- 必填参数\n\n## 输出\n返回结果对象。\n\n## 错误\n400 validation_failed。\n\n## 示例\n`{ex}`\n"
                )
            };
            let (sym_a, mod_a, sym_b, mod_b) = if self.unit_a.contains("lib") {
                ("list_users", "lib/users.py", "create_note", "src/core.py")
            } else {
                ("create_note", "src/core.py", "list_users", "lib/users.py")
            };
            match turn {
                0 => std::fs::write(
                    self.docs_root.join("a.md"),
                    good_doc(sym_a, "新增笔记", "curl -X POST /api/notes", mod_a),
                )
                .unwrap(),
                // u1 写超前：半成品落盘（提及符号过覆盖门，但缺「输入/输出」节
                // ——全树 lint 会 FAIL 它，scoped lint 不看它）
                1 => std::fs::write(
                    self.docs_root.join("b.md"),
                    format!(
                        "---\ntype: entry\n---\n\n# 半成品\n\n## 用途\n`{sym_b}` 尚未写完。\n"
                    ),
                )
                .unwrap(),
                // fix 轮：谁的 fix 就重写谁的文件为合规版（active 由指令抽取）
                _ => {
                    if active == self.unit_a {
                        std::fs::write(
                            self.docs_root.join("a.md"),
                            good_doc(sym_a, "新增笔记修补", "curl -X POST /api/notes", mod_a),
                        )
                        .unwrap();
                    } else {
                        std::fs::write(
                            self.docs_root.join("b.md"),
                            good_doc(sym_b, "半成品修补", "curl -X POST /api/fix", mod_b),
                        )
                        .unwrap();
                    }
                }
            }
            self.received.lock().unwrap().push(marker.clone());
            Ok(marker)
        }
    }

    let docs_root = tmp.path().join("docs/contexts");
    std::fs::create_dir_all(&docs_root).unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let agent = ScopedWritingAgent {
        docs_root: docs_root.clone(),
        received: received.clone(),
        n: 0,
        unit_a: ua.clone(),
        unit_b: ub.clone(),
    };

    // 卡住 u0 的裁决直到 u1 交卷（写超前的半成品已在盘上）才放行——旧实现在这个
    // 时点全树 lint 会把 b.md 的缺节记到 u0 头上
    let (tx1, rx1) = gate_pair();
    let received_w = received.clone();
    drive_gates(
        vec![(
            Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
            2,
        )],
        vec![tx1],
    );

    let factory = PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            [
                "<doc-verdict>PASS</doc-verdict>",
                "<doc-verdict>PASS</doc-verdict>",
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1]))),
        deadlines: Arc::new(Mutex::new(Vec::new())),
        submissions: Arc::new(Mutex::new(Default::default())),
    };

    run_pipeline(
        agent,
        Arc::new(Mutex::new(factory)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &units,
        "complete",
        String::new(),
        None,
        None,
    )
    .expect("scoped 机械门下 u0 不应被 u1 的半成品拖进断路器");

    let saved = state_path(&tmp).load();
    let state = saved.state().expect("断点应已落盘");
    let done = done_units(state);
    assert_eq!(
        done.len(),
        2,
        "两个单元都应完成（u0 直通，u1 半成品只烧自己一轮 fix）：{done:?}"
    );
}
