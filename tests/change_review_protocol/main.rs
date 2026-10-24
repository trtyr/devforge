//! 审查输出协议硬化 —— 验收对账测试
//! change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r（计划 §5 验收标准 1–32）
//!
//! 黑盒口径：只经**公开 API** 观察行为——`ProdChangeSession` / `ChangeSession` /
//! `parse_verdict` / `parse_review_report` / `review_with_gate` / `CodingSopLoop` /
//! `PlanningLoop` / `classify` / 角色提示词常量 / 仓内文档与常量——不窥探内部实现；
//! 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//!
//! 与仓内 `src/**` 的 `#[cfg(test)]` 单测相互独立：那一组在实现内部就地断言，
//! 本组从**仓外**（`tests/` 集成测试）看公开面，专门证伪「三通道分层 / 空箱拒收 /
//! 尾部协议位」这三件事是否真的落到对外可观测的行为上。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use devforge::change::boundary::{BoundaryVerdict, classify};
use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession};
use devforge::change::state::CHANGE_SOP;
use devforge::config::manifest::{
    AgentManifest, CHANGE_CODING_AGENT_PROMPT, CHANGE_TEST_AGENT_PROMPT, ConfigManifest,
};
use devforge::error::{DevforgeError, Result};
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::coding::CodingSopLoop;
use devforge::sop::coding_protocol::parse_review_report;
use devforge::sop::output::{Verdict, parse_verdict};
use devforge::sop::planning::{
    PlanAgentSession, PlanningLoop, ReviewSession, ReviewSessionFactory, UserLink,
};
use devforge::sop::review_gate::{
    MAX_PROTOCOL_VIOLATIONS, MAX_REVIEW_INSTANCE_FAILURES, ReviewBudget, ReviewOutcome,
    review_with_gate,
};

// ---------------- 常量 ----------------

/// 本变更的基线提交（计划 §1.1）。
/// 口径：**本场基线**（`docs/plantree/changes/<change-id>/baseline.txt`）= `a36f6b7…`
/// → 守卫的扫描范围与引擎的越界门同口径（同一个基线）。
const BASELINE: &str = "a36f6b79dec2e681db8daed0da067f9abd47a9f5";

/// 计划 §4 的白名单（**24 个**文件 = coding 步 23 个 + 测试阶段 1 个；
/// coding_gate.rs 为操作员补录扩容：resume5 死因 F16 解析器修复落点）。
const PLAN_WHITELIST: &[&str] = &[
    "src/observability/mod.rs",
    "src/observability/heartbeat.rs",
    "src/observability/tombstone.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/engine/policy.rs",
    "src/engine/client.rs",
    "src/server/mod.rs",
    "src/server/handlers.rs",
    "src/server/tests/mod.rs",
    "src/change/orchestrator.rs",
    "src/change/session.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/sop/doc/runner.rs",
    "src/sop/doc/fix.rs",
    "tests/f6_observability/main.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context/main.rs",
    "tests/change_review_protocol/main.rs",
    "src/change/coding_gate.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

/// 计划 §4 白名单去掉唯一的测试阶段文件 = **coding 步 23 个**。
const CODING_WHITELIST: &[&str] = &[
    "src/observability/mod.rs",
    "src/observability/heartbeat.rs",
    "src/observability/tombstone.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/engine/policy.rs",
    "src/engine/client.rs",
    "src/server/mod.rs",
    "src/server/handlers.rs",
    "src/server/tests/mod.rs",
    "src/change/orchestrator.rs",
    "src/change/session.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/sop/doc/runner.rs",
    "src/sop/doc/fix.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context/main.rs",
    "tests/change_review_protocol/main.rs",
    "src/change/coding_gate.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

/// 引擎**不再**合成的占位文案（裁决 B：空箱拒收 + 追补）。
const PLACEHOLDER_ISSUE: &str = "测试阶段判定失败（未给出 issues 明细）";
/// 空箱拒收的追补话术锚点。
const NUDGE_ANCHOR: &str = "请补齐失败明细后重发";

/// 审查协议块：PASS。
const PASS: &str = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
/// 审查协议块：PASS + 收工（MVP coding 循环用）。
const PASS_FINISH: &str = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>";
/// 审查协议块：FAIL + fix，**不带** `<issues>`（= 协议违例，判负通道拿不到它）。
const FAIL_EMPTY: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>";
/// 审查协议块：**缺** `<stage-verdict>`（= 协议违例）。
const NO_VERDICT: &str = "我看了，没发现问题。（没有输出协议块）";
/// 审查协议块：FAIL + 非空 issues（= 真判负）。
const FAIL_WITH_ISSUES: &str =
    "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- tests/change_review_protocol.rs:1 断言失败\n</issues>";

// ---------------- 夹具 ----------------

mod guards;
mod protocol;
mod support;
