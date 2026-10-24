//! 规划与编码 SOP（P001/P005 · 内置 SOP）。
//!
//! - [`output`]：规划输出协议（完成标记 + verdict 块，decisions/008 同一机制家族）
//! - [`planning`]：Plan Agent + Review 循环状态机（零校验流转，decisions/008）
//! - [`coding_protocol`]：编码输出协议（阶段完成标记 + Review Report 块，P005）
//! - [`coding`]：CodingSopLoop 状态机（阶段推进/打回/树完成，P005）
//! - [`testing_protocol`]：测试阶段布局与 findings 台账协议（P007）
//! - [`testing`]：TestingSopLoop 状态机（阶段推进/reroute 路由/doc-fix 移交，P007）
//! - [`doc`]：doc 域垂直同居（ARC-R3：runner/协议/断点/增量/修补/接线同居 sop/doc/）
//! - [`review_gate`]：审查输出的三通道机制（判负 / 协议违例 / 实例失败，D026）

pub mod coding;
pub mod coding_protocol;
pub mod doc;
pub mod output;
pub mod planning;
pub mod review_gate;
pub mod rpc_bridge;
pub mod testing;
pub mod testing_protocol;

pub use coding::{
    CodingReport, CodingSopLoop, CODING_REMEDIATION_PREFIX, CONTINUE_NUDGE, coding_kickoff,
    coding_review_instruction,
};

pub use coding_protocol::{
    parse_review_report, parse_stage_complete, NextAction, ReviewReport, StageVerdict, BLOCKERS_DIR,
    CODING_DIR, DOCS_DIR, REPORTS_DIR, STAGES_FILE, STAGE_COMPLETE_TAG, TODO_FILE,
};

pub use output::{is_complete, parse_verdict, Verdict, COMPLETION_TAG};
pub use planning::{
    PlanningLoop, PlanAgentSession, ReviewSession, ReviewSessionFactory, StageReport, UserLink,
    PLAN_KICKOFF, REMEDIATION_PREFIX, REVIEW_INSTRUCTION,
};
pub use review_gate::{
    MAX_PROTOCOL_VIOLATIONS, MAX_REVIEW_INSTANCE_FAILURES, ReviewBudget, ReviewOutcome,
    review_with_gate,
};
pub use rpc_bridge::{RpcPlanSession, RpcReviewFactory, RpcReviewSession};

pub use doc::fix::DocFixSopLoop;
pub use doc::prompts::{DOC_REMEDIATION_PREFIX, DOCS_DIR as DOCGEN_DOCS_DIR};
pub use doc::protocol::{parse_doc_complete, parse_doc_review, DocRoute, DocVerdict, DocReviewReport};
pub use doc::runner::DocReport;
