//! doc 断点与证据测试（plan 010）：done 剔除 / 证据归因 / 黑板落点 / 旧状态作废。

use std::sync::{Arc, Mutex};

use crate::sop::doc::evidence::{produced_files, snapshot_files};
use crate::sop::doc::mocks::{MockAgent, NoUser, pipe_factory, state_path};
use crate::sop::doc::prompts::DOCS_DIR;
use crate::sop::doc::runner::run_pipeline;
use crate::sop::doc::state::{bump_review, mark_done};

/// 行为类测试的 pipeline 调用（与 runner_tests 同口径；此处局部复制避免跨文件宏导出）
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
fn done_units_are_skipped_on_resume() {
    // 引擎侧（plan）剔除证据成立的 done 单元；loop 只跑 to_run
    let tmp = tempfile::tempdir().unwrap();
    let store = state_path(&tmp);
    let docs = tmp.path().join(DOCS_DIR);
    std::fs::create_dir_all(docs.join("a")).unwrap();
    std::fs::write(docs.join("a/x.md"), b"x").unwrap();
    let mut state = store.fresh_state();
    mark_done(&mut state, "unit-a-01", &["a/x.md".to_string()]);
    bump_review(&mut state);
    store.save(&mut state).unwrap();

    let all = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let resume = crate::sop::doc::resume::DocResume::new(&docs);
    let plan = crate::runstate::resume::plan(
        &tmp.path().join("plans"),
        "p",
        "t",
        &resume,
        &all,
    );
    assert_eq!(plan.to_run, vec!["unit-b-02".to_string()]);

    let agent = MockAgent::new(&["B\n<doc-complete>unit-b-02</doc-complete>"]);
    let seen = agent.received.clone();
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
    let rep = run_pipe!(agent, f, tmp, plan.to_run).unwrap();
    assert_eq!(rep.units_done, all);
    assert_eq!(rep.review_rounds, 2, "已完成的单元不再审");
    assert_eq!(seen.borrow().len(), 1, "只应跑 to_run 里的那个单元");
    assert!(seen.borrow()[0].contains("unit-b-02"));
}

#[test]
fn rewritten_file_counts_as_produced() {
    // 重做单元常常是改已有文件：只看「新文件」会把重写误判为无产出
    let tmp = tempfile::tempdir().unwrap();
    let docs = tmp.path().join(DOCS_DIR);
    std::fs::create_dir_all(docs.join("a")).unwrap();
    std::fs::write(docs.join("a/x.md"), b"old").unwrap();
    let before = snapshot_files(&docs);
    std::fs::write(docs.join("a/x.md"), b"new content, longer").unwrap();
    let files = produced_files(&docs, &before);
    assert_eq!(files, vec!["a/x.md".to_string()], "改写也算产出");
}

#[test]
fn untouched_files_are_not_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let docs = tmp.path().join(DOCS_DIR);
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("keep.md"), b"same").unwrap();
    let before = snapshot_files(&docs);
    assert!(produced_files(&docs, &before).is_empty());
}

#[test]
fn checkpoint_lives_on_blackboard_not_in_output_dir() {
    // plan 010 T007：状态住黑板 `plans/<p>/<t>/<sop>/state.json`，不得再写进产物目录
    let tmp = tempfile::tempdir().unwrap();
    let agent = MockAgent::new(&["A\n<doc-complete>unit-a-01</doc-complete>"]);
    let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
    run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();

    let store = state_path(&tmp);
    assert!(store.path().exists(), "断点应落在黑板：{}", store.path().display());
    assert!(
        store.path().starts_with(tmp.path().join("plans")),
        "断点不得住在产物目录：{}",
        store.path().display()
    );
    assert!(
        !tmp.path().join(DOCS_DIR).join("_meta/state.json").exists(),
        "旧位置（产物目录）不得再出现 state.json"
    );
}

#[test]
fn legacy_output_dir_state_does_not_participate_in_resume() {
    // 旧路径的断点作废：不得因此跳过任何单元
    let tmp = tempfile::tempdir().unwrap();
    let legacy = tmp.path().join(DOCS_DIR).join("_meta");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("state.json"),
        serde_json::json!({"done": ["unit-a-01"], "review_rounds": 9}).to_string(),
    )
    .unwrap();

    let agent = MockAgent::new(&[
        "A\n<doc-complete>unit-a-01</doc-complete>",
        "B\n<doc-complete>unit-b-02</doc-complete>",
    ]);
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>",
        "<doc-verdict>PASS</doc-verdict>",
    ]);
    let rep = run_pipe!(
        agent,
        f,
        tmp,
        ["unit-a-01".to_string(), "unit-b-02".to_string()]
    )
    .unwrap();
    assert_eq!(rep.units_done, vec!["unit-a-01", "unit-b-02"], "旧断点不得生效");
    assert_eq!(rep.review_rounds, 2, "旧轮次计数不得被采用");
}

