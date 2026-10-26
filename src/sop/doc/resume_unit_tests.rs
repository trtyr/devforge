//! （2026-10-26 测试分离：自 src/sop/doc/resume.rs 内联平移，纯移动零改动。）
use super::*;
use crate::runstate::resume::done_units;
use serde_json::json;

fn state_with(done: &[&str], files: serde_json::Value) -> RunState {
    let key = crate::runstate::StateKey::new("p", "t", DOC_SOP);
    RunState::new(&key, json!({ "done": done, "files": files }))
}

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

#[test]
fn unit_without_recorded_files_is_redone() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state_with(&["unit-a"], json!({}));
    let redo = DocResume::new(tmp.path()).verify_done(&st);
    assert_eq!(redo, vec!["unit-a".to_string()], "无证据不得当已完成");
}

#[test]
fn missing_product_file_forces_redo_of_that_unit_only() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    // b/c.md 不存在
    let st = state_with(
        &["unit-a", "unit-b"],
        json!({ "unit-a": ["a/x.md"], "unit-b": ["b/c.md"] }),
    );
    let redo = DocResume::new(tmp.path()).verify_done(&st);
    assert_eq!(redo, vec!["unit-b".to_string()]);
}

#[test]
fn all_products_present_is_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    let st = state_with(&["unit-a"], json!({ "unit-a": ["a/x.md"] }));
    assert!(DocResume::new(tmp.path()).verify_done(&st).is_empty());
}

#[test]
fn red_lint_invalidates_every_done_unit() {
    // lint 本身挂了（None）→ 保守全重做（旧口径保留）
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    let st = state_with(
        &["unit-a", "unit-b"],
        json!({ "unit-a": ["a/x.md"], "unit-b": ["a/x.md"] }),
    );
    let resume = DocResume::new(tmp.path()).with_lint(|_| Err("lint 崩了".into()));
    let mut redo = resume.verify_done(&st);
    redo.sort();
    assert_eq!(redo, vec!["unit-a".to_string(), "unit-b".to_string()]);
}

fn violation(file: &str, check: Check) -> Violation {
    Violation {
        file: file.into(),
        line: 1,
        check,
        severity: crate::docgen::lint::Severity::Fail,
        message: "测试违规".into(),
    }
}

#[test]
fn link_violation_in_own_file_is_tolerated() {
    // DR-16：指向未来单元枢纽的死链在树未完整时不可避免——Link 类不算证据失效
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    let st = state_with(&["unit-a"], json!({ "unit-a": ["a/x.md"] }));
    let resume = DocResume::new(tmp.path()).with_lint(|_| {
        Ok(vec![violation("a/x.md", Check::Link)])
    });
    assert!(resume.verify_done(&st).is_empty(), "Link 违规应被容忍");
}

#[test]
fn entity_violation_in_own_file_invalidates_that_unit_only() {
    // DR-16：单元级归因——违规落谁头上谁重做，别的单元不牵连
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    touch(tmp.path(), "b/y.md");
    let st = state_with(
        &["unit-a", "unit-b"],
        json!({ "unit-a": ["a/x.md"], "unit-b": ["b/y.md"] }),
    );
    let resume = DocResume::new(tmp.path()).with_lint(|_| {
        Ok(vec![violation("a/x.md", Check::Entity)])
    });
    assert_eq!(resume.verify_done(&st), vec!["unit-a".to_string()]);
}

#[test]
fn empty_done_needs_no_check() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state_with(&[], json!({}));
    assert!(DocResume::new(tmp.path()).verify_done(&st).is_empty());
    assert!(done_units(&st).is_empty());
}

#[test]
fn brief_names_every_redo_unit() {
    let tmp = tempfile::tempdir().unwrap();
    let st = state_with(&["unit-a"], json!({}));
    let redo = DocResume::new(tmp.path()).verify_done(&st);
    let brief = DocResume::new(tmp.path()).resume_brief(&st, &redo);
    assert!(brief.contains("unit-a"), "{brief}");
    let quiet = DocResume::new(tmp.path()).resume_brief(&st, &[]);
    assert!(quiet.is_empty());
}

#[test]
fn sop_name_matches_registration() {
    assert_eq!(DocResume::new("/tmp").name(), "doc-from-zero");
}

/// 自审 major 回归：Warn 级违规是改进提示，不是证据失效——
/// 单元自有文件里的 Warn 不再触发重做。
#[test]
fn warn_severity_violation_does_not_invalidate() {
let tmp = tempfile::tempdir().unwrap();
touch(tmp.path(), "a/x.md");
let st = state_with(&["unit-a"], json!({ "unit-a": ["a/x.md"] }));
let mut warn = violation("a/x.md", Check::Entity);
warn.severity = Severity::Warn;
let resume = DocResume::new(tmp.path()).with_lint(move |_| {
    let v = warn.clone();
    Ok(vec![v])
});
assert!(
    resume.verify_done(&st).is_empty(),
    "Warn 级违规不得判证据失效"
);
}

/// 自审 major 回归：Fail 级违规仍照旧触发重做（防 Warn 过滤把 Fail 也滤掉）。
#[test]
fn fail_severity_violation_still_invalidates() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a/x.md");
    let st = state_with(&["unit-a"], json!({ "unit-a": ["a/x.md"] }));
    let resume =
        DocResume::new(tmp.path()).with_lint(|_| Ok(vec![violation("a/x.md", Check::Entity)]));
    assert_eq!(resume.verify_done(&st), vec!["unit-a".to_string()]);
}
