//! 规划 SOP（P001 · 第一个内置 SOP）。
//!
//! - [`output`]：统一输出协议（完成标记 + verdict 块，decisions/008 同一机制家族）
//! - [`planning`]：Plan Agent + Review 循环状态机（零校验流转，decisions/008）

pub mod output;
pub mod planning;
pub mod rpc_bridge;

pub use output::{is_complete, parse_verdict, Verdict, COMPLETION_TAG};
pub use planning::{
    PlanningLoop, PlanAgentSession, ReviewSession, ReviewSessionFactory, StageReport, UserLink,
    PLAN_KICKOFF, REMEDIATION_PREFIX, REVIEW_INSTRUCTION,
};
pub use rpc_bridge::{RpcPlanSession, RpcReviewFactory, RpcReviewSession};
