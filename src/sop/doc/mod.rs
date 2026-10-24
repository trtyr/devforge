//! doc 域垂直同居（ARC-R3）：doc-from-zero 及其配套协议/断点/增量/修补全居于此。
//!
//! - [`protocol`]：三块输出协议（doc-complete / doc-verdict / route，P008）
//! - [`resume`]：断点与续跑（done 证据 + 单元级归因，DR-16）
//! - [`incremental`]：增量文档（影响面 / 基线推进，D021）
//! - [`fix`]：DocFixSopLoop 状态机（doc-review 内置审查轮，P007 D004）
//! - [`prompts`]：产出目录约定 + 三段提示词（R4 外置 assets 前的居所）
//! - [`state`] / [`evidence`]：断点状态与产出证据（plan 010）
//! - [`runner`]：run_pipeline 流水线（ARC-R1 后唯一 runner；里程碑 target 锚定本模块）

pub mod evidence;
pub mod fix;
pub mod incremental;
pub mod prompts;
pub mod protocol;
pub mod resume;
pub mod runner;
pub mod state;

#[cfg(test)]
pub(crate) mod mocks;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod runner_tests;

pub use prompts::{DOC_REMEDIATION_PREFIX, DOCS_DIR, META_DIR};
pub use runner::{AuditProvider, DocReport, run_pipeline};
