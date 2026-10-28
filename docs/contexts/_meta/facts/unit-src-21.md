# 勘察事实 — unit-src-21（src/change/state.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/state.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/state.rs`（360 行）。
- 模块声明：`src/change/mod.rs:21` `pub mod state;`；`src/change/mod.rs:42` `pub use state::{CHANGE_SOP, ChangePhase, ChangeState, TestFailureContext};`（`load` / `save` 不在转发面，走模块路径）。
- 文件头注释：change run 的检查点（P009 T011b，落点按 plan 010 D001/D022a：引擎写、住黑板）；状态本身走 `runstate::StateStore`（`plans/<project>/<task>/change/state.json`），本模块只定义 change 自己的扩展段——引擎不解释它；续跑语义（010 D002/D003）：`start` 默认从断点继续，`roundtrips` 跨会话保留。

## 公开条目

| 符号 | 位置 | 类型 |
|---|---|---|
| `CHANGE_SOP` | `#15` | `pub const &str = "change"`（注册名 + 状态目录名） |
| `ChangePhase` | `#20` | `#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]` + `#[serde(rename_all="lowercase")]` enum：`Plan` / `Coding` / `Testing` / `Done` |
| `ChangePhase::as_str` | `#28` | `pub fn(&self)->&'static str`：`plan`/`coding`/`testing`/`done` |
| `TestFailureContext` | `#44` | `pub struct { pub round:usize, pub issues:Vec<String> }`（Serialize/Deserialize） |
| `ChangeState` | `#53` | `pub struct`：`phase`、`baseline_sha`、`roundtrips`、`impl_steps_done`、`acceptance_done`、`review_rounds`、`test_reroutes`、`change_id`、`last_test_failure: Option<TestFailureContext>`（除 phase/baseline_sha 外均 `#[serde(default)]`） |
| `ChangeState::new` | `#84` | `pub fn(impl Into<String>, impl Into<String>)->Self`：phase=Plan、计数 0、空向量、last_test_failure=None |
| `ChangeState::from_run` | `#99` | `pub fn(&RunState,&str,&str)->Self`：取 `data["change"]` 反序列化，失败/缺失 → `new` |
| `ChangeState::write_into` | `#108` | `pub fn(&self,&mut RunState)`：`data` 非对象先重置为 `{}`，再写入 `"change"` 键 |
| `ChangeState::mark_step_done` | `#120` | `pub fn(&mut self,u32)`：不存在才 push 并 `sort_unstable` |
| `ChangeState::mark_acceptance_done` | `#127` | `pub fn(&mut self,&str)`：精确去重后 append |
| `ChangeState::bump_review` | `#133` | `review_rounds += 1` |
| `ChangeState::bump_roundtrip` | `#137` | `roundtrips += 1` |
| `ChangeState::bump_test_reroute` | `#141` | `test_reroutes += 1` |
| `ChangeState::set_test_failure` | `#149` | `pub fn(&mut self,usize,Vec<String>)`：**覆盖写** `Some(TestFailureContext{round,issues})` |
| `ChangeState::clear_test_failure` | `#154` | 置 `None` |
| `load` | `#160` | `pub fn(&StateStore,&str,&str)->ChangeState`：`Loaded::Found` → `from_run`，其余（Missing/Unusable）→ 冷启动 `new` |
| `save` | `#168` | `pub fn(&StateStore,&ChangeState)->Result<()>`：取已有或 `fresh_state`，`write_into` 后 `store.save`（原子写，由 runstate 实现） |

## 行为/测试事实（`#[cfg(test)]`，10 例）

- `roundtrip_persists_phase_baseline_and_counters`（`#191`）：save→load 全等；`impl_steps_done` 排序稳定为 `[1,2]`。
- `checkpoint_lives_on_blackboard_path`（`#209`）：路径以 `proj/task/change/state.json` 结尾且文件存在。
- `missing_checkpoint_means_cold_start`（`#224`）：无断点 → phase=Plan、roundtrips=0、baseline 取入参。
- `resume_keeps_roundtrips_across_sessions`（`#234`）：新 `StateStore` 同路径 → 往返计数 2、phase=Testing 保留。
- `counters_are_idempotent`（`#255`）：重复 mark 不重复入列。
- `failure_context_persists_across_sessions`（`#266`）：上下文跨会话保留、round 与 issues 逐字不变。
- `old_checkpoint_without_failure_key_still_loads`（`#296`）：旧断点无 `last_test_failure` 键 → 加载为 None，其余字段（phase/roundtrips/test_reroutes/impl_steps_done/acceptance_done）不丢。
- `failure_context_is_overwritten_not_appended`（`#328`）：第二次 `set_test_failure` 覆盖 round 与 issues。
- `clearing_failure_context_empties_it`（`#339`）：clear 后为 None。
- `foreign_state_is_ignored`（`#347`）：别的 SOP（`StateKey` 名 `doc-from-zero`）的断点不污染 change（roundtrips=0、change_id 取入参）。

