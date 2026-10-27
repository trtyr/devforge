# 勘察事实 — unit-src-17（src/change/session.rs + src/change/session/session_tests/{mod,plan}.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/session.rs.md`、
> `docs/contexts/src/change/session/session_tests/mod.rs.md`、
> `docs/contexts/src/change/session/session_tests/plan.rs.md` 与 `_meta/gaps.md`。
> 行号口径：本文件全部为当前工作区（ARC-R7 批 1 之后）实测行号。

## 文件一：src/change/session.rs（589 行）

- 文件头注释（`:1`-`:8`）：真实 `ChangeSession` 接线；本层「对话 + 解析 + 调机械门」；子会话是注入的 trait 对象。
- 模块声明与转发：`src/change/mod.rs:20` `pub mod session;`；`src/change/mod.rs:41` `pub use session::{ChangeCtx, ProdChangeSession};`。
- 文件尾：`:588` `#[cfg(test)]`、`:589` `mod session_tests;`（用例本体已外迁到 `src/change/session/session_tests/`）。

### import 块（`:10`-`:26`）

| 行 | 项 |
|---|---|
| `:10` | `std::path::{Path, PathBuf}` |
| `:12` | `change::boundary::{parse_route, replan_instruction, ImpactGap}` |
| `:13`-`:16` | `change::coding_gate::{changed_files, check_baseline_log, check_self_check, check_step_commits, parse_baseline_log, read_steps, BASELINE_LOG}` |
| `:17` | `change::gate0::DocLevel` |
| `:18` | `change::orchestrator::{ChangeSession, CodingTurn, PlanTurn, TestingTurn}` |
| `:19` | `change::plan_audit::review_instruction` |
| `:20` | `change::state::TestFailureContext` |
| `:21`-`:23` | `change::testing_gate::{check_regression_log, parse_acceptance_table, REGRESSION_LOG}` |
| `:24` | `error::{Context as _, DevforgeError, Result}` |
| `:25` | `sop::coding_protocol::{parse_review_report, parse_stage_complete, StageVerdict}` |
| `:26` | `sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink}` |
| `:27` | `sop::review_gate::{outcome_of_review_report, review_with_gate, ReviewBudget}` |

### 公开条目

| 符号 | 位置 | 形态 |
|---|---|---|
| `ChangeCtx` | `:30` | `#[derive(Debug, Clone)] pub struct`，字段 `repo: PathBuf`(`:32`)、`blackboard: PathBuf`(`:34`)、`change_dir: PathBuf`(`:36`)、`change_id: String`(`:37`)、`baseline: String`(`:38`)、`requirement: String`(`:40`)、`doc_level: DocLevel`(`:41`)，全 `pub` |
| `ChangeCtx::plan_path` | `:45` | `pub fn(&self) -> PathBuf` = `change_dir.join("change-plan.md")` |
| `ChangeCtx::acceptance_path` | `:49` | `pub fn(&self) -> PathBuf` = `blackboard.join("testing/acceptance.md")` |
| `ProdChangeSession` | `:55` | `pub struct`，8 个 `pub` 字段：`plan_agent`(`:56`)、`plan_review`(`:57`)、`coding_agent`(`:58`)、`coding_review`(`:59`)、`test_agent`(`:60`)、`test_review`(`:61`)、`ctx`(`:62`)、`plan_review_instances: usize`(`:64`)；无 derive |
| `plan_kickoff` | `:69` | `pub fn(&ChangeCtx) -> String` |
| `baseline_kickoff` | `:89` | `pub fn(&ChangeCtx) -> String` |
| `coding_review_instruction` | `:103` | `pub fn(&ChangeCtx, &[String]) -> String` |
| `testing_review_instruction` | `:125` | `pub fn(&ChangeCtx) -> String` |
| `coding_kickoff` | `:146` | `pub fn(&ChangeCtx, Option<&TestFailureContext>) -> String` |
| `MAX_EMPTY_ISSUES_NUDGES` | `:172` | `pub const usize = 3` |
| `ISSUES_NUDGE` | `:175` | `pub const &str`（单行常量，正文含 `<issues>` 与「请补齐失败明细后重发」） |
| `testing_kickoff` | `:179` | `pub fn(&ChangeCtx) -> String` |
| `plan_exists` | `:579` | `pub fn(&ChangeCtx) -> bool` = `ctx.plan_path().is_file()` |
| `baseline_log_path` | `:584` | `pub fn(&Path) -> PathBuf` = `blackboard.join(coding_gate::BASELINE_LOG)` |

