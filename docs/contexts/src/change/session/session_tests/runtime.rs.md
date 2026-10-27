---
type: module
module_path: src/change/session/session_tests/runtime.rs
unit: unit-src-19
description: coding/testing 运行时与审查三通道单测——门序、里程碑字段、违例重审与实例失败重试
---

# src/change/session/session_tests/runtime.rs

coding / testing 两个阶段运行期行为的单测（ARC-R6 自 session.rs 的 mod tests 平移，纯移动），共 19 个 `#[test]`。本文件把 `ProdChangeSession` 里**只能靠时序与副作用观察**的约定钉住：门序（基线 → 改代码 → 自证 → 步提交 → 审查）、gate.* 与 review.open 里程碑的字段形状、审查三通道的分流（判负 / 协议违例重审 / 实例失败重试）以及「审查轮次 = 实际开过的实例数」这条口径。

## 职责

**负责**：
- 运行时门序断言：红基线不得进入 coding 轮（`src/change/session/session_tests/runtime.rs:162`）；缺自证证据不得进入审查（`:108`）；证据齐备才开庭（`:139`）。
- 里程碑断言：三道机械门各落一条且带判定结论、步提交对账带步骤数、review.open 带角色名 / 轮次 / 模块 target，并逐条过 `assert_milestone_shape`（`:44`）。
- 分支与返回形状：越界申报（只发路由、不打标记）→ 零轮次返回（`:205`）；coding 审查判负 → `Quality`（`:230`）；测试审查判负（`:253`）、测试通过时回读验收对账表（`:283`）、`<route>coding</route>` 带失败明细（`:316`）、对账项无 path:line 被拒（`:339`）。
- 三通道与轮次：违例换新实例后重审通过、轮次计为 2（`:419`、`:445`、`:575`、`:603`）；违例超限 → 质量失败（`:469`）；空回复 / 通道错误换新实例（`:499`、`:551`）；实例失败超限 → 基础设施故障（`:521`）。
- 本文件私有夹具：`ScriptedResultFactory`（`:371`）与 `ScriptedResultReview`（`:389`）——能按脚本返回「空回复」与「错误」并数出开过几个实例，这是三通道用例唯一需要的特殊能力。

**不负责**：plan 轮与提示词契约（在 `src/change/session/session_tests/plan.rs`）；夹具基座 `fixture` / `coding_ready` / `Agent` / `ReviewFactory`（在 `src/change/session/session_tests/mod.rs`）；三通道判定逻辑本身（在 `src/sop/review_gate.rs`，本文件只从外部观察其分流结果）。

## 公开成员

