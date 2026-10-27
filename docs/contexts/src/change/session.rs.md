---
type: module
module_path: src/change/session.rs
unit: unit-src-17
description: change SOP 的真实 ChangeSession——pi 子进程会话 + 机械门 + 审查三通道接线
---

# src/change/session.rs

change SOP 的生产 `ChangeSession` 实现（P009 接线）：把子进程会话（干活 + 审查）接到编排器上。本层只做「对话 + 解析 + 调机械门」，不做业务判断——判断在提示词（agent 侧）与机械门（计划审计、编码门、边界、测试门）里；所有子会话都是注入的 trait 对象，因此整层可以用假实现做单测，不需要真跑 pi。

## 职责

**负责**：
- 会话上下文与提示词：`ChangeCtx`（仓 / 黑板 / 计划目录 / 基线 / 需求 / 文档档位）+ 六个指令构造器 + 两个只读助手。
- 三阶段实现：`run_plan` / `amend_plan` / `fix_plan`（共用 `run_plan_with`）、`run_coding`（基线轮 → 门 → coding 轮 → 审查）、`run_testing`（路由与空箱拒收 → 回归证据 → 验收对账表 → 审查）。
- 机械门接线：基线门、自证门、步提交对账、验收对账校验，并按结论落 `gate.*` 里程碑。
- 审查三通道：`review_once` 把协议违例 / 实例失败 / 判负分流，违例与失败换**全新实例**重试，实例数如实计入轮次。
- 提示词契约兜底：完成标记追补（`require_marker_nudged`）、空箱路由拒收（`ISSUES_NUDGE`）。

