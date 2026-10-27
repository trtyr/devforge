# 勘察事实 — unit-src-16（src/change/report.rs + src/change/session.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/report.rs.md`、`docs/contexts/src/change/session.rs.md` 与 `_meta/gaps.md`。

## 文件一：src/change/report.rs

- 模块声明：`src/change/mod.rs:19` `pub mod report;`；`src/change/mod.rs:38`-40 `pub use report::{AcceptanceSummary, ChangeReport, DiffStats, diff_stats, summarize_acceptance, write_report};`。
- 文件头注释：变更报告（P009 T012）——**引擎写，不派 agent**；只放可复算的东西（墙钟/diff 统计/审查轮次/往返/用量/验收对账）；对账没过时报告里必须一眼看出来。

### 公开条目

| 符号 | 位置 | 类型 |
|---|---|---|
| `DiffStats` | `#13` | `#[derive(Debug,Clone,Default,PartialEq,Eq)] pub struct { pub files:usize, pub insertions:usize, pub deletions:usize }` |
| `AcceptanceSummary` | `#21` | `#[derive(Debug,Clone,PartialEq,Eq)] pub struct { pub total:usize, pub done:usize, pub missing:Vec<String> }` |
| `AcceptanceSummary::passed` | `#28` | `pub fn(&self)->bool` = `missing.is_empty() && total > 0` |
| `ChangeReport` | `#35` | `pub struct { change_id, run_id, baseline_sha, wall_secs, diff, review_rounds, roundtrips, usage, acceptance }` |
| `ChangeReport::render` | `#49` | `pub fn(&self)->String`：markdown 报告 |
| `diff_stats` | `#95` | `pub fn(&Path,&str)->Result<DiffStats>`：`git diff --numstat <baseline>` |
| `summarize_acceptance` | `#125` | `pub fn(&[String],&[String])->AcceptanceSummary` |
| `write_report` | `#139` | `pub fn(&Path,&str,&ChangeReport)->Result<PathBuf>`：写 `docs/plantree/changes/<id>/report.md` |

### 行为/测试事实（6 例）

- `render_carries_every_required_field`（`#180`）：id/run/基线/墙钟/diff/审查轮次/往返/用量/「验收对账: 通过」全在。
- `failed_reconciliation_is_visible_in_report`（`#198`）：未通过 + 缺 1 条 + `## 未对账的验收标准` + 条目原文。
- `empty_criteria_is_reported_as_anomaly`（`#212`）：`total=0` → 「无验收标准」。
- `summarize_counts_missing`（`#223`）：total/done/missing 计数与 `passed()` 为假。
- `diff_stats_reads_numstat`（`#234`）：改 1 文件 + 新增 1 文件（新增需先入索引）→ files=2、insertions=3、deletions=0。
- `write_report_creates_directory_and_file`（`#282`）：建目录 + 写 `report.md`、路径以 `docs/plantree/changes/<id>/report.md` 结尾。

### crate 内调用点

- `src/change/orchestrator.rs:14` 引入 `report` / `AcceptanceSummary` / `ChangeReport` / `summarize_acceptance`；`:151` `ChangeOutcome.report` 字段；`:309` 验收汇总判缺项；`:371` `diff_stats(...).unwrap_or_default()`；`:372` `summarize_acceptance`；`:373` 组装 `ChangeReport`。
- `src/server/change_engine.rs:11` 引入 `write_report`；`:56` `finalize` 写报告（`:54` 用量由 `usage::from_session_dirs` 汇总、`:55` 墙钟 elapsed）；`:282` 单测引用三个类型。
- `src/change/mod.rs:19/38` 声明与转发。

### 外部依赖

- `crate::error::{DevforgeError, Result}`（`src/change/report.rs:8`）；`std::path::Path`、`std::process::Command`（git）；无外部 crate；测试用 `tempfile`。

## 文件二：src/change/session.rs

- 文件头注释：change SOP 的真实 `ChangeSession`（P009 接线）——本层只做「对话 + 解析 + 调机械门」，判断在提示词与机械门里；子会话全部是注入的 trait 对象。
- 模块声明：`src/change/mod.rs:20` `pub mod session;`；`src/change/mod.rs:41` `pub use session::{ChangeCtx, ProdChangeSession};`。

### 公开条目

