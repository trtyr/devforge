//! `doc-from-zero` 的续跑接入（plan 010 · T004/T007）：声明本 SOP 的 done 证据与恢复语。
//!
//! 证据口径（010 D003）：一个单元算「真做完」= 它当初产出的文件都还在 + `doc-lint` 仍为 0 违规。
//! 只做机械可判定的事——文档写得好不好归审查，不归这里。

use std::path::{Path, PathBuf};

use crate::runstate::resume::{Resumable, done_units};
use crate::runstate::RunState;

/// 本 SOP 的注册名（同时是状态目录名）。
pub const DOC_SOP: &str = "doc-from-zero";

/// `doc-lint` 门卫：返回是否仍绿（真实现用 lint_docs_with_entities，测试注入假实现）。
pub type LintGate = Box<dyn Fn(&Path) -> bool + Send + Sync>;

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

    pub fn with_lint(mut self, f: impl Fn(&Path) -> bool + Send + Sync + 'static) -> Self {
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
        // 2) 全局门：doc-lint 变红 → 所有 done 单元保守重做（lint 报的是全库问题，
        //    无法机械归因到单元；宁重做，不假绿）
        if let Some(lint) = &self.lint
            && !lint(&self.docs_root)
        {
            redo = done;
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
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a/x.md");
        let st = state_with(
            &["unit-a", "unit-b"],
            json!({ "unit-a": ["a/x.md"], "unit-b": ["a/x.md"] }),
        );
        let resume = DocResume::new(tmp.path()).with_lint(|_| false);
        let mut redo = resume.verify_done(&st);
        redo.sort();
        assert_eq!(redo, vec!["unit-a".to_string(), "unit-b".to_string()]);
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
