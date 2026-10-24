//! run_pipeline 行为测试（DR-14 流水线：重叠/fix 轮/断路器/里程碑/审计注入/协议重试）。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::sop::doc::mocks::{
    MockAgent, NoUser, PipeAgent, PipeReviewFactory, drive_gates, gate_pair, no_gates,
    pipe_factory, state_path,
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
    let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
    assert_eq!(rep.units_done, vec!["unit-a-01"]);
    assert_eq!(instructions.lock().unwrap().len(), 2, "空回复应换实例重试一次");
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
    assert!(format!("{err}").contains("doc-complete"), "{err}");
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
fn kickoff_carries_interleave_and_batch_rules() {
    // DR-11/DR-10④：kickoff 必须带穿插节奏与批量写预算护栏（合同 §七 同款）
    let k = doc_kickoff(Path::new("/repo"), "unit-x", "complete");
    assert!(k.contains("穿插推进"), "要带穿插节奏条款: {k}");
    assert!(k.contains("批量写出"), "要带同回合批量写: {k}");
    assert!(k.contains("30K tokens"), "要带输出预算护栏: {k}");
    assert!(k.contains("压轴最后写"), "枢纽压轴规则要在: {k}");
}

#[test]
fn review_instruction_puts_verdict_first() {
    // DR-13：裁决块前置——防思考型输出把协议块挤到截断线外；DR-12：不读码条款
    let ins = doc_review_instruction("unit-x", 1);
    assert!(ins.contains("第一行"), "要强制裁决块前置: {ins}");
    assert!(ins.contains("<doc-verdict>"), "要带协议块格式: {ins}");
    assert!(ins.contains("4000 字"), "要带叙述预算: {ins}");
    assert!(ins.contains("本轮作废"), "要写明截断后果: {ins}");
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
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1]))),
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
        gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1, rx2, rx3]))),
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
        gates: no_gates(),
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
    // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
    for line in text.lines().filter(|l| l.contains("\"event\":")) {
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