N/A —— 模块内没有 `pub` 项：19 个用例都是私有 `#[test] fn`，三个助手 `ScriptedResultFactory` / `ScriptedResultReview` / `ok` 也是文件私有（`:371`、`:389`、`:413`）。索引见 [私有助手](#私有助手) 与 [用例与断言](#用例与断言)。

## 依赖

- 上游夹具：`use super::*;`（`:3`）经 `src/change/session/session_tests/mod.rs` 的 glob 拿到 `ProdChangeSession` / `fixture` / `coding_ready` / `Agent` / `ReviewFactory` / `NoUser` / `PLAN_PASS`。因此本文件不重复声明这些依赖。
- 被测方法：`ProdChangeSession` 的 `run_coding`（`src/change/session.rs:375`）、`run_testing`（`src/change/session.rs:495`）、`run_plan`（`src/change/session.rs:332`）、`plan_reviews`（`src/change/session.rs:565`）。
- 断言工具：`capture_jsonl` 与 `assert_milestone_shape`（`src/observability/mod.rs:464`、`src/observability/mod.rs:478`，所在模块 `test_support` 声明在 `src/observability/mod.rs:422`）；`is_quality_failure`（`src/error.rs:74`）。
- 外部 crate：无。`fixture(true)` 与 `coding_ready` 会起 `git` 子进程；观测断言是进程内 tracing 捕获，不需要外部 collector、不需要网络与端口。

## 核心口径

- 🟢 **门序是「叫几次 agent」的事**：基线轮与 coding 轮各一次 `converse`，所以「红基线后 agent 只被叫 1 次」就等价于「没进 coding 轮」（`src/change/session/session_tests/runtime.rs:187` 的 `assert_eq!(seen.borrow().len(), 1)`）；反过来 `coding_runs_gates_and_returns_changed_files` 断言恰好 2 条 seen 消息，且第 1 条必须含目标仓路径（否则 agent 会在黑板目录里跑测试）（`src/change/session/session_tests/runtime.rs:7`）。
- 🟢 **里程碑断言是「按字段字面形态做子串匹配」，不解析 JSON**：`capture_jsonl` 拿到整段 tracing 文本后，用 `text.lines().filter(|l| l.contains(event))` 逐行筛事件，再用 `contains("\"passed\":true")`、`contains("\"steps\":1")`、`contains("\"role\":\"change-coding-review\"")`、`contains("\"round\":1")`、`contains("\"target\":\"devforge::change::session\"")` 断言字段与值（`src/change/session/session_tests/runtime.rs:44` 至 `:104`）；最后每个含 `"event":` 的行都过 `assert_milestone_shape`（级别与 `component` 字段）。因此字段改名、字段顺序改、或把 `tracing::info!` 挑到另一个模块（target 变）都会让用例变红，即使对外语义没变。
- 🟢 **三通道从外部如何区分**：违例 → 换新实例重审且**轮次增加**（`src/change/session/session_tests/runtime.rs:445`、`:575`、`:603`）；超限且 `is_quality_failure()` 为真 = 违例超限（`src/change/session/session_tests/runtime.rs:469`）；超限且该谓词为假 = 实例失败超限（`src/change/session/session_tests/runtime.rs:521`）。两个上限都来自 `src/sop/review_gate.rs:16` 与 `src/sop/review_gate.rs:19`（各 3，即「第 4 次超限」）。
- 🟢 **审查轮次口径**：`reviews` 与 `plan_reviews()` 返回的是**实际开过的实例数**，不是「判负次数」——违例重审如实计入（`src/change/session/session_tests/runtime.rs:436` 的 `plan_reviews() == 2` 是最直接的钉子）。
- 🟢 **测试阶段的证据是手写的，但路由早退分支不写**：`testing_review_is_invoked_and_can_fail`（`:255` 至 `:266`）、`testing_pass_reads_acceptance_table`（`:285` 至 `:296`）、`acceptance_table_without_test_ref_is_rejected`（`:341` 至 `:352`）与 `test_review_violation_retries_with_a_new_instance`（`:606` 至 `:617`）都不跑真测试，而是手写两份证据文件——回归日志（路径常量 `REGRESSION_LOG`，`src/change/testing_gate.rs:18`；内容 `command: cargo test` + `exit_code: 0`）与验收对账表（路径由 `acceptance_path()` 拼出，`src/change/session.rs:49`；一行一条「验收标准 → 测试文件:行」）——再断言引擎对这两份文件的读法。`testing_route_coding_returns_failures`（`:316`）是例外：它只给一条 `<route>coding</route>` + `<issues>`，**不写**回归日志也不写对账表——因为 `run_testing` 命中路由就在读证据之前返回（`src/change/session.rs:520` 至 `:525`），该路径根本不校验证据链。

## 私有助手

| 助手 | 位置 | 能力 |
|---|---|---|
| `ScriptedResultFactory` | `:371` | 按脚本回放 `Result<String>`（能回空串、能回 `Err`），并共享一个「已开实例数」计数 Rc |
| `ScriptedResultFactory` 的 `new` | `:377` | 返回 `(工厂, 计数 Rc)`——计数交给用例断言「违例后确实开了第二个实例」 |
| `ScriptedResultReview` | `:389` | `review` 先把计数 +1 再 `pop_front`；脚本耗尽 panic（`"Review 脚本耗尽"`），脚本与调用次数不匹配立刻暴露 |
| `ok` | `:413` | `fn(&str) -> Result<String>` 的语法糖 |

## 用例与断言

| 用例 | 位置 | 断言口径 |
|---|---|---|
| `coding_runs_gates_and_returns_changed_files` | `src/change/session/session_tests/runtime.rs:7` | 三段门 + 审查走完后 `changed` 含 src/a.rs、`reviews == 1`；agent 恰好 2 轮，第 1 轮是基线取证（含目标仓路径与基线证据文件名），第 2 轮才提 impl-steps |
| `coding_gates_and_review_open_are_logged_as_milestones` | `src/change/session/session_tests/runtime.rs:44` | gate.baseline / gate.self_check / gate.step_commits 各 1 条带 `"passed":true`，步提交带 `"steps":1`；review.open 1 条带 `"role":"change-coding-review"`、`"round":1`、`"target":"devforge::change::session"`；全部事件过 `assert_milestone_shape` |
| `coding_without_self_check_evidence_fails_before_review` | `src/change/session/session_tests/runtime.rs:108` | 删掉自证证据 → 错误文本点名证据文件，审查工厂 `seen` 为空（没开庭） |
| `coding_with_self_check_evidence_enters_review` | `src/change/session/session_tests/runtime.rs:139` | 证据齐备 → `reviews == 1` |
| `red_baseline_blocks_before_coding_turn` | `src/change/session/session_tests/runtime.rs:162` | 基线 `exit_code: 1` → 错误含「基线不是绿的」；agent 只被叫 1 次；gate.baseline 1 条带 `"passed":false` 与 `"exit_code":1`（红时退出码按字段落） |
| `route_plan_without_marker_is_accepted` | `src/change/session/session_tests/runtime.rs:205` | 只发 `<route>plan</route>` + `<impact-gap>`、不带标记 → 正常返回且 `reviews == 0`（证明先看路由再看标记） |
| `coding_review_failure_fails_the_stage` | `src/change/session/session_tests/runtime.rs:230` | 审查 FAIL → 错误含「变更编码审查未通过」与被指问题的文件名 |
| `testing_review_is_invoked_and_can_fail` | `src/change/session/session_tests/runtime.rs:253` | 回归日志绿 + 对账表齐 → 审查 FAIL → 错误含「变更测试审查未通过」 |
| `testing_pass_reads_acceptance_table` | `src/change/session/session_tests/runtime.rs:283` | 通过时 TestingTurn 的 Pass 变体里 `acceptance_done` 恰为对账表里的验收标准原文 |
| `testing_route_coding_returns_failures` | `src/change/session/session_tests/runtime.rs:316` | `<route>coding</route>` + 1 条 issues → TestingTurn 的 RouteCoding 变体带回该明细（此路径不校验回归日志与对账表） |
| `acceptance_table_without_test_ref_is_rejected` | `src/change/session/session_tests/runtime.rs:339` | 对账行不指测试 → 错误含「没有指到测试」 |
| `plan_review_instances_count_the_retry` | `src/change/session/session_tests/runtime.rs:419` | 违例后重审通过 → `plan_reviews() == 2`（轮次不是恒 1） |
| `plan_review_violation_retries_with_a_new_instance` | `src/change/session/session_tests/runtime.rs:445` | 违例后 PASS → PlanTurn 判 Pass 且 `opened == 2` |
| `four_plan_review_violations_are_a_quality_failure` | `src/change/session/session_tests/runtime.rs:469` | 4 次违例 → 错误含「协议违例」且 `is_quality_failure()` 为真（不自动重启） |
| `empty_review_reply_retries_with_a_new_instance` | `src/change/session/session_tests/runtime.rs:499` | 空回复后 PASS → `Pass` 且 `opened == 2`（空回复不算判负） |
| `four_empty_review_replies_are_an_infrastructure_failure` | `src/change/session/session_tests/runtime.rs:521` | 4 次空回复 → 错误含「基础设施故障」且 `is_quality_failure()` 为假（可重启） |
| `review_error_retries_with_a_new_instance` | `src/change/session/session_tests/runtime.rs:551` | 通道错误（`System("模型通道 500")`）后 PASS → `opened == 2` |
| `coding_review_violation_retries_with_a_new_instance` | `src/change/session/session_tests/runtime.rs:575` | FAIL 缺 issues（违例）后 PASS → `reviews == 2` 且 `opened == 2` |
| `test_review_violation_retries_with_a_new_instance` | `src/change/session/session_tests/runtime.rs:603` | 同上，作用于测试审查 → `TestingTurn::Pass{reviews: 2}` 且 `opened == 2` |

## 测试运行与环境

- 环境要求：Rust 工具链（`Cargo.toml` 为 `edition = "2024"` ⇒ Rust 1.85+；本机 rustup 无默认工具链，需显式 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`，否则 `cargo` 报「could not choose a version of cargo to run」）+ 可用 `git`（`fixture(true)` 会 `git init` / 提交基线）；无 pi 子进程、无端口；里程碑断言靠进程内 tracing 捕获，不需要外部 collector。
- 依赖安装：`cargo fetch`（首次需网络，之后可加 `CARGO_NET_OFFLINE=true`）。本文件经夹具用到 `tempfile`（临时仓与黑板）；`Cargo.toml` 的 dev-dependencies 里另有 `tower` / `http-body-util`，供服务侧测试用，本文件不用。
- 配置项：N/A（本文件不读环境变量；`DEVFORGE_TODAY` 等由其他模块消费）。
- 启停与健康检查：N/A（无长期进程）；失败模式本身就是「健康信号」——门缺失会让里程碑用例变红，通道分错会让超限用例变红。
- 种子数据：由 `fixture(true)` + `coding_ready` 生成临时仓与证据；四个 testing 类用例另手写回归日志与对账表（见 [核心口径](#核心口径)），路由早退用例不写。
- 测试运行：`cargo test --lib change::session::session_tests::runtime` 跑本文件 19 例（全模块 34 例用 `cargo test --lib change::session`）。本轮实测（`RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable TMPDIR=/var/tmp/u19-tmp CARGO_NET_OFFLINE=true`，快照 2026-10-24，源码 `src/change/session/session_tests/runtime.rs` 18:39 版）：

  ```
  cargo test --lib change::session::session_tests::runtime
  → 19 passed; 0 failed; 0 ignored; 594 filtered out; EXIT=0
  cargo test --lib change::session
  → 34 passed; 0 failed; 0 ignored; 579 filtered out; EXIT=0
  ```

  `filtered out` 随全库用例数变动，`19` / `34` 两个 passed 数由本模块用例数决定。`tempdir()` 按 `TMPDIR` 建目录（`src/change/session/session_tests/mod.rs:90`）——覆盖 `TMPDIR` 前先 `mkdir -p`，否则依赖夹具的用例会 `PathError { kind: NotFound }`。
- 公开接口清单：N/A（无 `pub` 项）——被测接口清单见 `docs/contexts/src/change/session.rs.md` 的「公开成员」。
