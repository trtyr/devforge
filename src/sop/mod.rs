//! 规划与编码 SOP（P001/P005 · 内置 SOP）。
//!
//! - [`output`]：规划输出协议（完成标记 + verdict 块，decisions/008 同一机制家族）
//! - [`planning`]：Plan Agent + Review 循环状态机（零校验流转，decisions/008）
//! - [`coding_protocol`]：编码输出协议（阶段完成标记 + Review Report 块，P005）
//! - [`coding`]：CodingSopLoop 状态机（阶段推进/打回/树完成，P005）
//! - [`testing_protocol`]：测试阶段布局与 findings 台账协议（P007）
//! - [`testing`]：TestingSopLoop 状态机（阶段推进/reroute 路由/doc-fix 移交，P007）
//! - [`doc_fix`]：DocFixSopLoop 状态机（写文档模式 + doc-review 内置审查轮，P007 D004）
//! - [`doc_from_zero`]：DocSopLoop 状态机（门0→单元滚动→审查，P008）
//! - [`doc_protocol`]：doc-from-zero 三块协议（doc-complete / doc-verdict / route，P008）
//! - [`review_gate`]：审查输出的三通道机制（判负 / 协议违例 / 实例失败，D026）

pub mod coding;
pub mod coding_protocol;
pub mod doc_fix;
pub mod doc_from_zero;
pub mod doc_incremental;
pub mod doc_protocol;
pub mod doc_resume;
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

pub use doc_from_zero::{
    DocReport, DocSopLoop, DOCS_DIR as DOCGEN_DOCS_DIR, DOC_CONTINUE_NUDGE, DOC_REMEDIATION_PREFIX,
};
pub use doc_protocol::{parse_doc_complete, parse_doc_review, DocRoute, DocVerdict, DocReviewReport};
