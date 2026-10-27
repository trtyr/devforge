---
type: module
module_path: src/change/session/session_tests/mod.rs
unit: unit-src-17
description: ProdChangeSession 测试共享夹具——脚本化 session 假实现 + 临时仓 Fixture
---

# src/change/session/session_tests/mod.rs

`ProdChangeSession` 测试的共享夹具（ARC-R6 把原 session.rs 内联 mod tests 平移成目录，纯移动）。本模块只提供**测试脚手架**：脚本化子会话假实现、计划样板、临时仓 Fixture、以及让 coding 段能过门的证据落盘助手。它自己不含 `#[test]`，用例在子模块 `plan` 与 `runtime` 里。

## 职责

**负责**：
- 脚本化假实现：`NoUser`（用户链路，问用户即失败）、`Agent`（`PlanAgentSession`，按队列回放并记录收到的消息）、`OneReview` 与 `ReviewFactory`（`ReviewSession` 工厂，每次 `open` 给一个共享队列的新实例——这正是「换新实例重审」可被断言的机制）。
- 计划样板与协议样例：`plan_text` 生成六块计划；`PLAN_PASS` / `PLAN_FAIL` 是审查协议的通过/判负样例。
- 临时仓与黑板：`fixture` 建临时仓的 src/a.rs、黑板目录 bb、以及仓内的计划样板文件，并按需 `git init` 出基线 sha。
- 过门预设：`coding_ready` 写基线证据与自证证据、写 impl-steps.md、改一个文件并提交（提交信息含 change-id 与 `step 1`）。
- 子模块挂载：`mod plan;`（`:195`）与 `mod runtime;`（`:196`）。

**不负责**：不含断言与用例（用例在子模块）；不生成生产行为；不替被测代码判门（门在 `src/change/coding_gate.rs` 等机械门里）。

## 公开成员

N/A —— 模块内没有任何 `pub` 项：`NoUser` / `Agent` / `OneReview` / `ReviewFactory` / `Fixture` 与 `plan_text` / `fixture` / `git` / `coding_ready` / `PLAN_PASS` / `PLAN_FAIL` 全是模块私有，仅经 `use super::*` 供 `plan` 与 `runtime` 两个子模块可见（`src/change/session/session_tests/mod.rs:1` 至 `:4`）。

索引（私有项，供查阅读用；见 [夹具清单](#夹具清单)）：

| 项 | 位置 |
|---|---|
| `NoUser` | `src/change/session/session_tests/mod.rs:11` |
| `Agent`（含 `new`） | `src/change/session/session_tests/mod.rs:18` |
| `OneReview` | `src/change/session/session_tests/mod.rs:37` |
| `ReviewFactory`（含 `new`） | `src/change/session/session_tests/mod.rs:47` |
| `PLAN_PASS` / `PLAN_FAIL` | `src/change/session/session_tests/mod.rs:68` / `:69` |
| `plan_text` | `src/change/session/session_tests/mod.rs:71` |
| `Fixture` | `src/change/session/session_tests/mod.rs:84` |
| `fixture` | `src/change/session/session_tests/mod.rs:89` |
| `git` | `src/change/session/session_tests/mod.rs:143` |
| `coding_ready` | `src/change/session/session_tests/mod.rs:157` |

## 依赖

- 上游（被测对象）：`use super::*`（`:4`）把 `src/change/session.rs` 的全部名字带进来——被测的 `ChangeCtx`、`ProdChangeSession`、`plan_kickoff`、`coding_kickoff`、`plan_exists` 等，以及生产 import 的 `Result` / `DevforgeError` / `DocLevel` / `TestFailureContext`。因此本模块不重复声明这些依赖。
- 下游（使用方）：`src/change/session/session_tests/plan.rs:3` 与 `src/change/session/session_tests/runtime.rs` 都以 `use super::*;` 继承夹具。
- 直接 import：`ReviewSession` 与 `ReviewSessionFactory`（`:5`）、`RefCell`（`:6`）、`VecDeque`（`:7`）、`Command`（`:8`）、`Rc`（`:9`）。
- 外部 crate：`tempfile`（`fixture` 里 `tempdir`，`src/change/session/session_tests/mod.rs:90`）；`git` 可执行文件（`fixture(true)` 与 `coding_ready`）；无网络、无 pi 进程。

## 夹具清单

- `NoUser`（`:11` 至 `:15`）：`impl UserLink`；`ask` 恒返回 `Err(DevforgeError::System(...))`，语义是「单测不应问用户」——任何误触用户链路的路径会立刻失败而不是挂住。
- `Agent`（`:18` 至 `:35`）：字段 `replies`（脚本队列）与 `seen`（收到的消息记录，`Rc<RefCell<...>>` 便于用例回看）；`converse` 先把 message 推进 `seen`，再 `pop_front` 取回放，队列耗尽即 panic（脚本与方法调用次数不匹配会立刻暴露）。
- `OneReview`（`:37` 至 `:46`）+ `ReviewFactory`（`:47` 至 `:66`）：工厂每次 `open` 造一个**新** `OneReview`，但队列与 `seen` 是共享的——所以「开了几个实例」（= 审查轮次）与「每个实例拿到什么指令」都能被断言。
- `PLAN_PASS`（`:68`）/ `PLAN_FAIL`（`:69`）：审查协议样例；`PLAN_FAIL` 带一条 `<issues>`「影响面漏了调用方」，供「判负回传」类用例断言。
- `plan_text`（`:71` 至 `:82`）：六块计划样板（需求复述 / `[自决]` 歧义条 / 影响面一条 src/a.rs:1 / 要改的 / 白名单 src/a.rs / 验收「给定 x=1 → 返回 2」 / 风险）。用例通过字符串替换改写歧义块（见 `src/change/session/session_tests/plan.rs:120` 的 `write_plan`）。
- `fixture(with_git: bool)`（`:89` 至 `:141`）：建临时仓与黑板、写计划文件；`with_git` 为真时 `git init` + `add -A` + 以 `user.name=t` / `user.email=t@localhost` 提交，并把 `rev-parse HEAD` 存进 ctx 的基线字段；`requirement` 固定为「把 a 改成 b」、文档档位为 None。
- `git(repo, args)`（`:143` 至 `:154`）：起 `git` 子进程，非零退出即 panic 并带 stderr。
- `coding_ready(f)`（`:157` 至 `:193`）：写基线证据与自证证据（两份都是 `command: cargo test` + `exit_code: 0`）、写 impl-steps.md（`## 步骤 1 改 a`）、改 src/a.rs 并以 `change: change-1 step 1` 提交，返回步骤数 `1`。语义：让后续三个门（基线 / 自证 / 步提交对账）都能过。

## 行为口径

- **失败即时暴露**：脚本队列耗尽、`git` 非零退出、误问用户都会 panic，用例不会静默通过。
- **隔离**：每个 `fixture` 一个 tempfile 临时目录（`Fixture` 持有 `tmp` 字段以延长生命周期，`:84` 至 `:87`）；不碰真实仓库、不写仓内文件。
- **可重放**：`cargo test --lib change::session` 是本模块全部用例的入口（需带 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`；`fixture` 的 `tempdir()` 依赖已存在的 `TMPDIR`，`src/change/session/session_tests/mod.rs:90`）。
