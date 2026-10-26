//! （2026-10-26 测试分离：自 src/runstate/resume.rs 内联平移，纯移动零改动。）
use super::*;
use std::collections::BTreeSet;
use std::cell::RefCell;

/// 假 SOP：证据就是「这个单元对应的文件在不在」，与任何真实 SOP 无关。
struct FakeSop {
    name: &'static str,
    /// 单元 → 证据文件
    evidence: Vec<(String, std::path::PathBuf)>,
    brief_calls: RefCell<Vec<(usize, Vec<String>)>>,
}

impl FakeSop {
    fn new(name: &'static str, root: &Path) -> Self {
        Self {
            name,
            evidence: vec![
                ("u1".to_string(), root.join("u1.txt")),
                ("u2".to_string(), root.join("u2.txt")),
            ],
            brief_calls: RefCell::new(Vec::new()),
        }
    }
}

impl Resumable for FakeSop {
    fn name(&self) -> &str {
        self.name
    }
    fn verify_done(&self, _state: &RunState) -> Vec<String> {
        self.evidence
            .iter()
            .filter(|(_, p)| !p.exists())
            .map(|(u, _)| u.clone())
            .collect()
    }
    fn resume_brief(&self, _state: &RunState, redo: &[String]) -> String {
        self.brief_calls
            .borrow_mut()
            .push((redo.len(), redo.to_vec()));
        if redo.is_empty() {
            String::new()
        } else {
            format!("以下单元的产物已不存在，必须重做：{}", redo.join(", "))
        }
    }
}

fn units() -> Vec<String> {
    vec!["u1".into(), "u2".into(), "u3".into()]
}

fn seed(root: &Path, sop: &str, done: &[&str]) {
    let key = StateKey::new("p", "t", sop);
    let mut st = RunState::new(&key, serde_json::json!({ "done": done }));
    crate::runstate::save(root, &key, &mut st).unwrap();
}

#[test]
fn no_checkpoint_means_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert!(!p.resumed);
    assert_eq!(p.to_run, units());
    assert!(p.skip.is_empty() && p.redo.is_empty() && p.note.is_none());
}

#[test]
fn fresh_ignores_checkpoint_entirely() {
    let tmp = tempfile::tempdir().unwrap();
    for u in ["u1", "u2"] {
        std::fs::write(tmp.path().join(format!("{u}.txt")), b"x").unwrap();
    }
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), true);
    assert!(!p.resumed);
    assert_eq!(p.to_run, units(), "fresh 必须全量重跑");
    assert!(p.skip.is_empty());
}

#[test]
fn valid_evidence_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    for u in ["u1", "u2"] {
        std::fs::write(tmp.path().join(format!("{u}.txt")), b"x").unwrap();
    }
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert!(p.resumed);
    assert_eq!(p.skip, vec!["u1".to_string(), "u2".to_string()]);
    assert_eq!(p.to_run, vec!["u3".to_string()]);
    assert!(p.redo.is_empty());
}

#[test]
fn broken_evidence_is_redone_not_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    // u2 的产物被删 → done 里有，但必须重做
    std::fs::write(tmp.path().join("u1.txt"), b"x").unwrap();
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert_eq!(p.skip, vec!["u1".to_string()]);
    assert_eq!(p.redo, vec!["u2".to_string()]);
    assert!(p.to_run.contains(&"u2".to_string()), "证据失效的单元必须重跑");
    assert!(!p.to_run.contains(&"u1".to_string()));
}

#[test]
fn corrupt_checkpoint_falls_back_to_cold_start_with_note() {
    let tmp = tempfile::tempdir().unwrap();
    let key = StateKey::new("p", "t", "fake");
    let path = key.path(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ broken").unwrap();
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert!(!p.resumed);
    assert_eq!(p.to_run, units());
    assert!(p.note.as_deref().unwrap_or("").contains("解析失败"));
}

#[test]
fn another_sop_checkpoint_is_invisible() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "other-sop", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert!(!p.resumed, "不能读到别的 SOP 的断点");
    assert_eq!(p.to_run, units());
}

#[test]
fn plan_is_order_stable_and_deduped_by_identity() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("u1.txt"), b"x").unwrap();
    seed(tmp.path(), "fake", &["u1", "ghost-unit"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    assert_eq!(p.skip, vec!["u1".to_string()]);
    assert_eq!(p.to_run, vec!["u2".to_string(), "u3".to_string()]);
    // 断点里存在但清单里没有的单元不得凭空复活
    assert!(!p.to_run.contains(&"ghost-unit".to_string()));
}

#[test]
fn start_gate_cold_start_when_no_checkpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let d = start_gate(tmp.path(), "p", "t", "fake", false).unwrap();
    assert!(!d.resumed);
    assert!(d.note.is_none());
}

#[test]
fn start_gate_resumes_when_checkpoint_present() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "fake", &["u1"]);
    let d = start_gate(tmp.path(), "p", "t", "fake", false).unwrap();
    assert!(d.resumed);
    assert!(d.note.is_none());
}

#[test]
fn start_gate_fresh_clears_only_that_sop() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "fake", &["u1"]);
    seed(tmp.path(), "other-sop", &["x"]);
    let d = start_gate(tmp.path(), "p", "t", "fake", true).unwrap();
    assert!(!d.resumed);
    assert!(d.note.as_deref().unwrap_or("").contains("fresh"));
    // 该 SOP 的 done 已清空
    assert!(!matches!(
        crate::runstate::load(tmp.path(), &StateKey::new("p", "t", "fake")),
        crate::runstate::Loaded::Found(_)
    ));
    // 其他 SOP 的断点不受影响
    assert!(matches!(
        crate::runstate::load(tmp.path(), &StateKey::new("p", "t", "other-sop")),
        crate::runstate::Loaded::Found(_)
    ));
}

#[test]
fn start_gate_reports_unusable_checkpoint_as_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let key = StateKey::new("p", "t", "fake");
    let path = key.path(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not json").unwrap();
    let d = start_gate(tmp.path(), "p", "t", "fake", false).unwrap();
    assert!(!d.resumed);
    assert!(d.note.is_some(), "损坏的断点必须在响应里回显原因");
}

#[test]
fn brief_receives_redo_set() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units(), false);
    let brief = sop.resume_brief(&RunState::new(&StateKey::new("p", "t", "fake"), serde_json::json!({})), &p.redo);
    let seen: BTreeSet<String> = p.redo.iter().cloned().collect();
    assert_eq!(seen.len(), 2);
    assert!(brief.contains("u1") && brief.contains("u2"), "{brief}");
}

/// 抽象守卫：trait 所在源文件里不得出现任何具体 SOP 的业务词。
#[test]
fn trait_surface_stays_agnostic() {
    let src = include_str!("resume.rs");
    let surface = src.split("#[cfg(test)]").next().unwrap_or("");
    for forbidden in ["doc-complete", "doc-verdict", "impl-step", "unit-src", "stage-complete"] {
        assert!(
            !surface.contains(forbidden),
            "引擎接入面混入了 SOP 业务词：{forbidden}"
        );
    }
}
