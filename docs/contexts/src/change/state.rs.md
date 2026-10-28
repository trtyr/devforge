---
type: module
module_path: src/change/state.rs
unit: unit-src-21
description: change run 断点——阶段/基线/计数/失败上下文，住黑板、跨会话续跑
---

# src/change/state.rs

change run 的检查点（P009 T011b；落点按 plan 010 D001/D022a：**引擎写、住黑板**）。状态本体走 `runstate` 的 `StateStore`（落 `plans/<project>/<task>/change/state.json`）。

分工（「引擎写」与「SOP 自己读写」不矛盾，两者分属不同层）：

| 归谁 | 内容 | 证据 |
|---|---|---|
| 引擎（`src/runstate/mod.rs`） | 落点路径、身份四元组校验、schema 版本、原子写（临时文件 + rename） | `src/runstate/mod.rs:12`（「本模块只认状态怎么存」）、`:136`-158 `save`、`:89` `matches` |
| 本模块（SOP 侧） | 扩展段 `data["change"]` 的结构与内容（本文件）、何时读写的调用点（`src/change/orchestrator.rs`） | `src/runstate/mod.rs:70`（「SOP 自己的扩展段：引擎不解释其中任何字段」） |

同一分工在 plan tree 里的出处：`docs/plantree/plans/009-change-sop/decisions/README.md:148`（D022「检查点由引擎写，agent 不碰」）与 `docs/plantree/plans/010-run-resume/decisions/README.md:43`（D005：引擎管状态文件位置/原子写/公共壳，SOP 管扩展段结构）。即：**落盘这件事归引擎，`data["change"]` 里写什么归本模块**——两者是分层，不是同一件事的两个写者。

续跑语义（`010 D002`：`start` 默认续跑，`fresh` 才重来）：`start` 默认从断点继续；`roundtrips` 跨会话保留，不因重启清零（`docs/plantree/plans/009-change-sop/roadmap.md:70` 的 T011b 把「往返」列为检查点内容）。

## 职责

**负责**：
- 断点数据结构：`ChangeState`（阶段、基线 sha、往返/回注/审查计数、已完成步骤与验收、change-id、失败上下文）。
- 状态扩展段的序列化/反序列化：`from_run` / `write_into`（挂在 `RunState` 的 `data` 字段 的 `"change"` 键下）。
- 计数与集合的幂等更新：`mark_step_done` / `mark_acceptance_done` / `bump_*`。
- 失败上下文的覆盖写与清除：`set_test_failure` / `clear_test_failure`。
- 读写断点：`load`（异常按冷启动）/ `save`（走 StateStore 原子写）。

