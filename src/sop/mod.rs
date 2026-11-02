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
//! - [`marker`]：完成标记追补的计数纪律单点（bounded/unlimited/reset/bump）

pub mod coding;
pub mod coding_protocol;
pub mod doc;
pub mod marker;
pub mod output;
pub mod planning;
pub mod review_gate;
pub mod rpc_bridge;
pub mod testing;
pub mod testing_protocol;

/// 有「续跑能力」的 SOP 名单（R7-P63）。
///
/// 断点文件对**任意** sop 名写入（重启记账 attempt>1 即写），但没实现续跑的
/// SOP 根本没有「续跑」能力——它的断点只是历史遗留数据，对它报
/// `resumed: true` 是对外谎报（日志/审计/响应体三处都吃这个字段）。
/// 名单内两种续跑形态并存：doc-from-zero 走 Resumable trait（sop/doc/resume），
/// change 走黑板续跑（change/state 的 load_strict + resume_brief）。
/// 业务词住业务层（runstate 引擎面禁 SOP id——守卫 trait_surface_stays_agnostic），
/// 判定由调用方把本名单成员判定注入 start_gate。新增续跑实现时把名字
/// 加进来；审计面 `grep -rn "impl Resumable"` 与 change 黑板续跑入口与此名单保持一致。
pub fn resumable_sops() -> &'static [&'static str] {
    &["doc-from-zero", "change"]
}

pub use coding::{
    CodingReport, CodingSopLoop, CODING_REMEDIATION_PREFIX, CONTINUE_NUDGE, coding_kickoff,
    coding_review_instruction,
};

pub use coding_protocol::{
    parse_review_report, parse_stage_complete, NextAction, ReviewReport, StageVerdict, BLOCKERS_DIR,
    CODING_DIR, DOCS_DIR, REPORTS_DIR, STAGES_FILE, STAGE_COMPLETE_TAG, TODO_FILE,
};

pub use marker::{MarkerNudge, MarkerStep, MAX_MARKER_NUDGES};
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
