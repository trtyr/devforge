# 勘察事实 — unit-src-22（src/change/testing_gate.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/change/testing_gate.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。
> 复核时间：本轮（2026-10-21 之后的重写轮）；源码 280 行，逐行读过。

## 文件与模块位置

- 文件：`src/change/testing_gate.rs`，280 行，无子模块。
- 文件头注释四条口径（`:1`-`:9`）：回归优先 / 验收对账 / 回注带失败明细原文 / D025 改写 D018-D019「不做定位」口径（两个路由目标同口径）。
- 声明：`src/change/mod.rs:22` `pub mod testing_gate;`。
- 转发面（`src/change/mod.rs:43`-46）：`AcceptanceMatch, FAILURE_BLOCK_TITLE, check_regression_claim, check_regression_log, coding_failure_block, failure_details, reconcile_acceptance, route_back_message`（8 个名字）。**`REGRESSION_LOG` 与 `parse_acceptance_table` 不在转发面**，消费者走模块路径。
- `use`（`:11`-15）：`std::path::Path`、`crate::change::coding_gate::parse_baseline_log`、`crate::change::state::TestFailureContext`、`crate::error::{DevforgeError, Result}`。生产无外部 crate；测试用 `tempfile`。

## 公开条目（逐个，行号取自当前文件）

| 符号 | 行 | 形状 |
|---|---|---|
| `REGRESSION_LOG` | `:18` | `pub const &str = "evidence/regression.log"` |
| `AcceptanceMatch` | `:21`-:25 | `#[derive(Debug, Clone, PartialEq, Eq)]`，字段 `pub criterion: String`（`:23`）、`pub test_ref: String`（`:24`） |
| `check_regression_claim` | `:28` | `pub fn(&str, bool) -> Result<i32>` |
| `check_regression_log` | `:45` | `pub fn(&Path, bool) -> Result<i32>` |
| `parse_acceptance_table` | `:57` | `pub fn(&str) -> Vec<AcceptanceMatch>` |
| `reconcile_acceptance` | `:77` | `pub fn(&[String], &[AcceptanceMatch]) -> Result<usize>` |
| `route_back_message` | `:118` | `pub fn(&str, &[String]) -> String` |
| `failure_details` | `:128` | `pub fn(&[String]) -> String` |
| `FAILURE_BLOCK_TITLE` | `:137` | `pub const &str = "上一轮测试失败明细如下（逐条原文）"` |
| `coding_failure_block` | `:142` | `pub fn(&TestFailureContext) -> String` |

## 函数内部可见的分支（只记分支，不解释动机）

- `check_regression_claim`（`:28`-:43）：调 `parse_baseline_log`，`map_err` 包成 `Quality("回归日志不可解析（要写 `exit_code: N`）：{e}")`；`ev.exit_code != 0 && claims_green` → `Quality("回归日志退出码 {n} 与「回归通过」的自述矛盾（命令：{cmd}）")`，`cmd` 空则填「未记录」；其余一律 `Ok(ev.exit_code)`（含：日志绿 + `claims_green=false` → `Ok(0)`，不判矛盾）。
- `check_regression_log`（`:45`-:55）：`blackboard.join(REGRESSION_LOG)` → `std::fs::read_to_string`，读失败 → `Quality("缺少回归证据 {path}（测试阶段必须先跑既有全套）：{e}")`；成功转 `check_regression_claim`。
- `parse_acceptance_table`（`:57`-:75）：`text.lines()` → `line.trim().trim_start_matches(['-', '*']).trim()` → `rsplit_once('→')`（最后一个箭头）→ 无箭头 `continue`；`criterion = left.trim()`；`test_ref = right.trim().trim_matches('\`')`；两者都非空才 push。
- `reconcile_acceptance`（`:77`-:116`）：`criteria.is_empty()` → `Quality("计划里没有验收标准，无从对账（计划阶段就该写出来）")`；逐条命中判定 `m.criterion == *c || m.criterion.contains(c) || c.contains(m.criterion)`，未命中收进 `missing`；`missing` 非空 → `Quality("验收对账缺 {n} 条（缺一条即零覆盖）：{join(；)}")`；随后 `table` 里 `test_ref.trim().is_empty()` 的条目收进 `empty_refs`，非空 → `Quality("验收项没有指到测试 path:line：{join}")`；通过 `Ok(criteria.len())`。
- `route_back_message`（`:118`-:125`）：`format!("测试阶段未通过，按 <route>{target}</route> 回去处理。\n失败明细：\n{}\n", failure_details(failures))`。
- `failure_details`（`:128`-:135`）：每项 `format!("- {f}")`，`join("\n")`；空切片 → `""`。
- `coding_failure_block`（`:142`-:148`）：`format!("{}：\n{}", FAILURE_BLOCK_TITLE, failure_details(&failure.issues))`；`failure.round` 未被读取。

## 仓内消费者（grep 全仓 `--include=*.rs`，排除本文件）