**不负责**：不实现文件布局与原子写（`src/runstate/mod.rs` 的 `StateStore`）；不推进阶段（`src/change/orchestrator.rs`）；不判何时清上下文（编排层决定「消费即清/终态清」）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `CHANGE_SOP` | 常量 | [CHANGE_SOP](#change_sop) |
| `ChangePhase` | 枚举 | [ChangePhase](#changephase) |
| `as_str` | 函数（`ChangePhase` 的方法） | [as_str](#as_str) |
| `TestFailureContext` | 结构体 | [TestFailureContext](#testfailurecontext) |
| `ChangeState` | 结构体 | [ChangeState](#changestate) |
| `new` | 函数 | [new](#new) |
| `from_run` | 函数 | [from_run](#from_run) |
| `write_into` | 函数 | [write_into](#write_into) |
| `mark_step_done` | 函数 | [mark_step_done](#mark_step_done) |
| `mark_acceptance_done` | 函数 | [mark_acceptance_done](#mark_acceptance_done) |
| `bump_review` | 函数 | [bump_review](#bump_review) |
| `bump_roundtrip` | 函数 | [bump_roundtrip](#bump_roundtrip) |
| `bump_test_reroute` | 函数 | [bump_test_reroute](#bump_test_reroute) |
| `set_test_failure` | 函数 | [set_test_failure](#set_test_failure) |
| `clear_test_failure` | 函数 | [clear_test_failure](#clear_test_failure) |
| `load` | 函数 | [load](#load) |
| `save` | 函数 | [save](#save) |

## 依赖

- 上游 / 消费方：
  - `src/change/mod.rs:21` `pub mod state;`；`:42` `pub use state::{CHANGE_SOP, ChangePhase, ChangeState, TestFailureContext};`（`load` / `save` 走模块路径）。
  - `src/change/orchestrator.rs:15` 引入 `ChangePhase` / `ChangeState` / `TestFailureContext` / `load` / `save`。调用频次：`run` 在**入口 `load` 一次**（`src/change/orchestrator.rs:160`），之后只在阶段迁移/计数需要落盘的分支 `save`（如 `:205`、`:232`、`:279`、`:320`、`:325`（超限分支先存后返错）、`:347`、`:362`）；没有「每次进阶段前重新 load」。
  - `src/change/orchestrator.rs:150` `ChangeOutcome` 的 `state` 字段；`src/change/orchestrator_tests.rs:10`/`:478`（测试）用 `CHANGE_SOP` 建 `StateKey`。
  - `src/change/session.rs:19` 引入 `TestFailureContext`（`coding_kickoff` 的入参/送达面）。
  - `src/change/testing_gate.rs:14` 引入 `TestFailureContext`（`coding_failure_block`）。
  - `tests/change_failure_context/main.rs:24` 至 `:26` 用 `CHANGE_SOP` / `ChangePhase` / `ChangeState` / `TestFailureContext` / `load` / `save` 做仓外黑盒验证。
- 向下依赖（逐行核对 `src/change/state.rs` 的 import）：`serde::{Deserialize, Serialize}`（`:9`）、crate 错误类型 `Result`（`:11`）、`crate::runstate::{Loaded, RunState, StateStore}`（`:12`）。
- 外部 crate：生产 `serde`（经 crate 依赖）；测试用 `tempfile`。

## 核心口径

- **断点位置**：`plans/<project>/<task>/change/state.json`，由 `StateKey::new(project, task, CHANGE_SOP)` 决定（`CHANGE_SOP` 同时是注册名与目录名）。路径规则在 `src/runstate/mod.rs:48`-54（`<root>/<project>/<task>/<sop>/state.json`）；**每个 SOP 一份文件**，doc 与 change 各一份互不覆盖（`src/runstate/mod.rs:4`-5、`:23`-24）。
- **冷启动语义**：文件缺失（`Missing`）或损坏/身份不符/schema 过期（`Unusable`）一律按冷启动——`load` 返回 `new(change_id, baseline)`，不报错、不 panic。判定见 `src/runstate/mod.rs:89`-99（`matches` 同时比 `schema_version` 与 `sop`/`project`/`task`）与 `:26`（`SCHEMA_VERSION`）。
- **向前兼容**：除 `phase`/`baseline_sha` 外所有字段带 `#[serde(default)]`；`last_test_failure: Option<T>` 缺键反序列化为 `None`（serde 对 Option 缺键不报 `missing_field`），旧断点照常加载、计数不丢。
- **幂等**：`mark_step_done` 去重并升序排序；`mark_acceptance_done` 精确去重；`bump_*` 是纯自增（重复调用会真的多加，调用方负责只在事件发生时调用一次）。
- **基线语义**：`baseline_sha` 由门 0 建立、回滚与 diff 都锚它（`src/change/state.rs:55`-56 的字段与注释）。字段本身没有 setter，本模块从不改写它；唯一例外在编排层——载入值为空（旧断点 / 首次）时用本次入参回填一次，此后不再变（🟢 CONFIRMED `src/change/orchestrator.rs:161`-163）。
- **失败上下文覆盖写**：新一轮回注的 issues 覆盖旧的，不追加（test agent 每轮重跑全套，追加会把已修好的旧失败继续喂回去）。
- **跨 SOP 隔离**：不同 SOP 用不同 `StateKey` 目录，别的 SOP 的断点不会被当成本 SOP 的状态。

---

### CHANGE_SOP

- 名称/类型：`pub const CHANGE_SOP: &str`。（🟢 CONFIRMED `src/change/state.rs:15`）
- 取值：`change`（无其它合法值；这是常量，**不存在「必填/默认值」语义**）。
- 影响：`StateKey::new(project, task, CHANGE_SOP)` → 断点落 `<root>/<project>/<task>/change/state.json`；同时是 SOP 注册名。
- 示例：测试用 `StateStore::new(root, StateKey::new("proj", "task", CHANGE_SOP))`，`path()` 以 proj/task/change/state.json 结尾（`src/change/state.rs:209`）。

### ChangePhase

- 职责：change run 的阶段，字符串形式落盘（`#[serde(rename_all = "lowercase")]`）便于人读与向前兼容。
- 构造 / 初始化：四个变体 `Plan` / `Coding` / `Testing` / `Done`；反序列化自字符串（`plan`/`coding`/`testing`/`done`）。
- 公开成员：仅一个方法 `as_str`（单独成节：[as_str](#as_str)）；字段仅四个无载荷变体，无可变状态。
- 生命周期：`Copy` 值对象，随 `ChangeState` 落盘。
- 典型用法：编排层 `phase_change(state, ChangePhase::Coding)`；观测层用 `as_str()` 写心跳阶段字段（`src/change/orchestrator.rs:33`）。
- 派生 trait：`Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`。（🟢 CONFIRMED `src/change/state.rs:20`）

### as_str

- 摘要：把阶段变体转成落盘/日志用的稳定小写字符串（`plan` / `coding` / `testing` / `done`）。（🟢 CONFIRMED `src/change/state.rs:28`）
- 行为语义：纯映射，`match` 四个变体各返一个 `&'static str`；无分配、无 IO、无副作用；因是穷举 match，新增变体会编译报错（不会默默返回旧值）。它与 `#[serde(rename_all = "lowercase")]` 的落盘字符串**一致**，所以「心跳里的阶段名」与「state.json 里的 phase」能对上号。
- 参数：`&self`（阶段变体本身）；无其它参数。
- 返回：`&'static str`——固定四个字面量之一（`plan` / `coding` / `testing` / `done`，与 `#[serde(rename_all = "lowercase")]` 的落盘值同形）。
- 异常：N/A（穷举 match，无失败路径）。
- 示例：`ChangePhase::Testing.as_str() == "testing"`；编排层 `src/change/orchestrator.rs:33` 用 `as_str()` 写观测阶段。与落盘字符串一致性的证据在反序列化侧：旧断点里写成 `"phase": "coding"`（`src/change/state_tests.rs`），载入后断言为 `Coding` 变体（`src/change/state_tests.rs`）。

### TestFailureContext

- 职责：上一轮测试失败的上下文（回注给 coding agent 的送达面内容）。
- 构造 / 初始化：由 `set_test_failure` 写入；字段 `pub` 可直构；`Deserialize` 可从断点恢复。
- 公开成员：
  - `round: usize` — 第几轮测试→coding 回注（首次 = 1，与 `test_reroutes` 同值）。
  - `issues: Vec<String>` — 失败明细**逐条原文**（含 path:line，不剥不改写）。
- 生命周期：住在断点里；跨会话保留（崩溃/重启后同一轮送达同一份）。
- 典型用法：`coding_kickoff(&ctx, Some(&failure))` 把明细渲染进送达块。
- 派生 trait：`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`。（🟢 CONFIRMED `src/change/state.rs:44`）

### ChangeState

- 职责：change run 的断点内容（阶段 + 基线 + 计数 + 已完成集合 + change-id + 失败上下文）。
- 构造 / 初始化：`new`（冷启动）、`from_run`（从 `RunState.data["change"]` 恢复）、或反序列化。
- 公开成员：
  - `phase: ChangePhase` — 当前阶段。
  - `baseline_sha: String` — 门 0 记下的基线 sha，回滚与 diff 都锚它。**基本不变**（源码注释口径）——唯一例外：从旧断点载入为空时，编排层用本次入参回填一次，此后不再变（🟢 CONFIRMED `src/change/orchestrator.rs:161`-163：`if state.baseline_sha.is_empty() { state.baseline_sha = run.baseline_sha.to_string() }`）。
  - `roundtrips: usize` — plan↔coding 往返次数（上限 3，由 `check_roundtrip` 判，`src/change/boundary.rs:160`）。
  - `impl_steps_done: Vec<u32>` — 已完成实施步骤号（升序）。
  - `acceptance_done: Vec<String>` — 已对账通过的验收项原文。
  - `review_rounds: usize` — 累计审查轮次。
  - `test_reroutes: usize` — 测试→coding 回注次数（跨会话累计）。
  - `change_id: String` — 跨会话核对同一件事。
  - `last_test_failure: Option<TestFailureContext>` — 失败上下文（`None` = 无）。
- 生命周期：每次 `run` 入口 `load` **一次**（`src/change/orchestrator.rs:160`）；阶段迁移/计数变化的分支才 `save`（见「依赖」节的调用点列表）；序列化进 `RunState.data["change"]`。
- 典型用法：`let mut st = load(store, id, base); st.bump_roundtrip(); save(store, &st)?;`
- 派生 trait：`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`。（🟢 CONFIRMED `src/change/state.rs:53`）

### new

- 摘要：冷启动构造：`phase=Plan`、计数全 0、集合空、`last_test_failure=None`。
- 行为语义：`change_id`/`baseline_sha` 取入参（`impl Into<String>`）；幂等、无 IO。（🟢 CONFIRMED `src/change/state.rs:84`）
- 参数：`change_id: impl Into<String>`、`baseline_sha: impl Into<String>`。
- 返回：`ChangeState`。
- 异常：N/A（纯构造）。
- 示例：`ChangeState::new("c1", "sha1")` → phase=Plan、roundtrips=0（`src/change/state.rs:224`）。

### from_run

- 摘要：从 `RunState.data["change"]` 抠出扩展段；缺失或反序列化失败就按入参新建。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:99`）：`data.get("change")` → `serde_json::from_value(...).ok()` → 成功用旧值，否则 `new(change_id, baseline)`。宽松（坏数据静默冷启动，不报错）。
- 参数：`state: &RunState`、`change_id: &str`、`baseline: &str`。
- 返回：`ChangeState`。
- 异常：N/A（不失败）。
- 示例：旧断点缺 `last_test_failure` 键仍能恢复其余字段（`src/change/state_tests.rs`）。

### write_into

- 摘要：把 `self` 写回 `RunState.data["change"]`。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:108`）：`data` 不是 object 时先重置为 `{}`；`to_value(self)` 失败时写 `Null`（不 panic）。无 IO。
- 参数：`state: &mut RunState`。
- 返回：N/A（`()`）。
- 异常：N/A（序列化失败降级为 `Null`）。
- 示例：`save` 内部调用（`src/change/state.rs:168`）。

### mark_step_done

- 摘要：登记已完成实施步骤号（幂等、保持升序）。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:120`）：不存在才 push，随后 `sort_unstable`；重复调用只留一份。
- 参数：`step: u32`。
- 返回：N/A。
- 异常：N/A。
- 示例：`mark_step_done(2)` 后 `mark_step_done(1)` → `[1,2]`（`src/change/state.rs:191`）。

### mark_acceptance_done

- 摘要：登记已对账通过的验收项（精确去重，保持插入顺序）。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:127`）：`acceptance_done` 中没有完全相同的串才 push。
- 参数：`criterion: &str`。
- 返回：N/A。
- 异常：N/A。
- 示例：同一条目登记两次仍只有一条（`src/change/state.rs:255`）。

### bump_review

- 摘要：累计审查轮次 +1。
- 行为语义：`review_rounds += 1`，纯自增、无去重。（🟢 CONFIRMED `src/change/state.rs:133`）
- 参数：N/A（`&mut self`）。
- 返回：N/A。
- 异常：N/A。
- 示例：测试里 `bump_review()` 后 roundtrip 全等校验（`src/change/state.rs:191`）。

### bump_roundtrip

- 摘要：plan↔coding 往返次数 +1。
- 行为语义：`roundtrips += 1`；上限判定不在本模块——`check_roundtrip` 用 `MAX_ROUNDTRIPS = 3` 在超限时报 `Quality`（`src/change/boundary.rs:13`、`:160`），编排层在回退分支先判后自增（`src/change/orchestrator.rs:242`-244）。（🟢 CONFIRMED `src/change/state.rs:137`）
- 参数：N/A（`&mut self`）。
- 返回：N/A。
- 异常：N/A。
- 示例：连调两次 → 2，跨会话仍为 2（`src/change/state.rs:234`）。

### bump_test_reroute

- 摘要：测试→coding 回注次数 +1。
- 行为语义：`test_reroutes += 1`；跨会话累计（防无休止互推）。上限不在本模块：编排层在 testing 失败分支比 `MAX_TEST_REROUTES = 3`，超限报 `Quality`（`src/change/orchestrator.rs:21`、`:324`-328）。（🟢 CONFIRMED `src/change/state.rs:141`）
- 参数：N/A（`&mut self`）。
- 返回：N/A。
- 异常：N/A。
- 示例：旧断点恢复后 `test_reroutes=2` 不丢（`src/change/state_tests.rs`）。

### set_test_failure

- 摘要：写入失败上下文（**覆盖**语义）。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:149`）：`last_test_failure = Some(TestFailureContext { round, issues })`——新 issues 覆盖旧的，不追加。
- 参数：`round: usize`（第几轮回注）、`issues: Vec<String>`（明细原文）。
- 返回：N/A。
- 异常：N/A。
- 示例：先 `(1, ["旧失败"])` 再 `(2, ["新失败"])` → round=2、issues 只有「新失败」（`src/change/state_tests.rs`）。

### clear_test_failure

- 摘要：清除失败上下文（消费即清 / 终态清）。
- 行为语义：`last_test_failure = None`。（🟢 CONFIRMED `src/change/state.rs:154`）
- 参数：N/A（`&mut self`）。
- 返回：N/A。
- 异常：N/A。
- 示例：清除后 `last_test_failure == None`（`src/change/state_tests.rs`）。

### load

- 摘要：读断点；异常按冷启动。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:160`）：`store` 的 `load()` 为 `Found` → `ChangeState::from_run(...)`；`Missing` / `Unusable` → `new(change_id, baseline)`。不返回错误。
- 参数：`store: &StateStore`、`change_id: &str`、`baseline: &str`。
- 返回：`ChangeState`。
- 异常：N/A（冷启动兜底）。
- 示例：无断点时 phase=Plan、roundtrips=0、baseline 取入参（`src/change/state.rs:224`）。

### save

- 摘要：原子写回断点（走 `StateStore`：临时文件 + rename，由 runstate 实现）。
- 行为语义（🟢 CONFIRMED `src/change/state.rs:168`）：先把断点读成 `RunState`（局部变量 `run`）——`Found` 就用它，否则 `fresh_state()`（`src/runstate/mod.rs:214`）；再 `state.write_into(&mut run)`；最后 `store.save(&mut run)?`。后果：
  - **只覆盖 `data` 里的 `"change"` 键**，`data` 里的其它键保留（`src/change/state.rs:108`-118 的 `insert` 语义）；公共壳字段里 `attempt` 沿用旧值，`schema_version` / `sop` / `project` / `task` / `updated_at_ms` 由引擎强制重写（`src/runstate/mod.rs:138`-142）。
  - 由于每个 SOP 一份文件，「别的 SOP 的段」不会出现在同一 `data` 里（隔离由路径保证，见「断点位置」）——不会互相覆盖是目录级事实，不是本函数的合并逻辑。
- 参数：`store: &StateStore`、`state: &ChangeState`（要落盘的扩展段快照；函数内读到的 `RunState` 变量叫 `run`，与 `ChangeState` 不是同一类型）。
- 返回：`Result<()>`。
- 异常：`Result` 透传 `store` 的 `save` 的错误（crate 错误类型，落盘失败）。
- 示例：save 后 `path()` 处出现 state.json（`src/change/state.rs:209`）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `roundtrip_persists_phase_baseline_and_counters` | save→load 全等、步骤升序 | `src/change/state.rs:191` |
| `checkpoint_lives_on_blackboard_path` | 断点落黑板路径 | `src/change/state.rs:209` |
| `missing_checkpoint_means_cold_start` | 无断点 → 冷启动 | `src/change/state.rs:224` |
| `resume_keeps_roundtrips_across_sessions` | 往返计数跨会话保留 | `src/change/state.rs:234` |
| `counters_are_idempotent` | 步骤/验收去重 | `src/change/state.rs:255` |
| `failure_context_persists_across_sessions` | 上下文跨会话逐字保留 | `src/change/state_tests.rs` |
| `old_checkpoint_without_failure_key_still_loads` | 旧断点缺键仍加载、计数不丢 | `src/change/state_tests.rs` |
| `failure_context_is_overwritten_not_appended` | 覆盖写不追加 | `src/change/state_tests.rs` |
| `clearing_failure_context_empties_it` | 清除后为 None | `src/change/state_tests.rs` |
| `foreign_state_is_ignored` | 其它 SOP 断点不污染 | `src/change/state_tests.rs` |

## 测试运行与环境

- 测试与被测代码同文件：`#[cfg(test)] mod tests`（`src/change/state.rs:178` 起），`#[test]` 共 10 例，上表逐例给出位置。
- **环境前提**：edition 2024（`Cargo.toml:4`）⇒ 需 Rust **1.85+** 且 toolchain 已安装（rustup 无默认 toolchain 时必须显式指定，如 `RUSTUP_TOOLCHAIN=stable`）。本次实测：`rustc 1.99.0` / `cargo 1.99.0`。
- **依赖获取**：`cargo fetch`（首次需网络；本模块的测试只需 `tempfile`，`Cargo.toml:27`；生产无新增外部 crate）。
- **运行单模块单测**（🟢 CONFIRMED，本轮实跑：`cargo test --lib change::state` → `10 passed; 0 failed; 0 ignored; 588 filtered out`，EXIT=0；编译 39.00s，`rustc 1.99.0`）：

  ```bash
  CARGO_TARGET_DIR=<仓外独立空目录> cargo test --lib change::state
  ```

  预期输出：`test result: ok. 10 passed; 0 failed; 0 ignored; 588 filtered out`（`filtered out` 的数随仓内测试总数变化，不是本模块的断言），用例名与上表十例一一对应。⚠️ `/tmp` 容量小时把 `TMPDIR` 指到宽裕目录（否则大量临时目录创建可能写失败）。
- 运行依赖：`tempfile`（dev-dependency，`Cargo.toml:27`）作 `StateStore` 根；`serde_json`（测试直接拼旧断点 `data`，如 `src/change/state_tests.rs`-312，属 crate 已有依赖）；不需要 git、外部服务、端口或网络。
- 每个用例独立 `tempdir`，`StateKey` 用 `("proj","task",CHANGE_SOP)`，可并行跑。
- **本单元的 Runbook 边界**：本节承担环境 / 依赖获取 / 测试运行与隔离三件；**N/A** 项与理由——启停与健康检查：库内模块，无独立进程；配置项：无运行时可配项（路径由 `StateKey` 入参决定，常量 `CHANGE_SOP` 不外部可调）；种子数据：无预置种子（用例自行构造断点或空目录）；公开接口清单：上文「公开成员」表即全部（无 HTTP/CLI 端点）。
