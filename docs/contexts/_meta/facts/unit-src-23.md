# 勘察事实 — unit-src-23（src/change/testing_gate.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/testing_gate.rs.md` 与 `_meta/gaps.md`。
> 行号口径：当前工作区实测（文件 280 行，mtime 2026-10-21）。

## 文件级

- 文件头注释（`:1`-`:9`）：测试阶段机械门（P009 T011 / D017-D019）；三条口径——回归优先、验收对账、回注带失败明细原文（D025 改写 D018/D019「不做定位」，两个路由目标同口径）。
- import（`:11`-`:15`）：`std::path::Path`（`:11`）、`change::coding_gate::parse_baseline_log`（`:13`）、`change::state::TestFailureContext`（`:14`）、`error::{DevforgeError, Result}`（`:15`）。
- 生产代码无外部 crate；测试用 `tempfile`（`:177`）。
- 模块声明 `src/change/mod.rs:22` `pub mod testing_gate;`；转发 `src/change/mod.rs:43`-46（8 个名字：`AcceptanceMatch`、`FAILURE_BLOCK_TITLE`、`check_regression_claim`、`check_regression_log`、`coding_failure_block`、`failure_details`、`reconcile_acceptance`、`route_back_message`）。`REGRESSION_LOG` 与 `parse_acceptance_table` 不经转发面。

## 公开条目

| 符号 | 位置 | 形态 / 行窗口 |
|---|---|---|
| `REGRESSION_LOG` | `:18` | `pub const &str = "evidence/regression.log"`（文档注释 `:17`） |
| `AcceptanceMatch` | `:22` | `#[derive(Debug, Clone, PartialEq, Eq)]`（`:21`）`pub struct { criterion: String :23, test_ref: String :24 }` |
| `check_regression_claim` | `:28` | `pub fn(&str, bool) -> Result<i32>`；`:29`-`:33` 解析并包 `Quality("回归日志不可解析（要写 \`exit_code: N\`）：{e}")`；`:34`-`:40` `exit_code != 0 && claims_green` → `Quality("回归日志退出码 {} 与「回归通过」的自述矛盾（命令：{}）")`；`:41` `Ok(ev.exit_code)` |
| `check_regression_log` | `:45` | `pub fn(&Path, bool) -> Result<i32>`；`:46` 路径拼接；`:47`-`:52` 读失败 → `Quality("缺少回归证据 {}（测试阶段必须先跑既有全套）：{e}")`；`:53` 转 `check_regression_claim` |
| `parse_acceptance_table` | `:57` | `pub fn(&str) -> Vec<AcceptanceMatch>`；`:60` 行 trim + 去 `-`/`*`；`:61` `rsplit_once('→')` 无箭头则跳过；`:64`/`:65` 左右半 trim，右侧再 `trim_matches('`')`；`:66`-`:71` 两侧非空才 push |
| `reconcile_acceptance` | `:77` | `pub fn(&[String], &[AcceptanceMatch]) -> Result<usize>`；`:78`-`:82` 空 criteria → `Quality("计划里没有验收标准，无从对账（计划阶段就该写出来）")`；`:85`-`:87` 命中判定 = `==` 或双向 `contains`；`:92`-`:98` 缺项 → `Quality("验收对账缺 {} 条（缺一条即零覆盖）：{}")`；`:99`-`:110` 空引用 → `Quality("验收项没有指到测试 path:line：{}")`；`:111` `Ok(criteria.len())` |
| `route_back_message` | `:118` | `pub fn(&str, &[String]) -> String`；`:119`-`:122` `format!("测试阶段未通过，按 <route>{target}</route> 回去处理。\n失败明细：\n{}\n", failure_details(failures))` |
| `failure_details` | `:128` | `pub fn(&[String]) -> String`；`:129`-`:133` 每项 `- {f}`，`join("\n")` |
| `FAILURE_BLOCK_TITLE` | `:137` | `pub const &str = "上一轮测试失败明细如下（逐条原文）"`（文档注释 `:136`） |
| `coding_failure_block` | `:142` | `pub fn(&TestFailureContext) -> String`；`:143`-`:147` `format!("{}：\n{}", FAILURE_BLOCK_TITLE, failure_details(&failure.issues))` |

## 测试（`#[cfg(test)] mod tests`，`:150`-`:151` 起，12 例）

