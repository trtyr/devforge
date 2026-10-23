//! `doc-from-zero` 的续跑接入（plan 010 · T004/T007）：声明本 SOP 的 done 证据与恢复语。
//!
//! 证据口径（010 D003 + DR-16 单元级归因）：一个单元算「真做完」= 它当初产出的文件都还在
//! + 落在它自己文件里的 lint 违规只剩 Link 类（穿插/枢纽后写模式下，指向未来单元的死链
//! 在树未完整时不可避免，由 run 末尾的全量 lint 兕底）。只做机械可判定的事。

use std::path::{Path, PathBuf};

use crate::docgen::lint::{Check, Violation};
use crate::runstate::resume::{Resumable, done_units};
use crate::runstate::RunState;

/// 本 SOP 的注册名（同时是状态目录名）。
pub const DOC_SOP: &str = "doc-from-zero";

/// `doc-lint` 门卫：跑一次全树 lint，返回违规清单；`None` = lint 本身挂了（保守全重做）。
pub type LintGate = Box<dyn Fn(&RunState) -> Option<Vec<Violation>> + Send + Sync>;

/// 文档 SOP 的续跑适配器。
pub struct DocResume {
    /// 产出根：`<repo>/docs/contexts`。
    docs_root: PathBuf,
    /// `doc-lint` 门卫。
    lint: Option<LintGate>,
}

impl DocResume {
    pub fn new(docs_root: impl Into<PathBuf>) -> Self {
        Self {
            docs_root: docs_root.into(),
            lint: None,
        }
    }

    pub fn with_lint(mut self, f: impl Fn(&RunState) -> Option<Vec<Violation>> + Send + Sync + 'static) -> Self {
        self.lint = Some(Box::new(f));
        self
    }

    /// 某个单元当初产出的文件（相对产出根）。
    pub fn files_of(state: &RunState, unit: &str) -> Vec<String> {
        state
            .data
            .get("files")
            .and_then(|v| v.get(unit))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Resumable for DocResume {
    fn name(&self) -> &str {
        DOC_SOP
    }

    fn verify_done(&self, state: &RunState) -> Vec<String> {
        let done = done_units(state);
        if done.is_empty() {
            return done;
        }
        // 1) 逐个单元：没登记产出 = 无证据；登记了但文件不在 = 证据失效
        let mut redo: Vec<String> = done
            .iter()
            .filter(|u| {
                let files = Self::files_of(state, u);
                files.is_empty() || files.iter().any(|p| !self.docs_root.join(p).exists())
            })
            .cloned()
            .collect();
        if !redo.is_empty() {
            return redo;
        }
        // 2) 单元级归因（DR-16）：全树 lint 一次，只有落在**本单元自己文件里**的非 Link 违规
        //    才算证据失效。Link 类容忍——穿插/枢纽后写模式下，早期单元引用后期单元的枢纽文件
        //    在树未完整时必死链；真正的坏链接 run 末尾的全量 lint 兑底。
        if let Some(lint) = &self.lint {
            match lint(state) {
                Some(violations) => {
                    for u in &done {
                        let files = Self::files_of(state, u);
                        let bad = violations.iter().any(|v| {
                            if v.check == Check::Link {
                                return false;
                            }
                            let rel = v
                                .file
                                .strip_prefix(&self.docs_root)
                                .unwrap_or(v.file.as_path());
                            files
                                .iter()
                                .any(|f| Path::new(f) == rel)
                        });
                        if bad {
                            redo.push(u.clone());
                        }
                    }
                }
                // lint 本身挂了 → 无法机械归因，保守全重做（旧口径）
                None => redo = done,
            }
        }
        redo
    }

    fn resume_brief(&self, _state: &RunState, redo: &[String]) -> String {
        if redo.is_empty() {
            return String::new();
        }
        format!(
            "断点复核：以下单元已标 done，但产出文件缺失或 doc-lint 变红，必须整单元重写（不是补一句）：{}",
            redo.join("、")
        )
    }
}

#[cfg(test)]
mod tests {
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
        let resume = DocResume::new(tmp.path()).with_lint(|_| None);
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
            Some(vec![violation("a/x.md", Check::Link)])
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
            Some(vec![violation("a/x.md", Check::Entity)])
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
}
