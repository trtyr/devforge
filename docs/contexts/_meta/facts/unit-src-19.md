# 勘察事实 — unit-src-19（src/change/session/session_tests/runtime.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/session/session_tests/runtime.rs.md` 与 `_meta/gaps.md`。
> 行号口径：当前工作区实测（文件 638 行）。

- 文件头注释（`:1`）：coding/testing 运行时与审查协议重试测试（ARC-R6 自 session.rs 的 mod tests 平移，纯移动）。
- `use super::*;`（`:3`）——经 `src/change/session/session_tests/mod.rs` 的 `use super::*` 链拿到 `ProdChangeSession` / `ChangeCtx` / `fixture` / `coding_ready` / `Agent` / `ReviewFactory` / `NoUser` / `PLAN_PASS` 等。
- 段注释（`:368`）：`// ---------------- 审查三通道：违例重审 / 实例失败重试（验收 1-8） ----------------`。

## 私有助手（本文件自建，mod.rs 里没有）

| 符号 | 位置 | 形态 |
|---|---|---|
| `ScriptedResultFactory` | `:371` | 字段 `replies: Rc<RefCell<VecDeque<Result<String>>>>`、`opened: Rc<RefCell<usize>>` |
| `ScriptedResultFactory::new` | `:377` | `fn(Vec<Result<String>>) -> (Self, Rc<RefCell<usize>>)`：内部建计数 Rc 并返回克隆（调用方据此断言「开过几个实例」） |
| `ScriptedResultReview` | `:389` | 字段同上；`impl ReviewSession::review`（`:395` 至 `:401`）先把计数 +1，再 `pop_front`，队列耗尽 panic（`"Review 脚本耗尽"`） |
| `impl ReviewSessionFactory for ScriptedResultFactory` | `:404` | `open` 每次造新 `ScriptedResultReview`，共享同一个队列与计数 Rc |
| `ok` | `:413` | `fn(&str) -> Result<String>`，把字面量包成 `Ok` |

## 19 个 `#[test]`

| 用例 | 位置 | 脚本 / 断言事实 |
|---|---|---|
| `coding_runs_gates_and_returns_changed_files` | `:7` | `fixture(true)` + `coding_ready`；coding 脚本 2 条带标记回复，coding review 回 `PLAN_PASS`；断言 `changed` 含 src/a.rs、`reviews == 1`、agent 恰好被叫 2 次、第 1 条含「不要改任何代码」与基线证据文件名与目标仓路径、第 2 条含 impl-steps |
| `coding_gates_and_review_open_are_logged_as_milestones` | `:44` | 包 `capture_jsonl`；每个 gate 事件恰好 1 条且含 `"passed":true`；步提交条目含 `"steps":1`；`review.open` 恰好 1 条且含 `"role":"change-coding-review"`、`"round":1`、`"target":"devforge::change::session"`；所有 `"event":` 行过 `assert_milestone_shape` |
| `coding_without_self_check_evidence_fails_before_review` | `:108` | 删掉自证证据文件后跑 `run_coding` → Err 文本含 evidence/self-check.log，且审查工厂 `seen` 为空 |
| `coding_with_self_check_evidence_enters_review` | `:139` | 证据齐全 → `reviews == 1` |
| `red_baseline_blocks_before_coding_turn` | `:162` | 基线证据 `exit_code: 1`；只给 1 条回复；Err 含「基线不是绿的」；agent 只被叫 1 次；gate.baseline 恰 1 条且含 `"passed":false` 与 `"exit_code":1` |
| `route_plan_without_marker_is_accepted` | `:205` | coding 第 2 条只发 `<route>plan</route>` + `<impact-gap>`（无 stage-complete）→ `turn.reply` 含路由、`reviews == 0`（不 panic 即证明未要求标记） |
| `coding_review_failure_fails_the_stage` | `:230` | coding review 回 FAIL + issues → Err 文本含「变更编码审查未通过」与「src/a.rs」 |
| `testing_review_is_invoked_and_can_fail` | `:253` | 手写回归日志（exit_code 0）与对账表；test review 回 FAIL → Err 含「变更测试审查未通过」 |
| `testing_pass_reads_acceptance_table` | `:283` | 同上夹具；test review 回 `PLAN_PASS` → `TestingTurn::Pass{acceptance_done}` 恰为 `["给定 x=1 → 返回 2"]` |
| `testing_route_coding_returns_failures` | `:316` | 只回 `<route>coding</route>` + 1 条 issues（无回归日志/对账表）→ `TestingTurn::RouteCoding{failures}` 含「断言失败」 |
| `acceptance_table_without_test_ref_is_rejected` | `:339` | 对账表行写「（没写测试）」→ Err 含「没有指到测试」 |
| `plan_review_instances_count_the_retry` | `:419` | 审查先违例后 PASS → `plan_reviews() == 2` |
| `plan_review_violation_retries_with_a_new_instance` | `:445` | 违例后 PASS → `PlanTurn::Pass`，`opened == 2` |
| `four_plan_review_violations_are_a_quality_failure` | `:469` | 4 条违例回复 → Err 含「协议违例」且 `is_quality_failure()` 为真 |
| `empty_review_reply_retries_with_a_new_instance` | `:499` | 空回复后 PASS → `Pass`，`opened == 2` |
| `four_empty_review_replies_are_an_infrastructure_failure` | `:521` | 4 条空回复 → Err 含「基础设施故障」且 `is_quality_failure()` 为假 |
| `review_error_retries_with_a_new_instance` | `:551` | 第 1 条回 `Err(DevforgeError::System("模型通道 500"))`，第 2 条 PASS → `Pass`，`opened == 2` |
| `coding_review_violation_retries_with_a_new_instance` | `:575` | coding review 先回 FAIL 无 issues（违例）后 PASS → `reviews == 2`、`opened == 2` |
| `test_review_violation_retries_with_a_new_instance` | `:603` | test review 先违例后 PASS → `TestingTurn::Pass{reviews: 2}`、`opened == 2` |

## crate 内被引用符号（位置）

- `crate::observability::test_support::capture_jsonl`（`src/observability/mod.rs:464`）、`assert_milestone_shape`（`src/observability/mod.rs:478`）；模块声明 `src/observability/mod.rs:422`。
- `DevforgeError::is_quality_failure`（`src/error.rs:74`）。
- 三通道实现：`src/sop/review_gate.rs:16` `MAX_PROTOCOL_VIOLATIONS = 3`、`:19` `MAX_REVIEW_INSTANCE_FAILURES = 3`、`:78` `review_with_gate`；违例超限文案「审查连续 N 轮输出协议违例…质量失败（不自动重启）」（`:133` 至 `:138`）；实例失败超限文案「…基础设施故障（触发 run 级自动重启）」（`:157` 至 `:162`）。
- 被测方法：`src/change/session.rs:375` `run_coding`、`:495` `run_testing`、`:332` `run_plan`、`:565` `plan_reviews`。

## 外部依赖

- 无额外 crate；`git` 由 `fixture(true)` 使用；观测断言是进程内 tracing 捕获，不需要外部 collector。