## crate 内调用点（消费者）

- `src/change/orchestrator.rs:15` 引入 `ChangePhase` / `ChangeState` / `TestFailureContext` / `load` / `save`；`:30`/`:45` 用 `ChangeState`；`:150` `ChangeOutcome.state`；`:403`/`:871` 测试里用 `CHANGE_SOP` 建 `StateKey`。
- `src/change/session.rs:19` 引入 `TestFailureContext`（`:146`/`:379` 签名，测试 `:1024`/`:1050` 直构）。
- `src/change/testing_gate.rs:14` 引入 `TestFailureContext`（`:142` `coding_failure_block`）。
- `src/change/mod.rs:21/42` 声明与转发。
- 集成测试 `tests/change_failure_context/main.rs:24` 至 `:26` 用 `CHANGE_SOP` / `ChangePhase` / `ChangeState` / `TestFailureContext` / `load` / `save`。

## 事实补充（2026-10-21 复核）

- import 行：`src/change/state.rs:9` `serde::{Deserialize, Serialize}`、`:11` `crate::error::Result`、`:12` `crate::runstate::{Loaded, RunState, StateStore}`（旧记 `:8`/`:10`/`:11` 为笔误）。
- 读写频次：`src/change/orchestrator.rs:160` 在 `run` 入口 `load` **一次**；`save` 只在 `:205`/`:232`/`:279`/`:320`/`:325`/`:347`/`:362` 七处。
- `baseline_sha` 回填：`src/change/orchestrator.rs:161`-163 在载入值为空时用本次入参回填一次（之后不再改）；与 `src/change/state.rs:55` 的「全程不动」注释存差异，以代码为准。
- `save` 的写入面：`write_into` 只 `insert` `data["change"]`（`src/change/state.rs:108`-118）；引擎侧 `save` 强制重写身份四元组 + `updated_at_ms`（`src/runstate/mod.rs:137`-142），`attempt` 保留；每 SOP 一份文件（`src/runstate/mod.rs:4`-5、`:23`-24）。
- `fresh_state` = `RunState::new(key, json!({}))`（`src/runstate/mod.rs:214`）。

## 可重放命令（2026-10-21 实跑）

- `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外独立目录> cargo test --lib change::state` → `10 passed; 0 failed; 590 filtered out`，EXIT=0；`rustc 1.99.0`；edition 2024（`Cargo.toml:4`）。

## 外部依赖

- `serde::{Deserialize, Serialize}`（`src/change/state.rs:9`）、`crate::error::Result`（`:11`）、`crate::runstate::{Loaded, RunState, StateStore}`（`:12`）。
- 无其他外部 crate；测试用 `tempfile` 与 `serde_json`（经 runstate 的 `data` 字段直构）。

## 事实复核（本轮重审前，逐条对齐源码行号）

- 调用点行号：`src/change/orchestrator.rs:33` `crate::observability::set_phase(to.as_str())`（`:30` 为 `phase_change` 定义）；`:242`-244 回退分支**先** `boundary::check_roundtrip(next)` **后** `bump_roundtrip()`；`:324`-329 testing 失败分支 `test_reroutes > MAX_TEST_REROUTES` 先 `save` 再报 `Quality`；`:21` `MAX_TEST_REROUTES = 3`。
- `roundtrips` 上限归属：常量与判定在 `src/change/boundary.rs:13`（`MAX_ROUNDTRIPS = 3`）与 `:160`；本模块的 `bump_roundtrip`（`src/change/state.rs:137`）只自增。
- `baseline_sha`：字段与注释在 `src/change/state.rs:55`-56；本模块无 setter；`src/change/orchestrator.rs:161`-163 仅在载入值为空时回填一次。
- `runstate` 侧行号复核：`src/runstate/mod.rs:48`-54 `StateKey::path`、`:136`-158 `save`（`:138`-142 强制重写身份四元组与 `updated_at_ms`）、`:214` `fresh_state`、`:89`-94 `matches`。
- `ChangePhase` 小写落盘证据：旧断点手写 `"phase": "coding"`（`src/change/state_tests.rs`）→ 载入断言 `Coding` 变体（`:320`）。
- 测试实跑（本轮）：`TMPDIR=/var/tmp/dfz-src21/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/var/tmp/dfz-src21/target cargo test --lib change::state` → `10 passed; 0 failed; 0 ignored; 588 filtered out`，EXIT=0，编译 39.00s（`filtered out` 随仓内测试总数变化，上轮为 590）。