### 私有条目

| 符号 | 位置 | 形态 |
|---|---|---|
| `evidence_exit_code` | `:204` | `fn(&Path) -> Option<i32>`：`read_to_string` → `parse_baseline_log` → `ev.exit_code` |
| `MAX_MARKER_NUDGES` | `:210` | `const usize = 3` |
| `MARKER_NUDGE` | `:213` | `const &str`（含 `<stage-complete>N` 样板） |
| `require_marker_nudged` | `:218` | `fn(stage: &str, first_reply: String, redeliver: impl FnMut(&str) -> Result<String>) -> Result<String>` |
| `issues_of` | `:238` | `fn(&str) -> Vec<String>`：找 `<issues>`…`</issues>`（无闭合则到串尾），逐行 `trim` + 去 `-`/`*` 前缀 + 丢空行 |
| `review_once` | `:264` | `fn(&mut dyn ReviewSessionFactory, role: &str, instruction: &str) -> Result<(ReviewReport, usize)>` |
| `ProdChangeSession::run_plan_with` | `:292` | `fn(&mut self, &mut dyn UserLink, &str) -> Result<PlanTurn>`，plan/amend/fix 共用 |

### `impl ChangeSession for ProdChangeSession`（`:331`）

| trait 方法 | 位置 | 主要调用 |
|---|---|---|
| `run_plan` | `:332` | `plan_kickoff` → `run_plan_with` |
| `amend_plan` | `:337` | `replan_instruction(gaps, roundtrip)` + 仓/计划路径后缀 → `run_plan_with` |
| `fix_plan` | `:352` | 打回消息（含 `ask-user-question` / `用户裁决` 字样）→ `run_plan_with` |
| `run_coding` | `:375` | 基线轮 → `check_baseline_log` → coding 轮 → `parse_route` 短路 → `require_marker_nudged` → `check_self_check` → `read_steps` → `check_step_commits` → `changed_files` → `review_once` |
| `run_testing` | `:495` | `testing_kickoff` → 循环（`parse_route` + `issues_of` + 空箱追补）→ `require_marker_nudged` → `check_regression_log(bb,true)` → 读 `acceptance_path` → `parse_acceptance_table` → 每行 `plan_audit::path_refs` 非空 → `review_once` |
| `plan_reviews` | `:565` | 返回 `plan_review_instances` |
| `plan_facts` | `:569` | 读 `plan_path` → `plan_audit::plan_facts(&text)` |

### 行为要点（只记事实，判定理由见文档）

- `run_plan_with`：`converse` → `require_marker_nudged("change-plan-agent", …)`（`:294`-`:296`）→ 读计划全文（读不到 → `DevforgeError::Quality`，`:298`-`:300`）→ `audit_verdicts` 不干净 → **直接** `Ok(PlanTurn::Fail{issues})`，不进审查（`:304`-`:309`）→ `audit_plan_file`（`:310`-`:315`）→ `review_once(… "change-plan-review" …)`（`:316`-`:320`）→ 写 `plan_review_instances`（`:321`）→ verdict Pass/Fail 映射（`:322`-`:327`）。
- `run_coding`：基线轮标记追补用 stage 名 `"change-coding-agent（基线轮）"`（`:382`-`:386`）；基线门 tracing 事件 `gate.baseline`（`:389`-`:408`），红路径 `exit_code = evidence_exit_code(...)`（`:404`）；`parse_route(&reply) == Some("plan")` → 立即返回 `CodingTurn{reviews: 0}` 且 `changed = changed_files(...).unwrap_or_default()`（`:415`-`:422`）；`gate.self_check`（`:428`-`:445`）；`gate.step_commits` 带 `steps`/`commits` 字段（`:449`-`:473`）；`changed_files`（`:474`）；审查判负 → `Err(DevforgeError::Quality(...))`（`:488`-`:491`）。
- `run_testing`：循环里 `parse_route` 命中 `coding`/`plan` 且 `issues_of` 非空才 break（`:501`-`:507`）；空箱 `nudges += 1`，`> MAX_EMPTY_ISSUES_NUDGES` → `Quality`（`:509`-`:516`），否则下一条消息换成 `ISSUES_NUDGE`（`:515`）；路由命中后映射 `TestingTurn::RouteCoding` / `RoutePlan`（`:520`-`:525`）；通过路径再要求标记（`:526`-`:528`）→ `check_regression_log(bb, true)`（`:530`）→ 读对账表（`:532`-`:535`，解析 `:536`，取 `done` `:537`）→ 每行 `path_refs` 为空即 `Quality`（`:539`-`:546`）→ `review_once(… "change-test-review")`（`:548`-`:552`）。
- `review_once`：先 `crate::observability::next_review_round(role)`（`:270`）→ `set_step(&format!("review:{role}"))`（`:272`）→ `tracing::info!(event="review.open", role, round)`（`:273`-`:279`）→ `review_with_gate(factory, 0, |_| instruction.to_string(), &mut budget, |text| Ok(outcome_of_review_report(parse_review_report(text)?)))`（`:280`-`:287`）。
- `require_marker_nudged`：`while parse_stage_complete(&reply).is_none()`（`:225`），每次 `nudges += 1`（`:226`），`nudges > MAX_MARKER_NUDGES` → `Quality`（文案含「连续 {nudges} 次」「已追补 3 次」与 `crate::sop::coding::tail(&reply)`，`:227`-`:231`），否则 `reply = redeliver(MARKER_NUDGE)?`（`:233`）。

