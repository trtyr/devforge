//! 变更 SOP（P009）的引擎侧实现。
//!
//! 与 MVP（从零做需求）共用 plan / coding / testing 三段骨架，差别集中在：
//! - **门 0**：基线（diff 锚点）、`doc_level` 档位、`change-id`；
//! - **plan**：用 codegraph 把需求落到文件与符号，产出可机械校验的改动边界；
//! - **coding / testing**：白名单、越界二分、回归前置与验收对账。
//!
//! 本模块只放引擎侧（确定性）逻辑；agent 的产出与判断由 profile 提示词约束。

pub mod boundary;
pub mod change_id;
pub mod coding_gate;
pub mod docs;
pub mod gate0;
pub mod history;
pub mod orchestrator;
pub mod plan_audit;
pub mod prepare;
pub mod report;
pub mod session;
pub mod state;
pub mod testing_gate;

pub use boundary::{
    BoundaryVerdict, ImpactGap, MAX_ROUNDTRIPS, check_roundtrip, classify as classify_boundary,
    merge_gaps_into_whitelist, replan_instruction,
};
pub use change_id::{assign_change_id, change_root, make_change_id, slugify, validate_change_id};
pub use coding_gate::{SELF_CHECK_LOG, check_baseline_log, check_self_check, check_step_commits, read_steps};
pub use docs::{DocAction, apply as apply_doc_level, essential_ready};
pub use gate0::{Baseline, DocLevel, ensure_baseline};
pub use history::{append_roundtrip, history_path};
pub use orchestrator::{
    ChangeOutcome, ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change,
};
pub use plan_audit::{PlanAudit, audit as audit_plan, review_instruction};
pub use prepare::{ChangePrep, prepare as prepare_change};
pub use report::{
    AcceptanceSummary, ChangeReport, DiffStats, diff_stats, summarize_acceptance, write_report,
};
pub use session::{ChangeCtx, ProdChangeSession};
pub use state::{CHANGE_SOP, ChangePhase, ChangeState, TestFailureContext};
pub use testing_gate::{
    AcceptanceMatch, FAILURE_BLOCK_TITLE, check_regression_claim, check_regression_log,
    coding_failure_block, failure_details, reconcile_acceptance, route_back_message,
};
