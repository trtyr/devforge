---
type: module
module_path: src/change/mod.rs
unit: unit-src-09
description: 变更 SOP（P009）引擎侧实现的模块根——子模块声明与转发导出
---

# src/change/mod.rs

变更 SOP（P009）的引擎侧实现模块根。

## 职责

本文件是 crate::change 模块根：声明全部子模块，并按**显式选定清单**把部分公开符号 `pub use` 带到 crate::change 根（**不是每个子模块的全部公开符号**；未列出的公开符号仍经 `crate::change::<子模块>::…` 访问）。

**负责**：
- `pub mod` 声明 13 个子模块：`boundary` / `change_id` / `coding_gate` / `docs` / `gate0` / `history` / `orchestrator` / `plan_audit` / `prepare` / `report` / `session` / `state` / `testing_gate`。
- 按选定清单转发公开符号（**非全量**，逐模块清单见下文「转发导出清单」）；部分用别名（`classify as classify_boundary`、`apply as apply_doc_level`、`run as run_change`、`audit as audit_plan`、`prepare as prepare_change`）。

**不负责**：不含任何业务逻辑（纯声明与转发）；不判断流程（见 `src/change/orchestrator.rs`）；不实现各门/各阶段（见各子模块文档）。

## 公开成员

本表列的是**子模块**本身（`pub mod`）；各子模块被带到 crate::change 根部的符号见下文「转发导出清单」。

| 成员 | 类型 | 文档 |
|---|---|---|
| `boundary` | 子模块 | `src/change/boundary.rs`（unit-src-01） |
| `change_id` | 子模块 | `src/change/change_id.rs`（unit-src-02） |
| `coding_gate` | 子模块 | `src/change/coding_gate.rs`（unit-src-04） |
| `docs` | 子模块 | `src/change/docs.rs`（unit-src-07） |
| `gate0` | 子模块 | `src/change/gate0.rs`（unit-src-08） |
| `history` | 子模块 | `src/change/history.rs`（unit-src-08） |
| `orchestrator` | 子模块 | `src/change/orchestrator.rs`（unit-src-09） |
| `plan_audit` | 子模块 | `src/change/plan_audit.rs`（unit-src-12） |
| `prepare` | 子模块 | `src/change/prepare.rs`（unit-src-15） |
| `report` | 子模块 | `src/change/report.rs`（unit-src-16） |
| `session` | 子模块 | `src/change/session.rs`（unit-src-16） |
| `state` | 子模块 | `src/change/state.rs`（unit-src-21） |
| `testing_gate` | 子模块 | `src/change/testing_gate.rs`（unit-src-22） |

## 依赖

- 向下依赖：`src/change/` 下的全部 13 个子模块（见公开成员表）。本文件自身不依赖外部 crate（错误面等依赖在各子模块，不在本文件）。
- 消费方（🟢 CONFIRMED）：`src/server/change_engine.rs:9` 直接引入 `orchestrator` 的 `ChangeRun` 与 `run`（重命名为 `run_orchestrator`）、`:10` 引入 `prepare`、`:11` 引入 `write_report`、`:12` 引入 `ChangeCtx` / `ProdChangeSession`、`:13` 引入 `CHANGE_SOP`；生产调用 `prepare` 在 `:103`（`:317` 为单测内调用）。集成测试 `tests/change_failure_context/main.rs:18` 至 `:27` 走子模块路径（`boundary` / `gate0` / `orchestrator` / `session` / `state` / `testing_gate`）。
- 注：仓内消费者全部走**子模块路径**；本文件 `pub use` 的 5 个别名（`run_change` / `prepare_change` / `apply_doc_level` / `classify_boundary` / `audit_plan`）在 `src/` 与 `tests/` 中以 `crate::change::<别名>` 形式的调用点为 **0**（🟢 CONFIRMED：`grep -rn "change::run_change\|change::prepare_change\|change::apply_doc_level\|change::classify_boundary\|change::audit_plan" src tests` 无命中），属对外 API 面。⚠️ 同名词不同实体：`src/server/change_engine.rs:66` 的 `pub fn run_change` 是服务器入口方法、`src/server/mod.rs:240` 的分支名，**不是**本文件的别名。

## 转发导出清单

下表是 `src/change/mod.rs:24`-46 的**全部转发行**；列出的符号可直接以 `crate::change::<符号>` 使用（带别名者按别名）。**未列出 ≠ 私有**——它们是子模块的 `pub` 成员，只是不经根导出，须写 `crate::change::<子模块>::<符号>`。