### 外部依赖 / 消费方

- 生产依赖的外部 crate：无（只有 `std` + crate 内模块）；测试用 `tempfile`（`Cargo.toml` dev-dependency）
- 上游：`src/server/change_engine.rs:12` 引入；`:180`-`:189` 直构 `ProdChangeSession`（六个子会话 + `ctx.clone()` + `plan_review_instances: 0`）；`:246`-`:255` `build_ctx` 从 `ChangePrep` 组装 `ChangeCtx`（字段逐一对应）
- 仓外黑盒：`tests/change_failure_context/main.rs:23` 用 `ChangeCtx` / `ProdChangeSession` / `coding_kickoff`
- 被引用的 crate 内符号位置：`plan_audit::{audit_verdicts :98, audit_plan_file :442, plan_facts :387, path_refs :318, review_instruction :409}`、`testing_gate::{REGRESSION_LOG :18, FAILURE_BLOCK_TITLE :137, coding_failure_block :142}`、`coding_gate::{BASELINE_LOG :16, SELF_CHECK_LOG :18}`、`sop::coding::tail :24`、`observability::{set_step :179, next_review_round :263}`、`sop::review_gate::{MAX_PROTOCOL_VIOLATIONS :16 = 3, MAX_REVIEW_INSTANCE_FAILURES :19 = 3, ReviewBudget :26, outcome_of_review_report :60, review_with_gate :78}`

## 文件二：src/change/session/session_tests/mod.rs（196 行）

- 文件头注释（`:1`-`:2`）：共享夹具，ARC-R6 自 `session.rs mod tests` 平移；子 mod 经 `use super::*` 继承名字。
- `use super::*;`（`:4`）+ `sop::planning::{ReviewSession, ReviewSessionFactory}`（`:5`）；`std::cell::RefCell`/`std::collections::VecDeque`/`std::process::Command`/`std::rc::Rc`（`:6`-`:9`）。
- 类型：
  - `NoUser`（`:11`）：`impl UserLink`（`:12`-`:15`），`ask` 恒返回 `Err(DevforgeError::System("单测不应问用户：{q}"))`（`:13`）。
  - `Agent`（`:18`）：字段 `replies: VecDeque<String>`、`seen: Rc<RefCell<Vec<String>>>`；`Agent::new(&[&str])`（`:23`）；`impl PlanAgentSession::converse`（`:30`-`:34`）把 message 推入 `seen`、`pop_front` 取回复，耗尽即 `expect("Agent 脚本耗尽")` panic。
  - `OneReview`（`:37`）`impl ReviewSession::review`（`:41`-`:45`）：同样见闻记录 + 队列耗尽 panic（`"Review 脚本耗尽"`）。
  - `ReviewFactory`（`:47`）、`ReviewFactory::new(&[&str])`（`:52`）；`impl ReviewSessionFactory::open`（`:59`-`:66`）：每个 `open` 返回一个共享同一队列/见闻表的新 `OneReview`。