| 符号 | 位置 | 类型 |
|---|---|---|
| `ChangeCtx` | `#30` | `#[derive(Debug,Clone)] pub struct { repo, blackboard, change_dir, change_id, baseline, requirement, doc_level }` |
| `ChangeCtx::plan_path` | `#45` | `pub fn(&self)->PathBuf`：`change_dir/change-plan.md` |
| `ChangeCtx::acceptance_path` | `#49` | `pub fn(&self)->PathBuf`：`blackboard/testing/acceptance.md` |
| `ProdChangeSession` | `#55` | `pub struct`：3 个 agent + 3 个 review factory + `ctx` + `plan_review_instances`（全 pub） |
| `plan_kickoff` | `#69` | `pub fn(&ChangeCtx)->String` |
| `baseline_kickoff` | `#89` | `pub fn(&ChangeCtx)->String` |
| `coding_review_instruction` | `#103` | `pub fn(&ChangeCtx,&[String])->String` |
| `testing_review_instruction` | `#125` | `pub fn(&ChangeCtx)->String` |
| `coding_kickoff` | `#146` | `pub fn(&ChangeCtx,Option<&TestFailureContext>)->String` |
| `MAX_EMPTY_ISSUES_NUDGES` | `#172` | `pub const usize = 3` |
| `ISSUES_NUDGE` | `#175` | `pub const &str` 空箱补齐话术 |
| `testing_kickoff` | `#179` | `pub fn(&ChangeCtx)->String` |
| `plan_exists` | `#579` | `pub fn(&ChangeCtx)->bool` |
| `baseline_log_path` | `#584` | `pub fn(&Path)->PathBuf` |

### 内部符号

- `evidence_exit_code`（`#204`）、`MAX_MARKER_NUDGES`（`#210`）、`MARKER_NUDGE`（`#213`）、`require_marker_nudged`（`#218`）、`issues_of`（`#238`）、`review_once`（`#264`）。

### 行为要点（代码内可见）

- `run_plan`/`amend_plan`/`fix_plan` 共用 `run_plan_with`：converse → 追标记 → 读计划 → `audit_verdicts` 硬门（不 clean 直接 `PlanTurn::Fail`，不进审查）→ `audit_plan_file`（失败包 `Quality`）→ `review_once` → 记录 `plan_review_instances`。
- `run_coding`：基线轮（`baseline_kickoff` + 追标记）→ `check_baseline_log`（落 gate.baseline）→ `coding_kickoff(last_failure)`；`parse_route == plan` 时不要求标记、直接 `CodingTurn{reviews:0}`；否则追标记 → `check_self_check`（gate.self_check）→ `read_steps` → `check_step_commits`（gate.step_commits）→ `changed_files` → coding review（Fail → `Quality`）。
- `run_testing`：循环 `testing_kickoff`；带路由无 `<issues>` → 追 `ISSUES_NUDGE`，超过 `MAX_EMPTY_ISSUES_NUDGES`（3）→ `Quality`；路由直接返回 `RouteCoding`/`RoutePlan`；否则追标记 → `check_regression_log(board,true)` → 读对账表（缺失 → `Context` 错误）→ 每项 `path_refs` 非空 → test review。
- `review_once`：落 `review.open`（role/round/`set_step`）→ `review_with_gate(..., ReviewBudget::default(), ...)`；协议违例/实例失败/判负三通道由 `src/sop/review_gate.rs` 分流，返回实际实例数。

### 测试事实（`#[cfg(test)]`，34 例）

- 位置：`#658`/`#671`/`#769`/`#787`/`#814`/`#833`/`#876`/`#902`/`#933`/`#954`/`#1001`/`#1021`/`#1047`/`#1067`/`#1109`/`#1138`/`#1175`/`#1239`/`#1270`/`#1293`/`#1336`/`#1361`/`#1384`/`#1414`/`#1447`/`#1470`/`#1550`/`#1576`/`#1600`/`#1630`/`#1652`/`#1682`/`#1706`/`#1734`。
- 测试基建：`Agent`（脚本化 `PlanAgentSession`，记录 seen 消息）、`ReviewFactory`/`OneReview`（脚本化审查）、`ScriptedResultFactory`（可返回 `Err`/空回复并数开过实例）、`NoUser`、`fixture(with_git)`、`coding_ready(f)`。

### crate 内调用点（消费者）

- `src/change/mod.rs:20/41` 声明与转发。
- `src/server/change_engine.rs:246` `build_ctx` 组装 `ChangeCtx`；会话装配处构建 `ProdChangeSession`。
- `src/change/orchestrator.rs` 经 `ChangeSession` trait 调用（生产实现即本文件）。
- `tests/change_failure_context/main.rs:23` 用 `ChangeCtx`/`ProdChangeSession`/`coding_kickoff`。

### 外部依赖

- `crate::change::{boundary, coding_gate, gate0, orchestrator, plan_audit, state, testing_gate}`；`crate::error`；`crate::sop::{coding_protocol, planning, review_gate}`；`std::path`。测试：`tempfile`、`std::rc::Rc`/`RefCell`、`std::process::Command`（git）。