- `src/change/session.rs:20`-22 use 块引入 `check_regression_log` / `parse_acceptance_table` / `REGRESSION_LOG`。
- `src/change/session.rs:136`（`testing_review_instruction` 的 format 实参）、`:194`（`testing_kickoff` 的 format 实参）把 `REGRESSION_LOG` 拼进提示词。
- `src/change/session.rs:165` `coding_failure_block(f)` 拼进 `coding_kickoff` 的送达块。
- `src/change/session.rs:530` `check_regression_log(&self.ctx.blackboard, true)?`（声称通过后的回归门）。
- `src/change/session.rs:536` `parse_acceptance_table(&table_text)`；`:538`-545 逐条 `plan_audit::path_refs(&m.test_ref).is_empty()` → `Quality`。
- `src/change/session.rs:1006` / `:1033` / `:1060`（`#[cfg(test)]`）用 `FAILURE_BLOCK_TITLE` / `coding_failure_block` 做正反断言。
- `src/change/orchestrator.rs:333` `route_back_message("coding", &failures)` → 同一 `msg` 进 `:334` `tracing::warn!` 与 `:356` `Quality` 文本；`:345` `state.set_test_failure(round, failures.clone())`。
- `src/change/orchestrator.rs:360` `route_back_message("plan", &failures)` → `:364` `Quality` 文本。
- `src/change/coding_gate_tests.rs`（`#[cfg(test)]`）调 `check_regression_claim` 钉「回归日志不可解析」文案；`:427` / `:502` 注释声明 testing_gate 是 `parse_baseline_log` 的第二调用方。
- `tests/change_failure_context/main.rs:27` 引入 `FAILURE_BLOCK_TITLE` / `route_back_message`；`tests/change_failure_context/context.rs:39` 无上下文时不得出现标题；`:59` `coding_failure_block`；`:240` / `:254` 两个集成用例调 `route_back_message`。
- `reconcile_acceptance`：本文件外只出现在 `src/change/mod.rs:45` 的 re-export，**无生产调用点**；运行时覆盖率汇总是 `summarize_acceptance`（`src/change/report.rs:125`，精确 `d == *c`，`src/change/report.rs:128`），由 `src/change/orchestrator.rs:309` / `:372` 调用。

## 交付面三出口（同一失败的三处落点）

1. 日志面：`src/change/orchestrator.rs:334` `tracing::warn!(%msg, …)`。
2. 错误面：`src/change/orchestrator.rs:356`（coding）/ `:364`（plan）的 `Quality` 文本内嵌同一 `msg`。
3. 送达面：`state.set_test_failure`（`:345`）→ `TestFailureContext`（`src/change/state.rs:44`-49，字段 `round: usize` / `issues: Vec<String>`）→ 下一轮 `coding_failure_block`（`src/change/session.rs:165`）。
渲染共用 `failure_details`（`src/change/testing_gate.rs:128`）。

## 测试（`#[cfg(test)] mod tests`，`:150` 起，12 例）

| 测试 | 行 | 断言要点 |
|---|---|---|
| `red_regression_contradicting_the_claim_fails` | `:155` | `exit_code: 1` + `true` → 文案含「矛盾」与 `cargo test`，`is_quality_failure()` 真 |
| `red_regression_with_honest_claim_is_fine` | `:164` | `exit_code: 2` + `false` → `Ok(2)` |
| `green_regression_passes` | `:171` | `exit_code: 0` + `true` → `Ok(0)` |
| `missing_regression_log_is_quality_failure` | `:177` | 空临时目录 → 文案含「缺少回归证据」 |
| `acceptance_table_parses_arrows` | `:184` | 2 行 → 2 条；`test_ref` 去反引号；`criterion` 为 `空输入报错` |
| `one_missing_criterion_is_zero_coverage` | `:194` | 缺 1 条 → 文案含「缺 1 条」与缺项原文 |
| `full_reconciliation_passes` | `:204` | 全覆盖 → `Ok(1)` |
| `empty_criteria_is_rejected` | `:211` | `&[]` → 文案含「没有验收标准」 |
| `route_back_message_carries_the_failure_details_verbatim` | `:217` | 含 `<route>coding</route>`、`tests/test_a.py:12`、`接口返回 500`；不含「<位置略>」「请自己定位原因」「本消息不提供文件位置」；`plan_audit::path_refs` 能从消息里解析出 `("tests/test_a.py", 12)` |
| `route_to_plan_carries_the_same_details` | `:240` | `plan` 目标同样带 `path:line`、无零信息话术 |
| `failure_details_render_one_issue_per_line_verbatim` | `:250` | 等于 `- a.rs:1 失败\n- b.rs:2 失败` |
| `coding_failure_block_carries_the_title_and_verbatim_issues` | `:260` | 含标题、两条明细；无零信息话术；`TestFailureContext{round:1, issues:[…]}` 直构 |

- 未被任何用例覆盖的分支：`reconcile_acceptance` 的 `empty_refs` 分支（`:107`-:114`）——`parse_acceptance_table` 会先丢弃空 `test_ref` 行，因此只能靠手工构造 `AcceptanceMatch` 才会走到。

## 可重放命令（本轮实跑）

```
TMPDIR=/var/tmp/dfz-src22b/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable \
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/var/tmp/dfz-src22b/target \
cargo test --lib change::testing_gate
```

- 结果：`test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 586 filtered out; finished in 0.00s`，EXIT=0，编译 38.78s。
- `rustc 1.99.0 (b940084d7 2026-09-28)`；`edition = "2024"`（`Cargo.toml:4`）。
