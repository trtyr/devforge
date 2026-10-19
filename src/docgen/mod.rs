//! 文档生成与门禁（P008 doc-from-zero 的引擎侧能力）。
//!
//! 与 SOP 无关的纯函数层：清单抽取、doc-lint、覆盖对账都落在这里，
//! 由引擎在"交审前/审查时"调用，结果作为确定性证据注入审查输入（D012）。

pub mod inventory;
pub mod lint;
