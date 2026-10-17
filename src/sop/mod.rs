//! 规划与编码 SOP（P001/P005 · 内置 SOP）。
//!
//! - [`output`]：规划输出协议（完成标记 + verdict 块，decisions/008 同一机制家族）
//! - [`planning`]：Plan Agent + Review 循环状态机（零校验流转，decisions/008）
//! - [`coding_protocol`]：编码输出协议（阶段完成标记 + Review Report 块，P005）

pub mod coding_protocol;
pub mod output;
pub mod planning;
pub mod rpc_bridge;

pub use coding_protocol::{
    parse_review_report, parse_stage_complete, NextAction, ReviewReport, StageVerdict, BLOCKERS_DIR,
    CODING_DIR, DOCS_DIR, REPORTS_DIR, STAGES_FILE, STAGE_COMPLETE_TAG, TODO_FILE,
};

pub use output::{is_complete, parse_verdict, Verdict, COMPLETION_TAG};
pub use planning::{
    PlanningLoop, PlanAgentSession, ReviewSession, ReviewSessionFactory, StageReport, UserLink,
    PLAN_KICKOFF, REMEDIATION_PREFIX, REVIEW_INSTRUCTION,
};
pub use rpc_bridge::{RpcPlanSession, RpcReviewFactory, RpcReviewSession};
