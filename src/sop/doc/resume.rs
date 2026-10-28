//! `doc-from-zero` 的续跑接入（plan 010 · T004/T007）：声明本 SOP 的 done 证据与恢复语。
//!
//! 证据口径（010 D003 + DR-16 单元级归因）：一个单元算「真做完」= 它当初产出的文件都还在，
//! 且落在它自己文件里的 lint 违规只剩 Link 类（穿插/枢纽后写模式下，指向未来单元的死链
//! 在树未完整时不可避免，由 run 末尾的全量 lint 兕底）。
//!
//! 只做机械可判定的事。

use std::path::{Path, PathBuf};

use crate::docgen::lint::{Check, Severity, Violation};
use crate::runstate::resume::{Resumable, done_units};
use crate::runstate::RunState;

/// 本 SOP 的注册名（同时是状态目录名）。
pub const DOC_SOP: &str = "doc-from-zero";

/// `doc-lint` 门卫：跑一次全树 lint，返回违规清单；`Err(原因)` = lint 本身挂了
///（保守全重做）。自审 major：旧签名 `Option` 把错误原因吞在闭包里——
/// resume 决策（全重做）无法落埋点说明为什么，排查只能靠猜。
pub type LintGate = Box<dyn Fn(&RunState) -> Result<Vec<Violation>, String> + Send + Sync>;

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

    pub fn with_lint(
        mut self,
        f: impl Fn(&RunState) -> Result<Vec<Violation>, String> + Send + Sync + 'static,
    ) -> Self {
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
        // 1) 逐个单元：没登记产出 = 无证据；登记了但文件不在 = 证据失效。
        //    自审 major：重做决策必须落埋点——否则续跑全重做时无法归因。
        let mut redo: Vec<String> = Vec::new();
        for u in &done {
            let files = Self::files_of(state, u);
            if files.is_empty() {
                tracing::warn!(
                    component = "doc-resume",
                    unit = %u,
                    "断点复核：单元未登记产出文件，判证据失效（重做）"
                );
                redo.push(u.clone());
                continue;
            }
            let missing: Vec<String> = files
                .iter()
                .filter(|p| !self.docs_root.join(p).exists())
                .cloned()
                .collect();
            if !missing.is_empty() {
                tracing::warn!(
                    component = "doc-resume",
                    unit = %u,
                    missing = ?missing,
                    "断点复核：单元产出文件缺失，判证据失效（重做）"
                );
                redo.push(u.clone());
            }
        }
        if !redo.is_empty() {
            return redo;
        }
        // 2) 单元级归因（DR-16）：全树 lint 一次，只有落在**本单元自己文件里**的非 Link 违规
        //    才算证据失效。Link 类容忍——穿插/枢纽后写模式下，早期单元引用后期单元的枢纽文件
        //    在树未完整时必死链；真正的坏链接 run 末尾的全量 lint 兑底。
        if let Some(lint) = &self.lint {
            match lint(state) {
                Ok(violations) => {
                    for u in &done {
                        let files = Self::files_of(state, u);
                        // 自审 major：只认 Fail 级违规——Warn 级（同名异写建议、
                        // 代码块过长）是改进提示，不是证据失效。
                        let bad: Vec<String> = violations
                            .iter()
                            .filter(|v| {
                                if v.severity != Severity::Fail || v.check == Check::Link {
                                    return false;
                                }
                                let rel = v
                                    .file
                                    .strip_prefix(&self.docs_root)
                                    .unwrap_or(v.file.as_path());
                                files.iter().any(|f| Path::new(f) == rel)
                            })
                            .map(|v| format!("{}:{} {:?}", v.file.display(), v.line, v.check))
                            .collect();
                        if !bad.is_empty() {
                            tracing::warn!(
                                component = "doc-resume",
                                unit = %u,
                                violations = ?bad,
                                "断点复核：单元自有文件存在 Fail 级 lint 违规，判证据失效（重做）"
                            );
                            redo.push(u.clone());
                        }
                    }
                }
                // lint 本身挂了 → 无法机械归因，保守全重做（旧口径）；
                // 原因现在能落埋点了（LintGate 带回 Err(原因)）。
                Err(reason) => {
                    tracing::warn!(
                        component = "doc-resume",
                        reason = %reason,
                        "断点复核：doc-lint 本身失败，保守全部重做"
                    );
                    redo = done;
                }
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
#[path = "resume_unit_tests.rs"]
mod tests;