| 子模块 | 转发行 | 转发到根的符号 |
|---|---|---|
| `boundary` | `src/change/mod.rs:24`-27 | `BoundaryVerdict`、`ImpactGap`、`MAX_ROUNDTRIPS`、`check_roundtrip`、`classify as classify_boundary`、`merge_gaps_into_whitelist`、`replan_instruction` |
| `change_id` | `src/change/mod.rs:28` | `assign_change_id`、`change_root`、`make_change_id`、`slugify`、`validate_change_id` |
| `coding_gate` | `src/change/mod.rs:29` | `SELF_CHECK_LOG`、`check_baseline_log`、`check_self_check`、`check_step_commits`、`read_steps` |
| `docs` | `src/change/mod.rs:30` | `DocAction`、`apply as apply_doc_level`、`essential_ready` |
| `gate0` | `src/change/mod.rs:31` | `Baseline`、`DocLevel`、`ensure_baseline` |
| `history` | `src/change/mod.rs:32` | `append_roundtrip`、`history_path` |
| `orchestrator` | `src/change/mod.rs:33`-35 | `ChangeOutcome`、`ChangeRun`、`ChangeSession`、`CodingTurn`、`PlanTurn`、`TestingTurn`、`run as run_change` |
| `plan_audit` | `src/change/mod.rs:36` | `PlanAudit`、`audit as audit_plan`、`review_instruction` |
| `prepare` | `src/change/mod.rs:37` | `ChangePrep`、`prepare as prepare_change` |
| `report` | `src/change/mod.rs:38`-40 | `AcceptanceSummary`、`ChangeReport`、`DiffStats`、`diff_stats`、`summarize_acceptance`、`write_report` |
| `session` | `src/change/mod.rs:41` | `ChangeCtx`、`ProdChangeSession` |
| `state` | `src/change/mod.rs:42` | `CHANGE_SOP`、`ChangePhase`、`ChangeState`、`TestFailureContext` |
| `testing_gate` | `src/change/mod.rs:43`-46 | `AcceptanceMatch`、`FAILURE_BLOCK_TITLE`、`check_regression_claim`、`check_regression_log`、`coding_failure_block`、`failure_details`、`reconcile_acceptance`、`route_back_message` |

**未转发、须走子模块路径的典型公开符号**（🟢 CONFIRMED：对照 `src/change/mod.rs:24` 起的 13 个转发行与各子模块的 `pub` 声明逐个核对）：

- `boundary`：`parse_route`（`src/change/boundary.rs:45`）。
- `change_id`：`date_segment`。
- `coding_gate`：`BASELINE_LOG`、`IMPL_STEPS`、`COMMIT_PREFIX`、`BaselineEvidence`、`Step`、`parse_baseline_log`、`parse_steps`、`changed_files`、`commits_since`、`step_numbers_in`、`impl_steps_path`。
- `docs`：`inventory_path`（`src/change/docs.rs:56`）。
- `orchestrator`：`MAX_TEST_REROUTES`、`MAX_PLAN_FIXES`。
- `plan_audit`：`PLAN_BLOCKS`、`VERDICT_MUST_ASK`、`VERDICT_SELF`、`VERDICT_ANSWER_FIELD`、`VerdictAudit`、`audit_verdicts`、`path_refs`、`plan_facts`、`audit_plan_file`。
- `state`：`load`、`save`。
- `session`：`coding_kickoff` 等会话辅助（不在 `src/change/mod.rs:41` 的转发行里）。

## 转发导出别名

有 5 个转发改变了名字（`crate::change::<别名>` 用法，原符号仍可经子模块路径取）：

| 别名 | 原符号（所在子模块） | 所在转发行 |
|---|---|---|
| `classify_boundary` | `classify`（`src/change/boundary.rs:133`） | `src/change/mod.rs:25`（块 `:24`-27） |
| `apply_doc_level` | `apply`（`docs` 子模块） | `src/change/mod.rs:30` |
| `run_change` | `run`（`src/change/orchestrator.rs:155`） | `src/change/mod.rs:34`（块 `:33`-35） |
| `audit_plan` | `audit`（`plan_audit` 子模块） | `src/change/mod.rs:36` |
| `prepare_change` | `prepare`（`src/change/prepare.rs`） | `src/change/mod.rs:37` |

（🟢 CONFIRMED：子模块声明在 `src/change/mod.rs:10`-22；转发块 `:24`-46；无别名的其余转发行见上表「转发导出清单」。）

## 测试覆盖

本文件无 `#[cfg(test)]` 测试（纯声明模块）。
