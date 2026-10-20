//! 变更 SOP（P009）的引擎侧实现。
//!
//! 与 MVP（从零做需求）共用 plan / coding / testing 三段骨架，差别集中在：
//! - **门 0**：基线（diff 锚点）、`doc_level` 档位、`change-id`；
//! - **plan**：用 codegraph 把需求落到文件与符号，产出可机械校验的改动边界；
//! - **coding / testing**：白名单、越界二分、回归前置与验收对账。
//!
//! 本模块只放引擎侧（确定性）逻辑；agent 的产出与判断由 profile 提示词约束。

pub mod change_id;
pub mod docs;
pub mod gate0;

pub use change_id::{assign_change_id, change_root, make_change_id, slugify, validate_change_id};
pub use docs::{DocAction, apply as apply_doc_level, essential_ready};
pub use gate0::{Baseline, DocLevel, ensure_baseline};
