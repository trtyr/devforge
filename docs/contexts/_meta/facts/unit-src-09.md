# 勘察事实 — unit-src-09（src/change/mod.rs + src/change/orchestrator.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/mod.rs.md`、`src/change/orchestrator.rs.md` 与 `_meta/gaps.md`。

## 文件一：src/change/mod.rs

- 模块根：声明 13 个子模块 + `pub use` 转发导出。无测试。
- 子模块声明（`#10-22`）：`boundary` / `change_id` / `coding_gate` / `docs` / `gate0` / `history` / `orchestrator` / `plan_audit` / `prepare` / `report` / `session` / `state` / `testing_gate`。
- 转发导出（`#24-46`，按**选定清单**转发、非各子模块全量公开符号；未列出者走子模块路径）：含别名 `classify as classify_boundary`（boundary）、`apply as apply_doc_level`（docs）、`run as run_change`（orchestrator）、`audit as audit_plan`（plan_audit）、`prepare as prepare_change`（prepare）。

## 文件二：src/change/orchestrator.rs

- 模块职责（文件头注释）：change 编排器（P009 T013）——阶段推进 + coding→plan 反向回退 + 往返上限 3。与 MVP 编排区别：多反向回退、多基线锚点、断点由引擎写（StateStore，跨会话不丢往返计数）。
- 只编排，不碰 prompt/子进程；真实执行由 `ChangeSession` 注入。

### 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `MAX_TEST_REROUTES` | `#21` | `pub const usize = 3`：测试失败回注 coding 的防御性上限 |
| `MAX_PLAN_FIXES` | `#24` | `pub const usize = 3`：plan 审查打回修复上限 |
| `PlanTurn` | `#56` | `#[derive(...)] pub enum`：`Pass` / `Fail { issues: Vec<String> }` |
| `CodingTurn` | `#63` | `pub struct { pub reply:String, pub changed:Vec<String>, pub reviews:usize }` |
| `TestingTurn` | `#74` | `pub enum`：`Pass{acceptance_done, reviews}` / `RouteCoding{failures}` / `RoutePlan{failures}` |
| `ChangeSession` | `#88` | `pub trait`：`run_plan` / `amend_plan` / `fix_plan` / `run_coding` / `run_testing` + 默认方法 `plan_facts` / `plan_reviews` |
| `ChangeRun` | `#130` | `pub struct<'a>` 编排输入：`store`/`repo`/`change_id`/`baseline_sha`/`criteria`/`whitelist`/`usage`/`run_id`/`wall_secs` |
| `ChangeOutcome` | `#149` | `pub struct`：`state: ChangeState` + `report: ChangeReport` |
| `run` | `#155` | `pub fn<S:ChangeSession>(ChangeRun, &mut S, &mut dyn UserLink) -> Result<ChangeOutcome>` |

### 内部符号

- `phase_change`（`fn(&mut ChangeState, ChangePhase)`）：写阶段 + 落 `phase.change` 里程碑 + `set_phase`（心跳同词汇）。
- `fix_step`（`fn(&ChangeState, &str, usize, bool)`）：把「正在做什么」写进心跳步骤字段（`fix:{kind}:{round}` / 阶段名）。

### 状态机 / 阶段流程（代码内可见）

- plan：打回-修复循环（`run_plan` → Pass 进 coding / Fail 收集 issues → `fix_plan` 重写重审，有界 `MAX_PLAN_FIXES`，超限 → Quality）。
- coding⇄plan：`run_coding` → `boundary::classify(changed, whitelist, reply)`：
  - `InBoundary` → 清失败上下文 → 进 testing；
  - `OutOfBoundary { extra }` → Quality「越界改动未申报（范围膨胀）」；
  - `RoutePlan { gaps }` → `check_roundtrip(next)`、`bump_roundtrip`、落历史 `append_roundtrip`、`merge_gaps_into_whitelist`、`amend_plan`；Pass → 续 coding；Fail → Quality。
- testing：`run_testing` →
  - `Pass { acceptance_done }` → `mark_acceptance_done`；`summarize_acceptance` 未全过 → Quality；全过 → Done + 清失败上下文。
  - `RouteCoding { failures }` → `bump_test_reroute`；超 `MAX_TEST_REROUTES` → Quality（超限不写上下文）；否则 `set_test_failure(round, failures)`、`test-reroute` fix 对、推 coding、返回 Quality（文案「已回注 coding」+ 明细原文）。
  - `RoutePlan { failures }` → 推 plan、返回 Quality（「已回注 plan」）。
- 报告：`diff_stats` + `summarize_acceptance` 组装 `ChangeReport`（含 usage 空→「未记录」）。

### 测试事实（`#[cfg(test)]`，23 例 → 见文档测试覆盖表）

- 位置锚点：#539 / #562 / #616 / #645 / #690 / #712 / #738 / #765 / #792 / #816 / #854 / #886 / #913 / #938 / #957 / #988 / #1018 / #1046 / #1069 / #1087 / #1105 / #1124 / #1156。
- 测试基建：`Scripted`（脚本化 ChangeSession，队列弹计划/修复/amend/coding/testing 结果，记录调用序列 `log` 与每轮 coding 收到的失败上下文 `seen_failures`）；`NoUser`（UserLink 拒绝问用户）。

### crate 内调用点（消费者）

- `src/change/mod.rs:33`-35 `pub use orchestrator::{ChangeOutcome, ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change}`（`run as run_change` 在 `:34`；2026-10-21 重新核对：旧记 `:37` 为笔误，`prepare` 才是 `:37`）。
- `src/change/orchestrator.rs` 依赖：`crate::change::boundary` / `report` / `state`（`ChangeState`/`ChangePhase`/`TestFailureContext`/`load`/`save`）/ `history`（`append_roundtrip`）/ `testing_gate`（`route_back_message`）；`crate::runstate::StateStore`；`crate::sop::planning::UserLink`；`crate::error::{DevforgeError, Result}`；`crate::observability`（`set_phase`/`set_step`）。

### 外部依赖

- 生产：无外部 crate（用 `crate::*` 内部模块 + `std`）。测试：`tempfile`。观测：`tracing`。

### 可重放命令（2026-10-21 实跑）

- `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外独立目录> cargo test --lib change::orchestrator` → `23 passed; 0 failed; 577 filtered out`，EXIT=0；`rustc 1.99.0` / `cargo 1.99.0`；edition 2024（`Cargo.toml:5`）。