**不负责**：
- 不判边界与往返（`src/change/boundary.rs`）、不解析证据/步骤文件格式（`src/change/coding_gate.rs`）、不判计划结构与裁决门（`src/change/plan_audit.rs`）、不判验收/回归口径（`src/change/testing_gate.rs`）、不推进阶段也不写断点（`src/change/orchestrator.rs`）、不写报告（`src/change/report.rs`）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `ChangeCtx` | 结构体 | [ChangeCtx](#changectx) |
| `ProdChangeSession` | 结构体 | [ProdChangeSession](#prodchangesession) |
| `plan_kickoff` | 函数 | [plan_kickoff](#plan_kickoff) |
| `baseline_kickoff` | 函数 | [baseline_kickoff](#baseline_kickoff) |
| `coding_review_instruction` | 函数 | [coding_review_instruction](#coding_review_instruction) |
| `testing_review_instruction` | 函数 | [testing_review_instruction](#testing_review_instruction) |
| `coding_kickoff` | 函数 | [coding_kickoff](#coding_kickoff) |
| `testing_kickoff` | 函数 | [testing_kickoff](#testing_kickoff) |
| `MAX_EMPTY_ISSUES_NUDGES` | 常量 | [MAX_EMPTY_ISSUES_NUDGES](#max_empty_issues_nudges) |
| `ISSUES_NUDGE` | 常量 | [ISSUES_NUDGE](#issues_nudge) |
| `plan_exists` | 函数 | [plan_exists](#plan_exists) |
| `baseline_log_path` | 函数 | [baseline_log_path](#baseline_log_path) |

## 依赖

- 上游 / 消费方：
  - `src/change/mod.rs:20` 声明 `pub mod session;`；`src/change/mod.rs:41` 转发 `ChangeCtx` 与 `ProdChangeSession`。
  - `src/server/change_engine.rs:12` 引入本模块；`:228` 起直构 `ProdChangeSession`（六个子会话 + ctx + 初始轮次 0）；`src/server/change_engine.rs:307` 的 `build_ctx` 从 `ChangePrep` 逐字段组装 `ChangeCtx`（`:309` 至 `:316` 的字段逐一对应）。
  - 仓外黑盒 `tests/change_failure_context/main.rs:23` 用 `ChangeCtx` / `ProdChangeSession` / `coding_kickoff`。
  - 编排层经 `ChangeSession` trait 调用（`src/change/orchestrator.rs`），本文件是生产实现。
- 向下依赖（`src/change/session.rs:9` 至 `:26` 的 import 块）：边界（`parse_route` / `replan_instruction` / `ImpactGap`）、编码门（`changed_files` / `check_baseline_log` / `check_self_check` / `check_step_commits` / `parse_baseline_log` / `read_steps` / `BASELINE_LOG`）、`DocLevel`、编排器 trait（`ChangeSession` / `CodingTurn` / `PlanTurn` / `TestingTurn`）、`review_instruction`、`TestFailureContext`、测试门（`check_regression_log` / `parse_acceptance_table` / `REGRESSION_LOG`）、`error::{DevforgeError, Result}` 与 `Context` trait、`parse_review_report` / `parse_stage_complete` / `StageVerdict`、`PlanAgentSession` / `ReviewSessionFactory` / `UserLink`、`outcome_of_review_report` / `review_with_gate` / `ReviewBudget`。
- 外部 crate：生产无（只用 `std` 与 crate 内模块）；测试用 `tempfile`（`Cargo.toml` 的 dev-dependency；同表另有 `tower` / `http-body-util`，供服务侧测试用，本模块不用）。

## 核心口径

- **门序固定**：coding 轮先验基线证据（`check_baseline_log`，`:389`）才允许改代码；改完先验自证证据（`check_self_check`，`:428`）、再步提交对账（`check_step_commits`，`:449`）、再算 `changed_files`（`:474`）、最后才进 coding 审查（`:477`）。
- **路由优先于标记**：coding 回复里出现 `<route>plan</route>` 时不要求 `<stage-complete>`（D008：越界申报只发路由 + `<impact-gap>`），直接返回 `CodingTurn{reviews: 0}` 交编排层补边界（`:415` 至 `:422`）。
- **空箱拒收**：测试阶段带 `<route>coding|plan</route>` 却没有 `<issues>` 明细 → 不收、追 `ISSUES_NUDGE` 重发；连续超过 `MAX_EMPTY_ISSUES_NUDGES` → `Quality`（`:509` 至 `:515`）。引擎**不合成占位失败明细**。
- **完成标记追补**：模型忘交 `<stage-complete>` 时同会话追补（`MARKER_NUDGE`）至多 3 次，第 4 次仍缺 → `Quality`（文案带「连续 N 次」「已追补 3 次」与回复尾巴）。
- **审查三通道**（`review_once`，委托 `src/sop/review_gate.rs`）：协议违例 → 换新实例重审，超限 `Quality`（不自动重启）；实例失败（空回复 / 通道错误 / 早退）→ 换新实例重试，超限 `System`（基础设施故障，可重启）；判负 → 交报告。返回的实例数就是报告里的审查轮次。

---

### ChangeCtx

- 职责：一次 change run 的固定上下文（路径与输入的集合）；所有提示词构造器与三阶段实现都只读它。字段全 `pub`，装配层直构。（🟢 CONFIRMED `src/change/session.rs:30`）
- 构造或初始化：生产由 `build_ctx` 组装（`src/server/change_engine.rs:307`）；单测里在 `src/change/session/session_tests/mod.rs:89` 的 `fixture` 中直构。
- 公开成员：
  - `repo: PathBuf` — 目标仓（真代码所在，`:32`）。
  - `blackboard: PathBuf` — SOP 黑板（`plans/<p>/<t>`）：impl-steps.md、证据、审查报告在这（`:34`）。
  - `change_dir: PathBuf` — 计划目录（目标仓 `docs/plantree/changes/<id>`，`:36`）。
  - `change_id: String`（`:37`）、`baseline: String`（基线 sha，`:38`）、`requirement: String`（需求原文，`:40`）、`doc_level: DocLevel`（`:41`）。
  - `plan_path(&self) -> PathBuf` — `change_dir` 下拼出计划文件名 change-plan.md；纯拼接无 IO（`:45`）。见 [ChangeCtx::plan_path](#changectxplan_path)。
  - `acceptance_path(&self) -> PathBuf` — `blackboard` 下拼出对账表相对路径 testing/acceptance.md；纯拼接无 IO（`:49`）。见 [ChangeCtx::acceptance_path](#changectxacceptance_path)。
- 生命周期：`#[derive(Debug, Clone)]`；随 `ProdChangeSession` 持有，跨三阶段不变；`build_ctx` 之后不再改写。
- 典型用法：`coding_kickoff` 与 `plan_kickoff` 的入参；审查前用 `plan_path()` 读计划全文。

### ChangeCtx::plan_path

- 摘要：计划文件路径。
- 行为语义：`change_dir.join("change-plan.md")`，无 IO、幂等（🟢 CONFIRMED `src/change/session.rs:45`）。
- 参数：`&self`。
- 返回：`PathBuf`（不保证存在；存在性判断见 `plan_exists`）。
- 异常：N/A（纯拼接，不可能失败）。
- 示例：`run_plan_with` 用它读计划全文（`src/change/session.rs:298` 取路径、`:299` 起 `read_to_string`）。

### ChangeCtx::acceptance_path

- 摘要：验收对账表路径。
- 行为语义：把 blackboard 与相对路径 testing/acceptance.md 拼接，无 IO（🟢 CONFIRMED `src/change/session.rs:49`）。
- 参数：`&self`。
- 返回：`PathBuf`。
- 异常：N/A（纯拼接）。
- 示例：`run_testing` 读它对账（`src/change/session.rs:532` 至 `:537`：`read_to_string` 起于 `:532`、`parse_acceptance_table` 在 `:536`、`acceptance_done` 在 `:537`；紧随其前的 `check_regression_log` 在 `:530`）；`testing_review_instruction` 把它写进审查指令（`src/change/session.rs:137`，即 format 实参里的 `ctx.acceptance_path().display()`）。

### ProdChangeSession

- 职责：生产态 `ChangeSession`——把六个注入的子会话（plan agent / plan review / coding agent / coding review / test agent / test review）接到三阶段流程上。本类型不含业务判断，只做调用顺序与门禁编排。
- 构造或初始化：字段全 `pub`，无 `new`；生产在 `src/server/change_engine.rs:228` 处直构，测试在 `src/change/session/session_tests/plan.rs:125` 的 `plan_session` 与各用例里直构。（🟢 CONFIRMED `src/change/session.rs:55`）
- 公开成员：
  - `plan_agent` / `coding_agent` / `test_agent: Box<dyn PlanAgentSession>`（`:56` / `:58` / `:60`）— 干活的会话。
  - `plan_review` / `coding_review` / `test_review: Box<dyn ReviewSessionFactory>`（`:57` / `:59` / `:61`）— 审查工厂，每次开庭 `open` 一个新实例。
  - `ctx: ChangeCtx`（`:62`）。
  - `plan_review_instances: usize`（`:64`）— plan 审查实际开过的实例数；`plan_reviews()` 只返回它（`:565`）。
- 生命周期：一次 run 内可变借用；`plan_review_instances` 在每次 plan 审查后重写（`:321`），供报告取「审查轮次」。
- 典型用法：实现 `ChangeSession` 的七个方法——`run_plan`（`:332`）、`amend_plan`（`:337`）、`fix_plan`（`:352`）、`run_coding`（`:375`）、`run_testing`（`:495`）、`plan_reviews`（`:565`）、`plan_facts`（`:569`）。共同步骤见 [核心口径](#核心口径) 与 [内部辅助](#内部辅助非-pub)。

### plan_kickoff

- 摘要：plan 阶段开工语（首次立项）。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:69`）：拼入 change-id、目标仓、需求原文、文档档位、计划文件路径；要求产出六块（需求复述与歧义 / 影响面每条 path:line / 三张表 / 白名单 / 验收标准 / 风险）；写死工具纪律（结构问题走 codegraph、不写实施顺序、不写回归命令、不改代码）与问询纪律（标了 `[必须裁决]` 的先 ask-user-question，答复逐字写进条目）；要求末尾输出 `<stage-complete>1</stage-complete>`。纯字符串组装，无副作用。
- 参数：`ctx: &ChangeCtx`。
- 返回：`String`（完整提示词）。
- 异常：N/A（纯格式化）。
- 示例：产物同时含 `ask-user-question` 与 `用户裁决` 锚点（断言在 `src/change/session/session_tests/plan.rs:217`）。

### baseline_kickoff

- 摘要：基线轮开工语——只取证、不改代码。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:89`）：明确「cd 到目标仓跑测试，不是在黑板目录里跑」；要求把结果写进黑板下的基线证据文件（`BASELINE_LOG`），首行 `command: <命令>`、一行 `exit_code: N`；要求基线红就如实写红（引擎会拦）。无副作用。
- 参数：`ctx: &ChangeCtx`。
- 返回：`String`。
- 异常：N/A（纯格式化）。
- 示例：`run_coding` 第一步就用它（`src/change/session.rs:382`）。

### coding_review_instruction

- 摘要：coding 审查指令（diff ↔ 计划对账）。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:103`）：给出 change-id、计划路径、黑板、基线 sha、本轮改动文件列表；六条判据（每块 diff 有无计划依据 / 是否全在白名单 / 测试是否单测且断言不空 / 有无计划外依赖 / 文档与 doc-lint 是否跟 / 自证证据是否真实覆盖本轮 diff）；要求先写 coding/reports/review.md，末尾输出 `<stage-verdict>` + `<next>` + `<issues>`。无副作用。
- 参数：`ctx: &ChangeCtx`、`changed: &[String]`（相对基线的改动文件列表，`changed.join("、")` 拼入正文）。
- 返回：`String`。
- 异常：N/A（纯格式化）。
- 示例：`run_coding` 用它开庭（`src/change/session.rs:477` 至 `:481`）。

### testing_review_instruction

- 摘要：测试审查指令（验回归证据与验收对账，不重跑套件）。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:125`）：给出黑板、回归日志路径、对账表路径；四条判据（回归真跑且退出码与结论一致 / 对账表每条指到具体测试 path:line / 至少一处反向验证 / 测试阶段不得改产品代码）；要求先写 testing/reports/review.md 并输出协议块。无副作用。
- 参数：`ctx: &ChangeCtx`。
- 返回：`String`。
- 异常：N/A（纯格式化）。
- 示例：`run_testing` 用它开庭（`src/change/session.rs:548` 至 `:552`，即 `review_once` 那次调用）。

### coding_kickoff

- 摘要：coding 开工语；`failure` 非空时**只追加**送达块，正文逐字不变。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:146`）：
  - 正文：change-id、基线、目标仓、计划路径、黑板（当前目录）；顺序要求（先写基线证据，红就停 → 写 impl-steps.md → 按步执行、每步一提交、提交信息含 change-id 与 `step N`）；只改白名单，越界必须发 `<route>plan</route>` + `<impact-gap>` 且放回复末尾；本轮必须留自证证据（命令原文 + 真实退出码 + 摘要，覆盖写）；末尾 `<stage-complete>1</stage-complete>`。
  - `Some(f)` → `format!("{base}\n\n{}", coding_failure_block(f))`（`:162` 至 `:168`）；`None` → 原样返回 `base`（不注入空块）。
- 参数：`ctx: &ChangeCtx`、`failure: Option<&TestFailureContext>`（上一轮测试失败上下文）。
- 返回：`String`。
- 异常：N/A（纯格式化）。
- 示例：无上下文时不含送达块标题、首末句不变（`src/change/session/session_tests/plan.rs:227`）；有上下文逐字带明细且不出现「请自己定位原因」类零信息话术（`:247`）；有上下文时恰好等于「正文 + 空行 + 送达块」（`:273`）。

### testing_kickoff

- 摘要：测试阶段开工语。
- 行为语义（🟢 CONFIRMED `src/change/session.rs:179`）：给出 change-id、目标仓、计划（验收标准来源）、黑板；顺序要求（跑既有全套回归并写回归日志 → 为变更点写测试 → 对账表写到 testing/acceptance.md，每行 `- <验收标准原文> → <测试文件>:<行>` → 至少一次反向验证）；失败发 `<route>coding</route>`（计划问题发 `<route>plan</route>`）并用 `<issues>` 逐条列明细，路由块与 `<issues>` 块都放回复末尾；通过发 `<stage-complete>1</stage-complete>`。无副作用。
- 参数：`ctx: &ChangeCtx`。
- 返回：`String`。
- 异常：N/A（纯格式化）。
- 示例：`run_testing` 的首条消息就是它（`src/change/session.rs:497`）。

### MAX_EMPTY_ISSUES_NUDGES

- 名称 / 类型：`pub const MAX_EMPTY_ISSUES_NUDGES: usize`（🟢 CONFIRMED `src/change/session.rs:172`）。
- 取值 / 默认：`3`。
- 是否必填：编译期常量，恒存在（无配置开关）。
- 影响：`run_testing` 里「带路由但 `<issues>` 为空」的追补上限；`nudges > 3` → `Quality`（文案含「未给出 issues 明细」与连续次数），不自动重启（`:510` 至 `:514`）。
- 示例：连发 4 次空箱路由 → 失败且文案含「4 次」（`src/change/session/session_tests/plan.rs:335`）。

### ISSUES_NUDGE

- 名称 / 类型：`pub const ISSUES_NUDGE: &str`（🟢 CONFIRMED `src/change/session.rs:175`）。
- 取值 / 默认：固定话术——说明「输出了回注路由但没有给出 `<issues>` 失败明细」，该回注已被引擎拒收，要求补齐后重发，路由块与 `<issues>` 块都放回复末尾。
- 是否必填：编译期常量。
- 影响：空箱被拒收后作为下一条 user 消息重发（`:515`）；不是引擎合成的占位失败明细。
- 示例：被拒收后第 2 条消息含「请补齐失败明细后重发」（`src/change/session/session_tests/plan.rs:293`）。

### plan_exists

- 摘要：只读检查计划文件是否已存在（编排层判断能否续跑用）。
- 行为语义：`ctx.plan_path().is_file()`；无写入、无副作用、幂等（🟢 CONFIRMED `src/change/session.rs:579`）。
- 参数：`ctx: &ChangeCtx`。
- 返回：`bool`（`true` = 计划文件是普通文件）。
- 异常：N/A（`is_file` 不返回错误）。
- 示例：续跑前判断计划是否已落盘，避免重发 plan 开工语。

### baseline_log_path

- 摘要：黑板上的基线证据路径（测试注入用）。
- 行为语义：`blackboard.join(BASELINE_LOG)`，纯路径拼接（🟢 CONFIRMED `src/change/session.rs:584`）。
- 参数：`blackboard: &Path`。
- 返回：`PathBuf`（不保证存在）。
- 异常：N/A（纯拼接）。
- 示例：单测用它写基线证据文件，再让 `check_baseline_log` 读同一位置。

## 内部辅助（非 pub）

- `evidence_exit_code`（`:204` 至 `:207`）：读证据文件并用门自己的 `parse_baseline_log` 解析，返回 `Option<i32>`；读不到或解析不了返回 `None`（tracing 的 `Option` 字段为 `None` 时整个字段不落）。红基线路径靠它把退出码按字段带出（`:404`）。
- `MAX_MARKER_NUDGES`（`:210`）= 3，与 `MARKER_NUDGE`（`:213`，含 `<stage-complete>N` 样板）：标记追补上限与话术。
- `require_marker_nudged`（`:218` 至 `:236`）：循环直到 `parse_stage_complete` 有值；每次追补 `nudges += 1`，超过 3 次返回 `Quality`（文案含「连续 {nudges} 次」「已追补 3 次」与最近回复尾巴，`:227` 至 `:231`）；接受任意阶段号的标记。四个调用点：plan 轮（`:294`）、基线轮（`:384`，stage 名带「基线轮」）、coding 轮（`:423`）、测试轮（`:526`）。
- `issues_of`（`:238` 至 `:252`）：抽 `<issues>…</issues>` 之间内容（无闭合则到串尾），逐行 `trim`、去 `-`/`*` 前缀、丢空行；无 `<issues>` → 空向量。
- `review_once`（`:264` 至 `:288`）：先取本次开庭轮次（`next_review_round`，`:270`）、把心跳步骤设为 review 加 role（`:272`）、落 review.open 里程碑（`:273` 至 `:279`），再委托 `review_with_gate`（ReviewBudget 的 default）跑三通道（`:280` 至 `:287`）；返回（判负报告，实际实例数）。
- `run_plan_with`（`:292` 至 `:328`）：plan/amend/fix 共用——converse → 标记追补 → 读计划全文（读不到 → `Quality`，`:298` 至 `:300`）→ 裁决硬门 `audit_verdicts` 不干净则**直接**返回 PlanTurn 的 Fail 变体、不进审查（`:304` 至 `:309`）→ `audit_plan_file`（`:310` 至 `:315`）→ `review_once`（`:316` 至 `:320`）→ 写轮次字段（`:321`）→ verdict 映射（`:322` 至 `:327`）。

## 测试覆盖（本模块 34 例）

用例本体在 `src/change/session/session_tests/`（`src/change/session.rs:589` 只有 `mod session_tests;` 一行声明）。

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `marker_nudge_redelivers_once_then_accepts` | 追补一次即收、话术带标记样板 | `src/change/session/session_tests/plan.rs:6` |
| `marker_nudge_gives_up_after_three_redeliveries` | 追补恰好 3 次后 Quality | `src/change/session/session_tests/plan.rs:19` |
| `plan_facts_are_read_back_from_plan_file` | 从计划回读验收与白名单 | `src/change/session/session_tests/plan.rs:32` |
| `plan_pass_runs_audit_and_review_with_audit_injected` | kickoff 带 id 与路径、审查注入审计 | `src/change/session/session_tests/plan.rs:50` |
| `plan_fail_carries_issues_back` | 审查 Fail → issues 回传 | `src/change/session/session_tests/plan.rs:77` |
| `plan_without_marker_is_quality_failure` | 连续无标记 → Quality（已追补 3 次） | `src/change/session/session_tests/plan.rs:96` |
| `plan_with_must_ask_without_answer_fails_before_review` | 裁决硬门在审查前打回 | `src/change/session/session_tests/plan.rs:139` |
| `plan_with_must_ask_and_answer_passes_and_review_sees_verdict_facts` | 留痕齐全过审、指令含裁决事实 | `src/change/session/session_tests/plan.rs:165` |
| `fix_plan_message_carries_the_ask_instruction` | 打回消息带 ask 与留痕指令 | `src/change/session/session_tests/plan.rs:196` |
| `plan_kickoff_carries_the_ask_contract` | 开工语带问询契约 | `src/change/session/session_tests/plan.rs:217` |
| `coding_kickoff_without_context_has_no_failure_block` | 无上下文不注空块、首末句不变 | `src/change/session/session_tests/plan.rs:227` |
| `coding_kickoff_with_context_carries_the_failure_details_verbatim` | 有上下文逐字带明细 | `src/change/session/session_tests/plan.rs:247` |
| `coding_kickoff_with_context_only_appends_the_block` | 有上下文只追加送达块 | `src/change/session/session_tests/plan.rs:273` |
| `empty_issues_route_is_rejected_then_accepted_verbatim` | 空箱拒收 → 补齐后逐字收 | `src/change/session/session_tests/plan.rs:293` |
| `repeated_empty_issues_routes_fail_fast` | 连续空箱 → Quality（无占位文案） | `src/change/session/session_tests/plan.rs:335` |
| `coding_runs_gates_and_returns_changed_files` | 基线轮 → 改代码 → 三门 → 审查 | `src/change/session/session_tests/runtime.rs:7` |
| `coding_gates_and_review_open_are_logged_as_milestones` | `gate.*` 与 review.open 里程碑 | `src/change/session/session_tests/runtime.rs:44` |
| `coding_without_self_check_evidence_fails_before_review` | 缺自证证据不进审查 | `src/change/session/session_tests/runtime.rs:108` |
| `coding_with_self_check_evidence_enters_review` | 有自证证据进审查（1 轮） | `src/change/session/session_tests/runtime.rs:139` |
| `red_baseline_blocks_before_coding_turn` | 红基线拦住 + 里程碑带退出码 | `src/change/session/session_tests/runtime.rs:162` |
| `route_plan_without_marker_is_accepted` | 路由优先、不要求标记、零轮次 | `src/change/session/session_tests/runtime.rs:205` |
| `coding_review_failure_fails_the_stage` | coding 审查 Fail → Quality | `src/change/session/session_tests/runtime.rs:230` |
| `testing_review_is_invoked_and_can_fail` | 测试审查被调用且可判负 | `src/change/session/session_tests/runtime.rs:253` |
| `testing_pass_reads_acceptance_table` | 通过时读对账表 | `src/change/session/session_tests/runtime.rs:283` |
| `testing_route_coding_returns_failures` | 路由 coding 带失败明细 | `src/change/session/session_tests/runtime.rs:316` |
| `acceptance_table_without_test_ref_is_rejected` | 对账项无 path:line 被拒 | `src/change/session/session_tests/runtime.rs:339` |
| `plan_review_instances_count_the_retry` | 轮次计入违例重审 | `src/change/session/session_tests/runtime.rs:419` |
| `plan_review_violation_retries_with_a_new_instance` | 违例换新实例重审 | `src/change/session/session_tests/runtime.rs:445` |
| `four_plan_review_violations_are_a_quality_failure` | 违例超限 Quality（不重启） | `src/change/session/session_tests/runtime.rs:469` |
| `empty_review_reply_retries_with_a_new_instance` | 空回复换新实例 | `src/change/session/session_tests/runtime.rs:499` |
| `four_empty_review_replies_are_an_infrastructure_failure` | 实例失败超限 System（可重启） | `src/change/session/session_tests/runtime.rs:521` |
| `review_error_retries_with_a_new_instance` | 通道错误换新实例 | `src/change/session/session_tests/runtime.rs:551` |
| `coding_review_violation_retries_with_a_new_instance` | coding 违例重审、轮次 2 | `src/change/session/session_tests/runtime.rs:575` |
| `test_review_violation_retries_with_a_new_instance` | test 违例重审、轮次 2 | `src/change/session/session_tests/runtime.rs:603` |

## 测试运行与环境

- 环境要求：Rust 工具链（`Cargo.toml` 为 `edition = "2024"` ⇒ Rust 1.85+；本机 rustup 无默认工具链，需显式 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`，否则 `cargo` 报「could not choose a version of cargo to run」）+ 可用 `git`（`fixture(true)` 会起 `git init/add/commit/rev-parse` 子进程）；无外部服务、无端口占用——子会话全是脚本化假实现（`Agent` / `ReviewFactory`），不 spawn pi。
- 依赖安装：`cargo fetch`（首次需网络；之后可加 `CARGO_NET_OFFLINE=true`）。本模块测试只用 `tempfile`（`Cargo.toml` 的 dev-dependencies 里另有 `tower` / `http-body-util`，供服务侧测试用）。
- 配置项：本模块不读环境变量；测试用的临时目录由 `tempfile` 按 `TMPDIR` 创建（`src/change/session/session_tests/mod.rs:90`）——若把 `TMPDIR` 指到不存在的目录，34 例里依赖夹具的 32 例会 panic（`PathError { kind: NotFound }`），所以覆盖 `TMPDIR` 前先 `mkdir -p`。
- 启停与健康检查：本模块无长期进程可启停；「健康」由门禁结论体现（gate.baseline / gate.self_check / gate.step_commits / review.open 四条 tracing 事件）。
- 种子数据：每个用例自建临时仓 + 计划样板（`src/change/session/session_tests/mod.rs:71` 的 `plan_text`、`:89` 的 `fixture`、`:157` 的 `coding_ready`）。
- 测试运行：`cargo test --lib change::session` 跑本模块全部 34 例；只跑 plan 段的 15 例用 `cargo test --lib change::session::session_tests::plan`。本轮实测（`RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=/var/tmp/u17-target TMPDIR=/var/tmp/u17-tmp CARGO_NET_OFFLINE=true`，快照 2026-10-24，源码 `src/change/session.rs` 18:40 版）：

  ```
  cargo test --lib change::session
  → 34 passed; 0 failed; 0 ignored; 579 filtered out; EXIT=0
  cargo test --lib change::session::session_tests::plan
  → 15 passed; 0 failed; 0 ignored; 598 filtered out; EXIT=0
  ```

  `filtered out` = 全库用例总数减本过滤器结果，随其他单元增删测试而变；`34` / `15` 两个 passed 数由本模块用例数决定，不受影响。
- 公开接口清单：本模块导出见 [公开成员](#公开成员)——`ChangeCtx` 及其两个路径方法、`ProdChangeSession`、六个指令构造器、两个常量、`plan_exists`、`baseline_log_path`；trait 方法经 `ChangeSession` 暴露。
