//! doc 域垂直同居（ARC-R3）：doc-from-zero 及其配套协议/断点/增量/修补全居于此。
//!
//! - [`protocol`]：三块输出协议（doc-complete / doc-verdict / route，P008）
//! - [`resume`]：断点与续跑（done 证据 + 单元级归因，DR-16）
//! - [`incremental`]：增量文档（影响面 / 基线推进，D021）
//! - [`fix`]：DocFixSopLoop 状态机（doc-review 内置审查轮，P007 D004）
//!
//! runner 与接线（`runner` / `wiring`）在本任务后续批次落位。

pub mod fix;
pub mod incremental;
pub mod protocol;
pub mod resume;
