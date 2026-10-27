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
    let p = plan(tmp.path(), "p", "t", &sop, &units());
    assert!(!p.resumed);
    assert_eq!(p.to_run, units());
    assert!(p.skip.is_empty() && p.redo.is_empty() && p.note.is_none());
}

#[test]
/// 自审 minor 185：`plan()` 不再有 fresh 参数——「清断点再冷启动」的唯一入口是
/// [`start_gate`]。本用例原来断言的是那个已删除的分支，现在改为钉住新口径：
/// start_gate(fresh) 清完断点之后，plan() 看到的必须是冷启动。
#[test]
fn fresh_path_is_owned_by_start_gate_only() {
    let tmp = tempfile::tempdir().unwrap();
    for u in ["u1", "u2"] {
        std::fs::write(tmp.path().join(format!("{u}.txt")), b"x").unwrap();
    }
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    // fresh=true 的入口：start_gate 负责清断点
    start_gate(tmp.path(), "p", "t", "fake", true).expect("start_gate 应成功");
    // 清完之后 plan() 只能看到空断点 = 冷启动，全量重跑
    let p = plan(tmp.path(), "p", "t", &sop, &units());
    assert!(!p.resumed, "断点已被 start_gate 清掉，plan 必须冷启动");
    assert_eq!(p.to_run, units(), "fresh 之后必须全量重跑");
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
    let p = plan(tmp.path(), "p", "t", &sop, &units());
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
    let p = plan(tmp.path(), "p", "t", &sop, &units());
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
    let p = plan(tmp.path(), "p", "t", &sop, &units());
    assert!(!p.resumed);
    assert_eq!(p.to_run, units());
    assert!(p.note.as_deref().unwrap_or("").contains("解析失败"));
}

#[test]
fn another_sop_checkpoint_is_invisible() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "other-sop", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units());
    assert!(!p.resumed, "不能读到别的 SOP 的断点");
    assert_eq!(p.to_run, units());
}

#[test]
fn plan_is_order_stable_and_deduped_by_identity() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("u1.txt"), b"x").unwrap();
    seed(tmp.path(), "fake", &["u1", "ghost-unit"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units());
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

/// 自审 minor（191）：旧用例传的是**新建的空 state**、断言的是假 SOP 自己的字符串拼接，
/// `brief_calls` 记录器全仓无断言——删掉 resume_brief 的真实语义它仍绿。现在断记录器：
/// 传给 brief 的 redo 集合必须就是 plan 算出来的那份（且 state 是 plan 带出的同一快照）。
#[test]
fn brief_receives_redo_set() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    let p = plan(tmp.path(), "p", "t", &sop, &units());
    let state = p.state.clone().expect("plan 必须交回同一份断点快照");
    let _brief = sop.resume_brief(&state, &p.redo);

    let calls = sop.brief_calls.borrow();
    assert_eq!(calls.len(), 1, "brief 必须被调用一次");
    assert_eq!(calls[0].0, p.redo.len(), "长度口径一致");
    let seen: BTreeSet<String> = calls[0].1.iter().cloned().collect();
    let expected: BTreeSet<String> = p.redo.iter().cloned().collect();
    assert_eq!(seen, expected, "传给 brief 的 redo 必须就是 plan 的那份");
    assert_eq!(expected.len(), 2, "red：两个产物都不存在");
    // 断点快照里的 done 名单必须与输入一致（不是空 state）
    assert_eq!(
        crate::runstate::resume::done_units(&state),
        vec!["u1".to_string(), "u2".to_string()]
    );
}

/// 自审 minor（99）：invalidate_done 必须把重做单元从 done 名单摘掉，且幂等。
#[test]
fn invalidate_done_removes_redo_units() {
    let key = StateKey::new("p", "t", "fake");
    let mut st = RunState::new(&key, serde_json::json!({"done": ["u1", "u2", "u3"]}));
    assert!(invalidate_done(&mut st, &["u2".to_string()]));
    assert_eq!(done_units(&st), vec!["u1".to_string(), "u3".to_string()]);
    assert!(!invalidate_done(&mut st, &["u2".to_string()]), "已摘除即无改动");
    assert!(!invalidate_done(&mut st, &[]), "空 redo 无改动");
}

/// 自审 minor（187）：redo 必须与本轮清单求交——不在本轮计划里的单元不得报给 agent。
#[test]
fn redo_is_intersected_with_this_rounds_units() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), "fake", &["u1", "u2"]);
    let sop = FakeSop::new("fake", tmp.path());
    // 本轮只有 u1（u2 的产物缺失，但它不在本轮清单里）
    let p = plan(tmp.path(), "p", "t", &sop, &["u1".to_string()]);
    assert_eq!(p.redo, vec!["u1".to_string()], "只报本轮内的单元");
}

/// 抽象守卫：trait 所在源文件里不得出现任何具体 SOP 的业务词。
///
/// 自审 minor（192）：旧口径的禁用词表是自己编的、与真实业务词不重合（"doc-complete" /
/// "impl-step" 全仓都不存在）——捕获不到它本想防的泄漏。现在禁用词表**从注册表与真实
/// 协议标记里生成**，并额外禁止直接依赖 SOP/docgen 模块。
#[test]
fn trait_surface_stays_agnostic() {
    let src = include_str!("resume.rs");
    let surface = src.split("#[cfg(test)]").next().unwrap_or("");

    // ① 真实 SOP id（来自内置注册表，不是我手编的名单）
    let mut forbidden: Vec<String> = crate::domain::sop::SopRegistry::builtin()
        .ids()
        .into_iter()
        .map(str::to_string)
        .collect();
    // ② 真实协议标记（各 SOP 与 agent 约定的字面量）
    for marker in [
        "<stage-complete>",
        "<doc-verdict>",
        "<stage-verdict>",
        "<audit-findings>",
        "<impact-gap>",
        "unit-src-",
        "assembly",
    ] {
        forbidden.push(marker.to_string());
    }
    for word in &forbidden {
        assert!(
            !surface.contains(word.as_str()),
            "引擎接入面混入了 SOP 业务词：{word}"
        );
    }
    // ③ 抽象层不得直接依赖具体 SOP / 文档流水线模块
    for path in ["crate::sop::", "crate::docgen::", "crate::change::"] {
        assert!(
            !surface.contains(path),
            "引擎接入面直接依赖了具体业务模块：{path}"
        );
    }
}