- 常量：`PLAN_PASS`（`:68`）= `<stage-verdict>PASS</stage-verdict>\n<next>advance</next>`；`PLAN_FAIL`（`:69`）= FAIL + `<next>fix</next>` + `<issues>` 一条「影响面漏了调用方」。
- `plan_text()`（`:71`-`:81`）：六块计划样板（需求复述 / `[自决]` 歧义条 / 影响面 `src/a.rs:1` / 要改的表 / 白名单 `src/a.rs` / 验收「给定 x=1 → 返回 2」 / 风险）。
- `Fixture{tmp, ctx}`（`:84`）；`fixture(with_git: bool)`（`:89`-`:141`）：临时目录下建 `repo/src/a.rs`、`bb/`、`repo/docs/plantree/changes/change-1/change-plan.md`；`with_git` 为真时 `git init` + `add -A` + 以 `user.name=t`/`user.email=t@localhost` 提交基线并把 `rev-parse HEAD` 存进 `ctx.baseline`；`ctx.requirement = "把 a 改成 b"`、`doc_level: DocLevel::None`。
- `git(repo, args)`（`:143`-`:151`）：`Command::new("git")` + `current_dir`，失败即 panic 带 stderr。
- `coding_ready(f) -> usize`（`:157`-`:192`）：写 `evidence/baseline-test.log`、`evidence/self-check.log`（均 `command: cargo test` / `exit_code: 0`）、`impl-steps.md`（`## 步骤 1 改 a`），改 `src/a.rs` 并以 `change: change-1 step 1` 提交，返回 `1`。
- 子 mod 声明：`mod plan;`（`:195`）、`mod runtime;`（`:196`）。

## 文件三：src/change/session/session_tests/plan.rs（362 行）

- 文件头注释（`:1`）：plan 段会话测试（ARC-R6 平移）。`use super::*;`（`:3`）。
- 15 个 `#[test]`：

| 用例 | 位置 | 脚本/断言事实 |
|---|---|---|
| `marker_nudge_redelivers_once_then_accepts` | `:6` | 追补回调断言消息含 `<stage-complete>N`；断言 `calls == 1` |
| `marker_nudge_gives_up_after_three_redeliveries` | `:19` | 恒不带标记 → err；断言 `calls == 3`、文案含「连续 4 次」「已追补 3 次」 |
| `plan_facts_are_read_back_from_plan_file` | `:32` | `plan_facts()` → 验收 `["给定 x=1 → 返回 2"]`、白名单 `["src/a.rs"]` |
| `plan_pass_runs_audit_and_review_with_audit_injected` | `:50` | PASS；kickoff 含 change-id 与 `change-plan.md`；审查指令含「勿质疑、勿重算」与 `path:line` |
| `plan_fail_carries_issues_back` | `:77` | FAIL → `PlanTurn::Fail{issues}` 含「调用方」 |
| `plan_without_marker_is_quality_failure` | `:96` | 四个无标记回复 → Quality，文案含 `stage-complete` 与「已追补 3 次」 |
| `plan_with_must_ask_without_answer_fails_before_review` | `:139` | 计划里 `[必须裁决]` 无留痕 → Fail；审查工厂队列为空且断言 `seen_review` 为空（不进审查） |
| `plan_with_must_ask_and_answer_passes_and_review_sees_verdict_facts` | `:165` | 带 `用户裁决：算一个词` → Pass；审查 1 次；指令含「裁决门事实」「留痕核对」「分类恰当性」 |
| `fix_plan_message_carries_the_ask_instruction` | `:196` | 打回消息含 `ask-user-question` 与 `用户裁决` |
| `plan_kickoff_carries_the_ask_contract` | `:217` | kickoff 含 `ask-user-question` 与 `用户裁决` |
| `coding_kickoff_without_context_has_no_failure_block` | `:227` | 不含 `FAILURE_BLOCK_TITLE`、不含「失败明细」；首行/末句锚点 |
| `coding_kickoff_with_context_carries_the_failure_details_verbatim` | `:247` | 含标题行、两条明细逐字（`tests/change_ask_ring.rs:557`、`接口返回 500`）；不含「请自己定位原因」「本消息不提供文件位置」「<位置略>」 |
| `coding_kickoff_with_context_only_appends_the_block` | `:273` | `with == format!("{plain}\n\n{}", coding_failure_block(&ctx))` |
| `empty_issues_route_is_rejected_then_accepted_verbatim` | `:293` | 第 1 轮空箱 → 追一条；第 2 轮补齐 → `RouteCoding{failures}` 两条逐字；消息数 2、「请补齐失败明细后重发」 |
| `repeated_empty_issues_routes_fail_fast` | `:335` | 4 轮空箱 → err 含「未给出 issues 明细」「4 次」，且不含占位文案 |

- 辅助函数：`write_plan(f, body)`（`:120`-`:123`）替换计划里的 `[自决]` 行；`plan_session(f, agent, review)`（`:125`-`:136`）造 `ProdChangeSession`（其余四个会话给空脚本假实现）。
