//! （2026-10-26 测试分离：自 src/sop/doc/incremental.rs 内联平移，纯移动零改动。）
use super::*;
use crate::docgen::impact::Impact;
use crate::runstate::StateKey;

fn state(done: &[&str], base: Option<&str>) -> RunState {
    let key = StateKey::new("p", "t", "doc-from-zero");
    let mut data = serde_json::json!({ "done": done });
    if let Some(b) = base {
        data["base_sha"] = serde_json::Value::String(b.into());
    }
    RunState::new(&key, data)
}

fn plan(all: &[&str], done: &[&str]) -> ResumePlan {
    ResumePlan {
        resumed: true,
        to_run: Vec::new(),
        redo: Vec::new(),
        state: None,
        skip: all.iter().map(|s| s.to_string()).filter(|u| !done.contains(&u.as_str())).collect(),
        note: None,
    }
}

#[test]
fn base_advances_only_on_pass() {
    let mut st = state(&[], Some("old"));
    assert!(!advance_base(&mut st, Some("new"), false), "失败不得推进");
    assert_eq!(state_base(&st).as_deref(), Some("old"));
    assert!(advance_base(&mut st, Some("new"), true));
    assert_eq!(state_base(&st).as_deref(), Some("new"));
    assert!(!advance_base(&mut st, None, true), "取不到 HEAD 不得瞎写");
}

#[test]
fn disabled_mode_is_full() {
    let st = state(&[], Some("abc"));
    assert_eq!(
        decide_mode(Some(&st), Some("def"), None, false),
        Mode::Full {
            reason: "未开启增量（DEVFORGE_DOC_INCREMENTAL）".into()
        }
    );
}

#[test]
fn missing_base_degrades_to_full_with_reason() {
    let st = state(&["unit-a"], None);
    match decide_mode(Some(&st), Some("head"), None, true) {
        Mode::Full { reason } => assert!(reason.contains("base_sha"), "{reason}"),
        other => panic!("应降级全量：{other:?}"),
    }
}

#[test]
fn missing_head_degrades_to_full() {
    let st = state(&[], Some("abc"));
    match decide_mode(Some(&st), None, None, true) {
        Mode::Full { reason } => assert!(reason.contains("HEAD"), "{reason}"),
        other => panic!("应降级全量：{other:?}"),
    }
}

#[test]
fn explicit_base_overrides_state_and_enables_incremental() {
    let st = state(&[], Some("old"));
    assert_eq!(
        decide_mode(Some(&st), Some("head"), Some("given"), true),
        Mode::Incremental {
            base: "given".into(),
            head: "head".into(),
        }
    );
}

#[test]
fn only_affected_units_are_dispatched() {
    let all = ["unit-a", "unit-b", "unit-c"];
    let mut st = state(&["unit-a", "unit-b", "unit-c"], Some("base1"));
    let mut p = plan(&all, &[]);
    let impact = Impact {
        units: vec!["unit-b".into()],
        ..Default::default()
    };
    let to_run = merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
    assert_eq!(to_run, vec!["unit-b".to_string()], "只派发受影响单元");
    assert_eq!(p.skip, vec!["unit-a".to_string(), "unit-c".to_string()]);
    assert_eq!(p.redo, vec!["unit-b".to_string()], "原本 done 的受影响单元记 redo");
}

#[test]
fn affected_unit_is_removed_from_done_before_dispatch() {
    let all = ["unit-a", "unit-b"];
    let mut st = state(&["unit-a", "unit-b"], Some("base1"));
    let mut p = plan(&all, &[]);
    let impact = Impact {
        units: vec!["unit-a".into()],
        ..Default::default()
    };
    merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
    let done = crate::runstate::resume::done_units(&st);
    assert_eq!(done, vec!["unit-b".to_string()], "受影响单元必须先摘掉，崩溃也不假绿");
}

#[test]
fn incremental_record_is_written_into_state() {
    let all = ["unit-a"];
    let mut st = state(&["unit-a"], Some("base1"));
    let mut p = plan(&all, &[]);
    let impact = Impact {
        units: vec!["unit-a".into()],
        assembly_review: vec!["README.md".into()],
        ..Default::default()
    };
    merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
    assert_eq!(st.data["incremental"]["units"][0], "unit-a");
    assert_eq!(st.data["incremental"]["assembly_review"][0], "README.md");
}

#[test]
fn stale_assembly_reference_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let docs = tmp.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "a\nb\n").unwrap();
    std::fs::write(
        docs.join("README.md"),
        "见 `src/a.rs:2`，旧引用 `src/a.rs:99`，失效 `src/gone.rs:1`\n",
    )
    .unwrap();
    let notes = consistency_notes(&docs, &repo);
    assert!(
        notes.iter().any(|n| n.contains("src/a.rs:99")),
        "越界行号要报：{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("src/gone.rs:1")),
        "缺失文件要报：{notes:?}"
    );
    assert!(
        !notes.iter().any(|n| n.contains("src/a.rs:2`")),
        "有效引用不该报：{notes:?}"
    );
}

#[test]
fn missing_glossary_and_inventory_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let notes = consistency_notes(tmp.path(), tmp.path());
    assert!(notes.iter().any(|n| n.contains("glossary.md")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("inventory.json")), "{notes:?}");
}