| 用例 | 位置 | 断言事实 |
|---|---|---|
| `red_regression_contradicting_the_claim_fails` | `:155` | 红 + 称绿 → err 含「矛盾」与「cargo test」，`is_quality_failure()` 真 |
| `red_regression_with_honest_claim_is_fine` | `:164` | 红 + 如实 → `Ok(2)` |
| `green_regression_passes` | `:171` | 绿 → `Ok(0)` |
| `missing_regression_log_is_quality_failure` | `:177` | 空临时目录 → err 含「缺少回归证据」 |
| `acceptance_table_parses_arrows` | `:184` | 两行表 → 2 条；第一条 `test_ref == "tests/test_a.py:12"`；第二条 `criterion == "空输入报错"`（反引号被剥） |
| `one_missing_criterion_is_zero_coverage` | `:194` | 缺一条 → err 含「缺 1 条」与缺项原文 |
| `full_reconciliation_passes` | `:204` | 全覆盖 → `Ok(1)` |
| `empty_criteria_is_rejected` | `:211` | `&[]` → err 含「没有验收标准」 |
| `route_back_message_carries_the_failure_details_verbatim` | `:217` | 含 `<route>coding</route>`、两条明细逐字、不含「<位置略>」「请自己定位原因」「本消息不提供文件位置」；`plan_audit::path_refs(&msg)` 能解出 `("tests/test_a.py", 12)` |
| `route_to_plan_carries_the_same_details` | `:240` | plan 目标同样带明细与定位，同样不出现零信息话术 |
| `failure_details_render_one_issue_per_line_verbatim` | `:250` | 渲染 == `- a.rs:1 失败\n- b.rs:2 失败` |
| `coding_failure_block_carries_the_title_and_verbatim_issues` | `:260` | 块含 `FAILURE_BLOCK_TITLE`、两条明细逐字、无三种零信息话术；`TestFailureContext` 在用例内直构（`:262`-`:268`） |

- 未被覆盖：`reconcile_acceptance` 的空引用分支（`:99`-`:110`）无用例——`parse_acceptance_table` 已丢弃空 `test_ref` 行，只能手工构造 `AcceptanceMatch` 才能走到。

## 消费方（已 grep 复核）

- `src/change/session.rs:20`-`:22` 引入 `check_regression_log` / `parse_acceptance_table` / `REGRESSION_LOG`；`:136` 把 `REGRESSION_LOG` 写进 testing review 指令；`:194` 写进 testing 开工语；`:165` 用 `coding_failure_block` 拼 coding 送达块；`:530` 声称通过后 `check_regression_log(bb, true)`；`:532`-`:535` 读对账表、`:536` 解析、`:539`-`:546` 逐行 `plan_audit::path_refs` 判可解析。
- `src/change/orchestrator.rs:333` `route_back_message("coding", &failures)` → `:334` `tracing::warn!(%msg)` → `:345` `state.set_test_failure(...)` → `:355`-`:357` `Quality` 文本内嵌同一 `msg`；`:360` `route_back_message("plan", ...)` → `:363`-`:365` 同样内嵌。
- `tests/change_failure_context/main.rs:27` 引入 `FAILURE_BLOCK_TITLE` / `route_back_message`；`tests/change_failure_context/context.rs:39` 断言无失败上下文时开工语不含标题行、`:59` 断言有上下文时等于「kickoff + 空行 + `coding_failure_block`」。
- `src/change/session/session_tests/plan.rs:232` / `:259` 以 `FAILURE_BLOCK_TITLE` 为反向/正向锚点；`:286` 断言 `coding_kickoff` 只追加 `coding_failure_block`。

## 下游依赖

- `src/change/coding_gate.rs:104` `parse_baseline_log` → 内部 `parse_evidence_log`（`:37`-`:99`；用 `BASELINE_LOG` + 标签「基线」拼错误文案）；`src/change/coding_gate.rs:554`-`:563` 的 `regression_claim_text_is_unchanged` 钉住「回归路径文案以『回归日志不可解析』开头」。
- `src/change/state.rs:44` `TestFailureContext`（`round: usize` + `issues: Vec<String>`）；本模块只用 `issues`。
- `src/error.rs` 的 `DevforgeError::Quality` / `Result`；测试用 `is_quality_failure`（`src/error.rs:74`）。
