//! 文档 lint（doc-lint 门）：机制面契约的机器检查（实体存在/必需节/链接/runbook/禁内容）。
//! ARC-R6 自单文件垂直切分：report 类型 / checks 检查器 / entities 实体校验 / tests。
//! 外部路径不变（pub use 层）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::Context as _;
use crate::error::Result;

pub use report::{Check, EntryType, LintReport, Severity, Violation};
pub use checks::{lint_docs, lint_docs_with_entities};
pub use entities::{EntityIndex, mentioned_entities};

mod checks;
mod entities;
mod report;

#[cfg(test)]
mod tests;
