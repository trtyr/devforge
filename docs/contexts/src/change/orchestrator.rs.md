---
type: module
module_path: src/change/orchestrator.rs
unit: unit-src-09
description: change 编排器——阶段推进、coding→plan 反向回退、往返上限 3
---

# src/change/orchestrator.rs

change 编排器（P009 T013）：阶段推进 + coding→plan 反向回退 + 往返上限 3。

## 职责

本模块把整条 change 流程编排起来：plan → coding → testing → report，并处理 coding→plan 反向回退、测试回注 coding、断点续跑与报表生成。它**只管编排**：不懂 prompt、不碰子进程；每一阶段的真实执行由 `ChangeSession` 注入（生产是 pi 子进程，单测是脚本化的假 session）。

**负责**：
- 阶段推进与断点：`run` 按 `ChangePhase`（plan/coding/testing/done）恢复/推进，用 `state` 模块的 `load` / `save` 跨会话续跑（往返计数、回注计数、失败上下文都落在断点里）。
- coding⇄plan 反向回退：boundary 的 `classify` 判定边界，`RoutePlan` → `append_roundtrip` 落历史 + `merge_gaps_into_whitelist` 补白名单 + `amend_plan` 补边界重进 coding；往返上限 3（`MAX_ROUNDTRIPS`），超限 Quality。
- 测试回注：`TestingTurn` 的 `RouteCoding` → 写失败上下文、推回 coding；有界 `MAX_TEST_REROUTES`。`RoutePlan` → 推回 plan。
- 修复轮与里程碑：`phase_change` / `fix_step` 落 phase.change 与 fix.start/fix.end 可观测事件（心跳同词汇）。
- 报表：组装 `ChangeReport`（diff / review 轮次 / 往返 / 用量 / 验收对账）。

