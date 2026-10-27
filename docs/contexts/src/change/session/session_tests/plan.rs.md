---
type: module
module_path: src/change/session/session_tests/plan.rs
unit: unit-src-17
description: plan 段与提示词契约单测——标记追补、裁决硬门、审查注入、失败明细送达、空箱拒收
---

# src/change/session/session_tests/plan.rs

plan 段（以及不依赖 git 的提示词契约）单测（ARC-R6 自 session.rs 的 mod tests 平移，纯移动）。15 个 `#[test]` 全部走 `use super::*`（`:3`）拿夹具，覆盖三块：**plan 轮的编排**（审计硬门 / 审查注入 / 判负回传 / `plan_facts` 回读）、**完成标记追补**、**提示词契约**（问询纪律是否写进开工语与打回消息、失败明细送达块是否逐字且只追加、空箱路由是否被拒收）。

## 职责

**负责**：
- 为 `src/change/session.rs` 的 plan 段与提示词构造器提供行为级断言（输入脚本化回复 → 断言返回值 / 错误文案 / 工厂开出的实例内容）。
- 给每条关键判定留一个「**错实现形态**」注释：说明删掉哪个机制本用例会变红（反向验证的可读版本，注释本身落在 `:140`、`:166`、`:197`、`:218`、`:228`、`:248`、`:274`、`:294`、`:336`）。
- 两个本模块私有助手：`write_plan`（`:120`，替换计划里的 `[自决]` 歧义条）与 `plan_session`（`:125`，造一个只挂 plan 侧脚本的 `ProdChangeSession`）。

**不负责**：coding / testing 段的门序与里程碑（在 `src/change/session/session_tests/runtime.rs`）；夹具本身（在 `src/change/session/session_tests/mod.rs`）。

## 公开成员

N/A —— 无 `pub` 项：15 个用例都是 `#[cfg(test)]` 下的私有 `#[test] fn`，两个助手 `write_plan` / `plan_session` 也是模块私有（仅本文件使用）。用例清单见 [用例与断言](#用例与断言)。

## 依赖

- 上游夹具：`use super::*`（`:3`）继承 `src/change/session/session_tests/mod.rs` 的 `NoUser` / `Agent` / `ReviewFactory` / `Fixture` / `fixture` / `plan_text` / `PLAN_PASS` / `PLAN_FAIL` / `coding_ready`，以及被测模块 `src/change/session.rs` 的全部名字。
- 被测符号：`require_marker_nudged`、`plan_kickoff`、`coding_kickoff`、`ProdChangeSession` 的 `run_plan` / `fix_plan` / `plan_facts` / `run_testing`。
- 跨模块引用：测试门模块（testing_gate）的 `FAILURE_BLOCK_TITLE`（`:232`、`:259` 断言用）与 `coding_failure_block`（`:286`）；`TestFailureContext`（`:250`、`:276`）。
- 外部 crate：无直接 import（`tempfile` 由夹具用）；不需要 `git` 的用例都走 `fixture(false)`，只有两个空箱用例用 `fixture(true)`（`:296`、`:337`）。

## 用例与断言

| 用例 | 位置 | 断言口径 |
|---|---|---|
| `marker_nudge_redelivers_once_then_accepts` | `:6` | 无标记 → 追补 1 次即收；追补话术必须含 `<stage-complete>N` 样板；最终回复带标记 |
| `marker_nudge_gives_up_after_three_redeliveries` | `:19` | 恒不带标记 → 恰好追补 3 次后报错；错误文案含「连续 4 次」与「已追补 3 次」 |
| `plan_facts_are_read_back_from_plan_file` | `:32` | `plan_facts()` 从计划文件回读出验收「给定 x=1 → 返回 2」与白名单 src/a.rs 两项 |
| `plan_pass_runs_audit_and_review_with_audit_injected` | `:50` | 通过路径：kickoff 含 change-id 与计划文件名；审查指令含「勿质疑、勿重算」（审计结论注入）与 path:line 判据 |
| `plan_fail_carries_issues_back` | `:77` | 审查判负 → 返回 PlanTurn 的 Fail 变体且 issues 含「调用方」（明细原样回传，引擎不改写） |
| `plan_without_marker_is_quality_failure` | `:96` | 四轮都不交标记 → `Quality`，文案含标记名与「已追补 3 次」 |
| `plan_with_must_ask_without_answer_fails_before_review` | `:139` | 计划有 `[必须裁决]` 却无留痕 → 硬门直接打回；审查工厂队列为空且 `seen` 为空（**不进审查轮**） |
| `plan_with_must_ask_and_answer_passes_and_review_sees_verdict_facts` | `:165` | 带 `用户裁决` 留痕 → 通过；审查恰好 1 轮；指令含「裁决门事实」「留痕核对」「分类恰当性」 |
| `fix_plan_message_carries_the_ask_instruction` | `:196` | 打回消息必须带 `ask-user-question` 与留痕字段名，否则 agent 不知道下一步去问用户 |
| `plan_kickoff_carries_the_ask_contract` | `:217` | 开工语必须带 `ask-user-question` 锚点与留痕字段锚点 |
| `coding_kickoff_without_context_has_no_failure_block` | `:227` | 无失败上下文时不得注入送达块（标题行与「失败明细」都不许出现），且首行 / 末句逐字不变 |
| `coding_kickoff_with_context_carries_the_failure_details_verbatim` | `:247` | 有上下文时带标题行、两条明细逐字（含 path:line），且不得出现「请自己定位原因」「本消息不提供文件位置」「<位置略>」这类零信息话术 |
| `coding_kickoff_with_context_only_appends_the_block` | `:273` | 有上下文时整串必须恰好等于「无上下文版本 + 空行 + 送达块」——正文一个字都不许改 |
| `empty_issues_route_is_rejected_then_accepted_verbatim` | `:293` | 第 1 轮空箱被拒收并追一条补齐话术；第 2 轮补齐 → `RouteCoding` 两条明细逐字、无引擎合成文案 |
| `repeated_empty_issues_routes_fail_fast` | `:335` | 连续 4 轮空箱 → `Quality`，文案含「未给出 issues 明细」与「4 次」，且全程不出现占位文案 |

## 测试运行与隔离

- 运行：`cargo test --lib change::session::session_tests::plan`（只跑本文件 15 例）；全模块 34 例用 `cargo test --lib change::session`。本轮实测 `15 passed; 0 failed; 0 ignored; 598 filtered out; EXIT=0`（全模块同一轮 `34 passed; 0 failed; 579 filtered out`；`filtered out` 随全库用例数变动，passed 数不受影响）。命令需带 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`（`edition = "2024"` ⇒ Rust 1.85+），并可用 `CARGO_NET_OFFLINE=true`。
- 环境：无需 pi、无外部服务；两个空箱用例（`:293`、`:335`）用 `fixture(true)`，因此本机需可用 `git`。`tempdir()` 按 `TMPDIR` 建目录（`src/change/session/session_tests/mod.rs:90`）——覆盖 `TMPDIR` 时必须先 `mkdir -p`，否则该目录下的全部用例 panic（`PathError { kind: NotFound }`）。
- 隔离：每个用例一个 `tempfile` 临时目录；不写仓内文件、不占端口。