#[test]
fn broken_evidence_unit_is_reincluded_and_rewritten() {
    // plan 010 T004：断点说 done，但证据不成立 → 必须重新进 kickoff 集合
    let tmp = tempfile::tempdir().unwrap();
    let store = state_path(&tmp);
    crate::sop::doc::mocks::seed_done(&store, &["unit-a-01"]); // 未登记任何产出文件

    let resume = crate::sop::doc::resume::DocResume::new(tmp.path().join(DOCS_DIR));
    let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let plan = crate::runstate::resume::plan(
        &tmp.path().join("plans"),
        "p",
        "t",
        &resume,
        &units,
    );
    assert!(plan.resumed);
    assert_eq!(plan.redo, vec!["unit-a-01".to_string()]);
    assert!(
        plan.to_run.contains(&"unit-a-01".to_string()),
        "证据失效的单元必须重跑：{:?}",
        plan.to_run
    );

    // 引擎按 to_run 派发：agent 必须真的收到 unit-a-01 的 kickoff 并重写
    let agent = MockAgent::new(&[
        "重写 A\n<doc-complete>unit-a-01</doc-complete>",
        "写 B\n<doc-complete>unit-b-02</doc-complete>",
    ]);
    let seen = agent.received.clone();
    let f = pipe_factory(&[
        "<doc-verdict>PASS</doc-verdict>",
        "<doc-verdict>PASS</doc-verdict>",
    ]);
    let rep = run_pipeline(
        agent,
        Arc::new(Mutex::new(f)),
        state_path(&tmp),
        &mut NoUser,
        tmp.path(),
        &plan.to_run,
        "complete",
        crate::runstate::resume::Resumable::resume_brief(
            &resume,
            store.load().state().expect("断点应在"),
            &plan.redo,
        ),
        None,
        None,
    )
    .unwrap();
    // 硬断言：kickoff 回路真的发生，而不是只信 plan 与收尾名单（审计指出的假阳性）
    let kicks = seen.borrow();
    assert_eq!(kicks.len(), 2, "两个单元都应收到 kickoff：{kicks:?}");
    assert!(
        kicks[0].contains("unit-a-01"),
        "第一个 kickoff 必须是被重做的单元：{}",
        kicks[0]
    );
    // 自审 minor（43）：不再断言兄弟模块的人类可读文案（那种断言让文案改动变成跨模块
    // 隐式契约）。改成结构断言：kickoff 必须携带「重做」语义的标记块，且指向重做单元清单。
    assert!(
        kicks[0].contains("unit-a-01") && kicks[0].to_lowercase().contains("重写"),
        "kickoff 必须说明该单元要重写：{}",
        kicks[0]
    );
    assert_eq!(rep.units_done, units);
}

#[test]
fn intact_evidence_is_not_redispatched() {
    let tmp = tempfile::tempdir().unwrap();
    let store = state_path(&tmp);
    // 登记 done + 真实存在的产物文件
    let docs = tmp.path().join(DOCS_DIR);
    std::fs::create_dir_all(docs.join("a")).unwrap();
    std::fs::write(docs.join("a/x.md"), b"x").unwrap();
    let mut state = store.fresh_state();
    mark_done(&mut state, "unit-a-01", &["a/x.md".to_string()]);
    store.save(&mut state).unwrap();

    let resume = crate::sop::doc::resume::DocResume::new(&docs);
    let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
    let plan = crate::runstate::resume::plan(
        &tmp.path().join("plans"),
        "p",
        "t",
        &resume,
        &units,
    );
    assert_eq!(plan.skip, vec!["unit-a-01".to_string()]);
    assert_eq!(plan.to_run, vec!["unit-b-02".to_string()]);
}

// DEVFORGE_DOC_ONLY 单元重派随串行 runner 退役（ARC-R1）：单单元重跑由续跑计划承担
//（见 done_units_are_skipped_on_resume / broken_evidence_unit_is_reincluded_and_rewritten）
