//! SOP 接入面（plan 010 · T002）：引擎只管「续到哪」，SOP 管「凭什么算做完」。
//!
//! 分工（010 D005）：
//! - **引擎**（本模块 + [`super`]）：状态怎么存、`start` 续跑/fresh 语义、复核调度、故障重启。
//! - **SOP**（实现 [`Resumable`]）：扩展段结构、`done` 的证据定义与复核、恢复时给 agent 的开工语。
//!
//! 这个 trait 刻意只有三个方法，且**不含任何具体 SOP 的业务词**——一旦这里出现某个 SOP 的
//! 单元名/阶段名，就说明抽象漏了，引擎会被迫知道业务细节。

use std::path::Path;

use crate::error::Result;
use crate::runstate::{Loaded, RunState, StateKey, load};

/// 一个可断点续跑的 SOP。
pub trait Resumable {
    /// SOP 名（与注册名一致，用作状态目录名）。
    fn name(&self) -> &str;

    /// 复核 `done`：返回**证据已失效、必须重做**的单元。
    ///
    /// 只做机械可判定的事（文件在不在、门禁还绿不绿）；语义判断归审查，不归这里。
    fn verify_done(&self, state: &RunState) -> Vec<String>;

    /// 恢复时追加给 agent 的开工语（重做了哪些、为什么）。空串表示不需要额外说明。
    fn resume_brief(&self, state: &RunState, redo: &[String]) -> String;
}

/// 续跑计划：调用方据此决定跳过哪些、重做哪些。
#[derive(Debug, Clone, PartialEq)]
pub struct ResumePlan {
    /// 是否从断点继续（false = 冷启动）。
    pub resumed: bool,
    /// 本次真正要跑的单元（按传入顺序，已剔除可信的 done）。
    pub to_run: Vec<String>,
    /// 本次要重做的单元（原本 done，但证据失效）。
    pub redo: Vec<String>,
    /// 本次跳过的单元（done 且证据成立）。
    pub skip: Vec<String>,
    /// 加载断点失败的原因（冷启动时非空，供记日志）。
    pub note: Option<String>,
}

/// 计算续跑计划。
///
/// - `fresh = true`：忽略断点，全部重跑（调用方负责清空检查点）。
/// - 断点缺失 / 不可用：冷启动，全部跑。
/// - 断点可用：`verify_done` 复核，证据失效的单元并入重做集合。
pub fn plan<S: Resumable + ?Sized>(
    plans_root: &Path,
    project: &str,
    task: &str,
    sop: &S,
    all_units: &[String],
    fresh: bool,
) -> ResumePlan {
    if fresh {
        return ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: Some("fresh: true —— 忽略断点全量重跑".into()),
        };
    }

    let key = StateKey::new(project, task, sop.name());
    match load(plans_root, &key) {
        Loaded::Found(state) => {
            let redo = sop.verify_done(&state);
            let skip: Vec<String> = all_units
                .iter()
                .filter(|u| !redo.iter().any(|r| r == *u))
                .filter(|u| done_units(&state).iter().any(|d| d == *u))
                .cloned()
                .collect();
            let to_run: Vec<String> = all_units
                .iter()
                .filter(|u| !skip.iter().any(|s| s == *u))
                .cloned()
                .collect();
            ResumePlan {
                resumed: true,
                to_run,
                redo,
                skip,
                note: None,
            }
        }
        Loaded::Missing => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: None,
        },
        Loaded::Unusable(reason) => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: Some(reason),
        },
    }
}

/// `start` 的续跑判定结果（plan 010 · T003）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartDecision {
    /// 是否从断点继续（false = 冷启动）。
    pub resumed: bool,
    /// 冷启动原因（fresh / 断点不可用），供日志与响应回显。
    pub note: Option<String>,
}

/// `start` 入口的续跑门：`fresh = true` 时清空该 SOP 的断点并回冷启动。
///
/// **只动该 SOP 自己那份**——同一 task 下其他 SOP 的状态文件不受影响（D001）。
pub fn start_gate(
    plans_root: &Path,
    project: &str,
    task: &str,
    sop_name: &str,
    fresh: bool,
) -> Result<StartDecision> {
    let key = StateKey::new(project, task, sop_name);
    if fresh {
        crate::runstate::clear(plans_root, &key)?;
        return Ok(StartDecision {
            resumed: false,
            note: Some("fresh: true —— 已清空该 SOP 的断点，全量重跑".into()),
        });
    }
    match load(plans_root, &key) {
        Loaded::Found(_) => Ok(StartDecision {
            resumed: true,
            note: None,
        }),
        Loaded::Missing => Ok(StartDecision {
            resumed: false,
            note: None,
        }),
        Loaded::Unusable(reason) => Ok(StartDecision {
            resumed: false,
            note: Some(reason),
        }),
    }
}

/// 从状态扩展段取 `done` 名单（各 SOP 统一放这里，便于引擎不碰业务结构）。
pub fn done_units(state: &RunState) -> Vec<String> {
    state
        .data
        .get("done")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
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
}