**不负责**：不产 prompt、不启动子进程（`ChangeSession` 实现方负责）；不判定边界本身（见 `src/change/boundary.rs`）；不解析证据日志/实施步骤（见 `src/change/coding_gate.rs`）；不决定往返历史的落盘格式与路径（见 `src/change/history.rs`；本模块只调用 `append_roundtrip`）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `MAX_TEST_REROUTES` | 常量 | [MAX_TEST_REROUTES](#max_test_reroutes) |
| `MAX_PLAN_FIXES` | 常量 | [MAX_PLAN_FIXES](#max_plan_fixes) |
| `PlanTurn` | 枚举 | [PlanTurn](#planturn) |
| `CodingTurn` | 结构体 | [CodingTurn](#codingturn) |
| `TestingTurn` | 枚举 | [TestingTurn](#testingturn) |
| `ChangeSession` | trait | [ChangeSession](#changesession) |
| `ChangeRun` | 结构体 | [ChangeRun](#changerun) |
| `ChangeOutcome` | 结构体 | [ChangeOutcome](#changeoutcome) |
| `run` | 函数（入口） | [run](#run) |

## 依赖

- 上游 / 消费方：`src/change/mod.rs:33`-35 `pub use orchestrator::…`（`run` 在该块内被重命名为 `run_change`，见 `src/change/mod.rs:34`；仓内无该别名的调用点，属对外 API 面）。实际调用点：`src/server/change_engine.rs:9` 引入 `ChangeRun` 与 `run`（重命名为 `run_orchestrator`）、`:209` 在 watchdog 内调用、`:48`（`finalize` 入参）与 `:284`（单测构造）使用 `ChangeOutcome`；集成测试 `tests/change_failure_context/main.rs:20` 至 `:22` 引子模块公开面（测试内本地重命名 `run`）。
- 向下依赖（`src/change/orchestrator.rs` 的 import 行，逐行核对）：

  | import 行 | 引入 |
  |---|---|
  | `src/change/orchestrator.rs:11` | `Path`（std） |
  | `:13` | `boundary`（self，含 `BoundaryVerdict` / `ImpactGap`） |
  | `:14` | `report`（self，含 `AcceptanceSummary` / `ChangeReport` / `summarize_acceptance`） |
  | `:15` | `state`（`ChangePhase` / `ChangeState` / `TestFailureContext` / `load` / `save`） |
  | `:16` | `DevforgeError` / `Result` |
  | `:17` | `StateStore`（runstate） |
  | `:18` | `UserLink`（sop/planning） |

  未走 import、在函数体内全限定引用的依赖：`append_roundtrip`（`src/change/orchestrator.rs:246`）、`route_back_message`（`:333`、`:360`）、`set_phase` / `set_step`（`:33`、`:51`）、`info!` / `warn!`（`:34`、`:178`、`:334` 等）。模块职责见 `src/change/boundary.rs`、`src/change/report.rs`、`src/change/state.rs`、`src/change/history.rs`、`src/change/testing_gate.rs` 各自文档。
- 外部 crate：生产无（全 crate 内部 + `std`）。测试用 `tempfile`（`Cargo.toml:27`）。

## 核心口径

- **阶段词汇**：阶段名用 `ChangePhase` 的变体名（plan/coding/testing/done）——与心跳的阶段字段同一套词汇。
- **断点**：`state` 模块的 `save` 落 `<plans>/<proj>/<task>/change/state.json`（`StateStore` 路径规则 `src/runstate/mod.rs:48`，生产构造见 `src/server/change_engine.rs:190`-193，SOP 段为 `change`；单测断言 `src/change/orchestrator_tests.rs:712`），跨会话保留往返计数 roundtrips、回注计数 test_reroutes、失败上下文 last_test_failure。
- **往返历史**：`append_roundtrip` 把每次补边界写进**目标仓**的 `docs/plantree/changes/<change_id>/history.md`（路径规则 `src/change/history.rs:10`-16，调用点 `src/change/orchestrator.rs:246`）——与断点（落 plans 根）不是同一棵树。
- **边界回路**：coding 改动经 `classify`（`src/change/boundary.rs:133`）归入 InBoundary（进 testing）/ OutOfBoundary（范围膨胀 Quality）/ RoutePlan（补边界重进 coding，有界 `MAX_ROUNDTRIPS`=3，`src/change/boundary.rs:13`）。⚠️ 超限时 `check_roundtrip` 的文案是「停下来问人」，但编排层实际不再问人：`run` 全程不调用 `ask`（🟢 CONFIRMED：`src/change` 内两处 `ask` 定义都在测试桩里——`src/change/orchestrator_tests.rs:20` 与 `src/change/session/session_tests/mod.rs:13`；`run` 全程不调用 `ask`），超限一律以 `Quality` 错误结束、交上层处理。
- **失败上下文**：测试回注把 `TestFailureContext`（round + issues 明细原文）写进断点，下一轮 coding 的 `run_coding(last_failure)` 收到；coding 过门/测试终态消费即清；超限分支不写不清。
- **错误**：plan 连修超限 / 越界未申报 / 补边界仍未过 / 验收缺项 / 回注超限 → `Quality`（`DevforgeError` 变体，质量门，不自动重启）。

---

### PlanTurn

- 职责：plan 阶段的结果。`Pass` = 过审，`Fail` = 审查打回（带问题清单）。
- 构造 / 初始化：由 `ChangeSession` 的 `run_plan` / `fix_plan` / `amend_plan` 返回。
- 公开成员（变体）：
  - `Pass` — 计划过审。
  - `Fail { issues: Vec<String> }` — 计划审查打回，`issues` 为回注给 plan agent 的问题清单。
- 生命周期：值对象，编排层按变体分派。
- 典型用法：`run` 在 plan 段匹配 `Pass` 进 coding；`Fail` 收集 issues 走 `fix_plan` 修复循环（`src/change/orchestrator.rs:155`）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/orchestrator.rs:56`）

### CodingTurn

- 职责：coding 阶段的结果（agent 完整回复 + 相对基线改动文件 + 本阶段审查轮次）。
- 构造 / 初始化：由 `ChangeSession` 的 `run_coding` 返回。
- 公开成员：
  - `reply: String` — agent 的完整回复（可能含 `<route>plan</route>` 与 `<impact-gap>` 声明）。
  - `changed: Vec<String>` — 相对基线的改动文件（`git diff --name-only <baseline>`）。
  - `reviews: usize` — 本阶段消耗的审查轮次（计入报告）。
- 生命周期：值对象，编排层用 `changed` + `reply` 走 boundary 的 `classify`。
- 典型用法：`run` 在 coding 段以 `classify(&turn.changed, &whitelist, &turn.reply)` 判定边界（`src/change/orchestrator.rs:155`）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/orchestrator.rs:63`）

### TestingTurn

- 职责：测试阶段的结果。`Pass` = 验收对账通过；`RouteCoding` / `RoutePlan` = 测试失败按目标回注。
- 构造 / 初始化：由 `ChangeSession` 的 `run_testing` 返回。
- 公开成员（变体）：
  - `Pass { acceptance_done: Vec<String>, reviews: usize }` — 已对账通过的验收项与审查轮次。
  - `RouteCoding { failures: Vec<String> }` — 回归/测试失败 → 回注 coding，`failures` 带明细原文（含 path:line）。
  - `RoutePlan { failures: Vec<String> }` — 发现计划层面问题 → 回 plan。
- 生命周期：值对象；编排层按变体推进或回注。
- 典型用法：`run` 在 testing 段匹配各变体（见 [run](#run)）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/orchestrator.rs:74`）

### ChangeSession

- 职责：各阶段执行器接口（生产 = pi 子进程，单测 = 脚本化假实现）。编排层只调用接口，不碰实现。
- 构造 / 初始化：由实现方提供并注入 `run`；`ChangeSession` 本身不可实例化（trait）。
- 公开成员（方法）：
  - `run_plan(&mut self, ui: &mut dyn UserLink, roundtrip: usize) -> Result<PlanTurn>` — 跑一次 plan（含审查），`roundtrip` 让实现知道第几次往返。
  - `amend_plan(&mut self, ui, gaps: &[ImpactGap], roundtrip) -> Result<PlanTurn>` — 带缺口打回 plan：只补边界并重审。
  - `fix_plan(&mut self, ui, issues: &[String], round: usize) -> Result<PlanTurn>` — 审查打回 plan：问题清单回注、重写计划后重审。
  - `run_coding(&mut self, ui, roundtrip, last_failure: Option<&TestFailureContext>) -> Result<CodingTurn>` — 跑 coding（含按步提交）；`last_failure` = 上一轮测试失败上下文（`None` = 无上下文，开工语逐字不变）。
  - `run_testing(&mut self, ui) -> Result<TestingTurn>` — 跑 testing（回归优先 + 验收对账）。
  - `plan_facts(&mut self) -> Result<(Vec<String>, Vec<String>)>`（默认空对）— 计划过审后回读（验收标准, 白名单），编排层据此进 coding/testing。
  - `plan_reviews(&self) -> usize`（默认 0）— plan 阶段审查轮次（计入报告）。
- 生命周期：可变借用贯穿 `run`；无自持会话之外的资源契约（由实现方决定）。
- 典型用法：生产实现包装 pi 子进程；测试用脚本化 session 按队列吐结果并记录调用序列（见测试覆盖）。
- 派生 trait：无（trait 本身）。（🟢 CONFIRMED `src/change/orchestrator.rs:88`）

### ChangeRun

- 职责：编排输入（一次 `run` 的参数集合）。
- 构造 / 初始化：调用方构值传入 `run`；字段 `pub`。
- 公开成员：
  - `store: &'a StateStore` — 断点存储。
  - `repo: &'a Path` — 目标仓（diff、报告落这里）。
  - `change_id: &'a str` — 变更 id。
  - `baseline_sha: &'a str` — 基线锚点。
  - `criteria: Vec<String>` — 计划验收标准（对账用）。
  - `whitelist: Vec<String>` — 计划白名单（初始；补边界会被并入）。
  - `usage: String` — 用量行（会话账汇总；空串则报告写「未记录」）。
  - `run_id: String` — 本次运行 id。
  - `wall_secs: u64` — 墙钟（调用方计时；单测给固定值）。
- 生命周期：借用 `store`/`repo`/`&str` 字段贯穿 `run`；`Vec`/`String` 字段由调用方构造后移入。
- 典型用法：`run(cfg(...), &mut session, &mut ui)`（`src/change/orchestrator.rs:155`）。
- 派生 trait：N/A（无 derive）。（🟢 CONFIRMED `src/change/orchestrator.rs:130`）

### ChangeOutcome

- 职责：编排结果。
- 构造 / 初始化：由 `run` 在流程走到报告段时组装返回。
- 公开成员：
  - `state: ChangeState` — 终态（含 `phase`、往返计数、验收已确认等）。
  - `report: ChangeReport` — 本次运行报表。
- 生命周期：值对象；`report` 可由 `change_engine` 的 `finalize`（`src/server/change_engine.rs:47`）继续落盘（见测试 `mock_chain_produces_report_and_done_state`）。
- 典型用法：`run` 返回后由调用方读 `state` 的 `phase` / 写报告。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/orchestrator.rs:149`）

### MAX_TEST_REROUTES

- 摘要：测试阶段失败后回注 coding 的防御性上限（决策只给了 plan↔coding 的 3 次；这里防止死循环）。（🟢 CONFIRMED `src/change/orchestrator.rs:21`）
- 取值：`3`（`pub const MAX_TEST_REROUTES: usize = 3`）。
- 行为语义：`run` 在 `RouteCoding` 分支先 `bump_test_reroute` 再比对；超过上限（>3）则返回 `Quality`（`测试→coding 已回注 N 次（上限 3），停止：…`）且**不写失败上下文**。
- 参数：N/A（常量）。
- 返回：N/A。
- 异常：N/A。
- 示例：`testing` 阶段已累计 3 次回注后再次 `RouteCoding` → 文案含「上限 3」（`src/change/orchestrator_tests.rs:625`）。

### MAX_PLAN_FIXES

- 摘要：plan 阶段审查打回后的修复上限（对齐 MVP 规划的重审节奏；超限 = 计划质量问题，不自动重启）。（🟢 CONFIRMED `src/change/orchestrator.rs:24`）
- 取值：`3`（`pub const MAX_PLAN_FIXES: usize = 3`）。
- 行为语义：`run` 的 plan 循环把 `fix_round` 累加，超过上限则返回 `Quality`（`计划审查连续 N 轮未通过（上限 3 轮修复）：…`）。
- 参数：N/A（常量）。
- 返回：N/A。
- 异常：N/A。
- 示例：初始 FAIL + 连续 3 轮 fix 仍 FAIL → 文案「连续 4 轮未通过」且 `is_quality_failure()`（`src/change/orchestrator_tests.rs:731`）。

### run

- 摘要：跑完整条 change 流程（从断点继续），返回终态与报表。入口函数。
- 行为语义（🟢 CONFIRMED `src/change/orchestrator.rs:155`）：
  - **载入断点**：`state` 模块的 `load(store, change_id, baseline_sha)`；基线为空则回填；从局部变量 `whitelist`/`criteria` 启动（补边界可并入白名单、计划可覆写验收标准）。
  - **plan 段**（`phase == Plan`）：打回-修复循环——`fix_round==0` 跑 `run_plan`，否则 `fix_plan(issues, fix_round)`；每次 `state.review_rounds += plan_reviews()`；`Pass` → 回读 `plan_facts`、`phase_change(Coding)`、保存、break；`Fail` → `fix_round+=1`，超 `MAX_PLAN_FIXES` → Quality。`fix_plan` 前后落 fix.start/fix.end（含轮次）。
  - **coding⇄plan 段**（`phase == Coding`）：循环 `run_coding(…, last_test_failure)`；`classify` 三路：InBoundary → `clear_test_failure` → `phase_change(Testing)` → break；OutOfBoundary{extra} → Quality「越界改动未申报（范围膨胀）」；RoutePlan{gaps} → `check_roundtrip(next)`、`bump_roundtrip`、`append_roundtrip(repo, change_id, roundtrips, gaps)`、`merge_gaps_into_whitelist`、`amend_plan(gaps)`（落 fix.start/fix.end）；Pass → `review_rounds += plan_reviews()` + 回读 facts + 保存 + 继续 coding；Fail → Quality「补边界后计划审查仍未过」。
  - **testing 段**（`phase == Testing`）：`run_testing`——Pass{acceptance_done} → `mark_acceptance_done`；`summarize_acceptance(criteria, done)` 未全过 → Quality「验收对账缺 N 条」；全过 → `phase_change(Done)`、`clear_test_failure`、保存。RouteCoding{failures} → `bump_test_reroute`；超限 → Quality（不写上下文）；否则 `set_test_failure(round, failures)`、`phase_change(Coding)`、保存、返回 Quality「已回注 coding，{route_back_message}」（落 fix.start/fix.end）。RoutePlan{failures} → `phase_change(Plan)`、保存、返回 Quality「测试发现计划问题，已回注 plan」。
  - **报告**：`diff_stats(repo, baseline_sha).unwrap_or_default()` + `summarize_acceptance` 组装 `ChangeReport`；`diff_stats` 的 `System` 错误被吞（取默认统计，不阻断收尾）；`usage` 空串→「未记录」；返回 `ChangeOutcome{state, report}`。
  - **保存与回推矩阵**（🟢 CONFIRMED：逐分支比对 `src/change/orchestrator.rs:169`-392；「是否 save」= 该分支返回/跳转前有没有调 `save`）：

    | 分支 | 是否 `save` | 断点 `phase` |
    |---|---|---|
    | plan `Fail` 超限（`:210`-215） | 否 | 仍是 `Plan`（未迁移） |
    | plan `Pass`（`:204`-206） | 是 | `Coding` |
    | plan `Fail` 未超限（`:208`-217） | 否（继续下一轮） | 仍是 `Plan` |
    | coding `InBoundary`（`:230`-233） | 是 | `Testing` |
    | coding `OutOfBoundary`（`:235`-240） | 否 | 仍是 `Coding` |
    | coding `RoutePlan` → `amend` 过审（`:269`-281） | 是 | 仍是 `Coding`（`continue` 重进 coding） |
    | coding `RoutePlan` → `amend` 被拒（`:282`-288） | 否 | 仍是 `Coding` |
    | testing `Pass` 但验收缺项（`:310`-316） | 否 | 仍是 `Testing` |
    | testing `Pass` 且验收全过（`:317`-321） | 是 | `Done` |
    | testing `RouteCoding` 超限（`:323`-331） | 是（先 `save` 再返错） | 仍是 `Testing` |
    | testing `RouteCoding` 未超限（`:345`-357） | 是 | `Coding` |
    | testing `RoutePlan`（`:359`-366） | 是 | `Plan` |

  - 幂等/断点：失败一律以 `Quality` 结束本次调用，但**不是每个分支都落盘**（见上表）。未落盘的分支阶段未迁移，下次 `run` 从同一阶段重入；这些分支里改过的 `review_rounds` 等内存字段不会写回断点（🟡 INFERRED：`state` 是 `load` 出来的局部变量，只有 `save` 的调用点才写回；`RoutePlan` 被拒分支例外地**已写 history 文件但断点未落**：`append_roundtrip` 在 `:246` 先于 `amend`）。往返/回注计数只在落盘分支变化，故跨会话累计。
- 参数：`run: ChangeRun<'_>` — 编排输入；`session: &mut S`（`S: ChangeSession`）— 阶段执行器；`ui: &mut dyn UserLink` — 用户连接（真实执行用）。
- 返回：`Result<ChangeOutcome>`。
- 异常：`Quality`（`DevforgeError` 变体）— 触发条件：plan 连修超限 / 越界未申报 / 补边界仍未过 / 验收缺项 / 测试回注超限（每次失败都是质量门）。也透传 `ChangeSession` 实现返回的 `Result` 错误（如红基线的 `Quality`，session 在 kickoff 前拦截）。
- 示例（全部见测试覆盖表，`src/change/orchestrator_tests.rs:146`+）：happy 链 plan→coding→testing→report（`:146`）；coding 往返补边界重进（`:252`）；越界未申报 Quality 且停在 coding（`:297`）；第 4 次往返拒（`:319`）；从断点续跑跳 plan 保往返（`:345`）；红基线停 coding 不回 plan（`:694`）；验收缺一条失败（`:653`）。

### phase_change（内部）

- 职责：阶段转换——写状态 + 落一条 phase.change 里程碑 + 把阶段写进观测上下文（心跳随之刷新）。（🟢 CONFIRMED `src/change/orchestrator.rs:30`）
- 行为语义：记录 `from`/`to`（`ChangePhase` 变体名）；`set_phase(to.as_str())`；经 tracing 的 `info` 落 event=phase.change、component=change-orchestrator。
- 参数：`state: &mut ChangeState`、`to: ChangePhase`。
- 返回：N/A（`()`）。
- 异常：N/A。

### fix_step（内部）

- 职责：把「正在做什么」写进心跳的步骤字段（心跳要回答「死在哪个环节」不只「死在哪个阶段」）。（🟢 CONFIRMED `src/change/orchestrator.rs:45`）
- 行为语义：`begin==true` → 步骤 `fix:{kind}:{round}`；`begin==false` → 恢复为当前阶段名；再 `set_step`。
- 参数：`state: &ChangeState`、`kind: &str`、`round: usize`、`begin: bool`。
- 返回：N/A（`()`）。
- 异常：N/A。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `happy_path_plan_coding_testing_report` | plan→coding→testing→report、审查轮计入 | `src/change/orchestrator_tests.rs:146` |
| `phase_changes_and_fix_rounds_are_logged_as_milestones` | 阶段转换落 phase.change、修复轮落 fix 对 | `src/change/orchestrator_tests.rs:169` |
| `the_test_to_coding_reinjection_is_logged_as_a_fix_pair` | 测试回注落 fix 对 + 推 coding | `src/change/orchestrator_tests.rs:223` |
| `coding_routes_back_to_plan_then_reenters_coding` | coding→plan→补边界→重进 coding、往返记断点 | `src/change/orchestrator_tests.rs:252` |
| `out_of_boundary_without_declaration_fails_and_stays_coding` | 越界未申报 Quality 且停在 coding | `src/change/orchestrator_tests.rs:297` |
| `fourth_roundtrip_is_refused` | 第 4 次往返拒、停在 3 次 | `src/change/orchestrator_tests.rs:319` |
| `resume_from_checkpoint_skips_plan_and_keeps_roundtrips` | 断点续跑跳 plan 保往返计数 | `src/change/orchestrator_tests.rs:345` |
| `the_failure_context_reaches_the_coding_session` | 失败上下文送达 coding 会话 | `src/change/orchestrator_tests.rs:372` |
| `testing_failure_routes_back_and_is_bounded` | 测试失败回注带明细原文 | `src/change/orchestrator_tests.rs:399` |
| `reroute_writes_the_failure_context_into_the_checkpoint` | 回注写失败上下文进断点 | `src/change/orchestrator_tests.rs:423` |
| `failure_context_survives_across_sessions` | 失败上下文跨会话保留 | `src/change/orchestrator_tests.rs:461` |
| `coding_success_clears_the_failure_context` | coding 过门消费即清 | `src/change/orchestrator_tests.rs:493` |
| `second_reroute_overwrites_the_failure_context` | 二次回注覆盖旧 issues 不追加 | `src/change/orchestrator_tests.rs:520` |
| `testing_pass_clears_the_failure_context` | 终态清上下文 | `src/change/orchestrator_tests.rs:545` |
| `reroute_over_the_limit_keeps_the_failure_context_untouched` | 超限分支不写不清 | `src/change/orchestrator_tests.rs:564` |
| `mock_chain_produces_report_and_done_state` | 接口级全链 report+done、断点落盘 | `src/change/orchestrator_tests.rs:595` |
| `test_reroutes_are_bounded_across_sessions` | 回注次数跳会话累计不重启清零 | `src/change/orchestrator_tests.rs:625` |
| `acceptance_missing_one_item_fails_run` | 验收缺一项失败 | `src/change/orchestrator_tests.rs:653` |
| `plan_reviews_are_counted_in_report` | plan 审查轮计入报告 | `src/change/orchestrator_tests.rs:676` |
| `red_baseline_stops_before_coding_agent_work` | 红基线停 coding 不回 plan 不重试 | `src/change/orchestrator_tests.rs:694` |
| `checkpoint_path_is_change_state_on_blackboard` | 断点隔离落黑板 state.json | `src/change/orchestrator_tests.rs:712` |
| `plan_rejection_is_quality_failure` | plan 连拒 Quality、修复恰好 3 轮 | `src/change/orchestrator_tests.rs:731` |
| `plan_fix_loop_recovers_after_one_rejection` | 打回一次重写重审过 → 正常进 coding | `src/change/orchestrator_tests.rs:763` |

## 测试运行与环境

- 单测位置（ARC-R6 已拆分）：`src/change/orchestrator.rs:394` 只有 `#[cfg(test)] mod orchestrator_tests;` 一行声明，用例本体在 `src/change/orchestrator_tests.rs`（23 例），上表逐例给出位置。
- **环境前提**：edition 2024（`Cargo.toml:5`）⇒ 需 Rust **1.85+** 且 toolchain 已安装（rustup 无默认 toolchain 时必须显式指定，如 `RUSTUP_TOOLCHAIN=stable`）。本次实测：`rustc 1.99.0` / `cargo 1.99.0`。
- **依赖获取**：`cargo fetch`（首次需网络；本模块自身只用到 `tempfile` 作为 dev-dependency，见 `Cargo.toml:27`）。
- **运行单模块单测**（🟢 CONFIRMED，本轮实跑：`cargo test --lib change::orchestrator` → `23 passed; 0 failed; 577 filtered out`，EXIT=0）：

  ```bash
  CARGO_TARGET_DIR=<仓外独立空目录> cargo test --lib change::orchestrator
  ```

  实测环境 `rustc 1.99.0` / `cargo 1.99.0`（2026-10 于本仓）；编译产物放仓外目录以免污染仓内 `target/`。
- 隔离：`StateStore` 以 `tempfile` 的临时目录作黑板根（测试辅助 `store`，`src/change/orchestrator_tests.rs:7`）；往返历史文件也落在同一临时目录（`repo` 参数即该 tempdir，路径规则 `src/change/history.rs:10`）——用例不碰真实 plans 根、不需要网络/端口/外部服务，可并行跑。
- 用例不要求 `git` 仓库：收尾 `diff_stats` 对非 git 临时目录返回 `System` 错误，被 `unwrap_or_default()` 吞掉（统计为空，`src/change/orchestrator.rs:371`；`diff_stats` 只产出 `System` 错误，`src/change/report.rs:95`-106）。
- 测试基建：`Scripted`（`src/change/orchestrator_tests.rs:26` 起，脚本化 `ChangeSession` + 调用序列 `log` + 每轮 `seen_failures`）、`NoUser`（`UserLink` 拒绝问用户，`:19`）、观测断言用 `capture_jsonl`（`:181`、`:234`）。集成面另见 `tests/change_failure_context/main.rs`（仓外黑盒，只经 change 子模块公开面观测；用例分散在 `tests/change_failure_context/context.rs` / `tests/change_failure_context/guards.rs` / `tests/change_failure_context/support.rs`）。
- **本单元的 Runbook 边界**：本节承担环境 / 依赖获取 / 测试运行与隔离三件；**N/A** 项及理由——启停与健康检查：本单元是库内模块，无独立进程（进程级启停属汇编层的服务 Runbook）；配置项：本模块无可配键，输入全经 `ChangeRun` 传入；种子数据：无持久化种子数据，断点由用例自行预置（如 `src/change/orchestrator_tests.rs:345` 起）；公开接口清单：上文「公开成员」表即全部（无 HTTP/CLI 端点）。
