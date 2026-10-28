---
type: module
module_path: src/change/testing_gate.rs
unit: unit-src-23
description: 测试阶段机械门——回归证据判定、验收对账、失败明细回注（原文不下修）
---

# src/change/testing_gate.rs

测试阶段的机械门（文件头注释 `src/change/testing_gate.rs:1` 至 `:9`，P009 T011 / D017-D019）：**回归优先**、**验收对账**、**回注带失败明细原文**（D025 改写 D018/D019 的「不做定位」口径——两个路由目标同口径，交付面与日志面同源）。整个模块是纯函数 + 一次文件读取，无内部状态、无并发、无缓存。

## 职责

**负责**：

- 回归证据判定：`REGRESSION_LOG` 定位、`check_regression_claim`（日志退出码 vs agent 自述）、`check_regression_log`（读黑板上那份日志）。
- 验收对账：`parse_acceptance_table`（解析「- 标准 → 测试:行」表）、`reconcile_acceptance`（逐条覆盖判定 + 空引用判定）。
- 回注文本生产：`route_back_message`（给 coding / plan 的回注消息）、`failure_details`（明细渲染，一行一条）、`FAILURE_BLOCK_TITLE` 与 `coding_failure_block`（coding 送达块）。

**不负责**：

- 不决定「何时回注 / 回注给谁」——分派在 `src/change/orchestrator.rs`（[orchestrator.rs.md](orchestrator.rs.md)），依据 `TestingTurn` 变体。
- 不跑测试、不判测试质量——测试阶段的实际会话在 `src/change/session.rs`，审查在 `src/sop/review_gate.rs`（[session.rs.md](session.rs.md)）。
- 不解析证据文件的通用形状——复用 `coding_gate` 的 `parse_baseline_log`（[coding_gate.rs.md](coding_gate.rs.md)），本模块只把它包成回归语境的错误文案。
- 不做验收覆盖率汇总——运行时汇总是 `summarize_acceptance`（[report.rs.md](report.rs.md)）；逐行引用可解析性由 `path_refs` 判（[plan_audit.rs.md](plan_audit.rs.md)）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `REGRESSION_LOG` | 常量 | [REGRESSION_LOG](#regression_log) |
| `AcceptanceMatch` | 结构体 | [AcceptanceMatch](#acceptancematch) |
| `check_regression_claim` | 函数 | [check_regression_claim](#check_regression_claim) |
| `check_regression_log` | 函数 | [check_regression_log](#check_regression_log) |
| `parse_acceptance_table` | 函数 | [parse_acceptance_table](#parse_acceptance_table) |
| `reconcile_acceptance` | 函数 | [reconcile_acceptance](#reconcile_acceptance) |
| `route_back_message` | 函数 | [route_back_message](#route_back_message) |
| `failure_details` | 函数 | [failure_details](#failure_details) |
| `FAILURE_BLOCK_TITLE` | 常量 | [FAILURE_BLOCK_TITLE](#failure_block_title) |
| `coding_failure_block` | 函数 | [coding_failure_block](#coding_failure_block) |

**转发面**：`src/change/mod.rs:43` 至 `:46` 转发 8 个名字（上表除 `REGRESSION_LOG` 与 `parse_acceptance_table` 外的全部）；这两个走模块路径消费（`src/change/session.rs:20` 至 `:22`）。模块声明 `src/change/mod.rs:22`。

## 依赖

**上游（谁用它）**：

- `src/change/session.rs:530` 声称通过后调 `check_regression_log(&self.ctx.blackboard, true)?`；`:532` 至 `:535` 读对账表、`:536` 解析、`:539` 至 `:546` 逐条用 `path_refs` 判引用可解析（🟢 CONFIRMED `src/change/session.rs:530` 至 `:546`）。
- `src/change/session.rs:136` 把 `REGRESSION_LOG` 写进 testing review 指令、`:194` 写进 testing 开工语；`:165` 用 `coding_failure_block` 拼 coding 送达块（🟢 CONFIRMED `src/change/session.rs:165`）。
- `src/change/orchestrator.rs:333` 调 `route_back_message("coding", &failures)`、`:360` 调 `route_back_message("plan", &failures)`（🟢 CONFIRMED `src/change/orchestrator.rs:333`）。
- 仓外黑盒：`tests/change_failure_context/main.rs:27` 引入 `FAILURE_BLOCK_TITLE` 与 `route_back_message`；`tests/change_failure_context/context.rs:39` 断言无失败上下文时开工语不含标题行、`:59` 断言有上下文时等于「kickoff + 空行 + 送达块」。

**下游（它依赖谁）**：

- `src/change/coding_gate.rs:104` 的 `parse_baseline_log`（内部 `parse_evidence_log` 在 `src/change/coding_gate.rs:37` 至 `:99`；本模块 `:13` 引入）。`src/change/coding_gate_tests.rs` 至 `:563` 的用例钉住「回归路径文案以『回归日志不可解析』开头」。
- `src/change/state.rs:44` 的 `TestFailureContext`（本模块 `:14` 引入，只用 `issues` 字段）。
- `src/error.rs` 的错误类型 DevforgeError（其 Quality 变体）与 `Result`（本模块 `:15` 引入）；测试里用 `is_quality_failure`（`src/error.rs:74`）。
- 标准库的 Path（`:11`）。生产零外部 crate；测试用 `tempfile`。

## 关键口径

- 🟢 **回归优先，只判「矛盾」不判「红」**：日志退出码非 0 且 agent 自称回归通过 → `Quality`；日志红但 agent 如实说红 → `Ok(非零)`，回注与否由编排层决定（`src/change/testing_gate.rs:34` 至 `:40`，用例 `src/change/testing_gate.rs:164`）。
- 🟢 **缺一条即零覆盖**：`reconcile_acceptance` 缺任一条即 `Quality`；计划没有验收标准本身也判 `Quality`（`src/change/testing_gate.rs:78` 至 `:98`）。
- 🟢 **明细原文不下修**：`route_back_message` 与 `coding_failure_block` 共用 `failure_details`（`src/change/testing_gate.rs:121`、`src/change/testing_gate.rs:146`），因此同一失败在四个落点同源——回注消息、日志（`src/change/orchestrator.rs:334` 的 warn）、错误文本（`src/change/orchestrator.rs:355` 至 `:357`、`:363` 至 `:365`）、下一轮 coding 送达块（源码注释 `src/change/testing_gate.rs:127` 只说「同一失败在三个出口同源（不漂移）」，未逐一列名）。不剥「文件:行」、不改写、不截断。
- 🟢 **`target` 只改措辞**：`coding` 与 `plan` 两个目标的明细逐字同源，函数内不做白名单校验（`src/change/testing_gate.rs:118` 至 `:123`；用例 `src/change/testing_gate.rs:240`）。
- 🟡 **对账表按最后一个箭头切**：`rsplit_once('→')` 让「标准原文本身含 →」不再把右侧切坏（依据：`src/change/testing_gate.rs:61` 的写法 + `:56` 的注释「按**最后一个** `→` 切」）。
- 🟡 **`reconcile_acceptance` 是宽松匹配，且运行时没用它**：命中判定含双向 `contains`（`src/change/testing_gate.rs:85` 至 `:87`），比运行时 `summarize_acceptance` 的精确相等（`src/change/report.rs:128`）宽；本文件外只出现在 `src/change/mod.rs:43` 转发与单测里。宽严差异与「空引用分支无用例」见 [_meta/gaps.md](../../_meta/gaps.md)。
- 🟡 **解析错误文案会串到基线口径**：`parse_baseline_log` 内部固定用基线文件名与「基线」标签（`src/change/coding_gate.rs:104` 至 `:106`），所以回归日志格式坏时，外层虽是「回归日志不可解析」，内层仍是「基线证据缺退出码（evidence/baseline-test.log …）」。见 [_meta/gaps.md](../../_meta/gaps.md)。

---

### REGRESSION_LOG

- **名称**：`REGRESSION_LOG`。
- **类型**：`pub const &'static str`。
- **默认值**：evidence/regression.log（相对黑板的路径，不是仓内绝对路径）。（🟢 CONFIRMED `src/change/testing_gate.rs:18`）
- **是否必填**：编译期常量，无「不填」；不可运行时覆盖。
- **取值范围**：字面量固定。
- **影响**：`check_regression_log` 用它拼读取路径（`:46`）；`session` 把它原样写进 testing review 指令与 testing 开工语（`src/change/session.rs:136`、`:194`）——改常量会同步改「让 agent 落证据」的提示词位置。
- **示例**：黑板下该相对路径的内容为 `command: cargo test` + `exit_code: 0` → `check_regression_log(blackboard, true)` 返回 `Ok(0)`。

### AcceptanceMatch

- **职责**：对账表的一行——一条验收标准到一处测试引用的映射。不校验引用是否可解析（那是 `path_refs` 的事），也不判覆盖率。
- **构造或初始化**：正常由 `parse_acceptance_table` 产出；两字段 `pub`，可直接构造（`reconcile_acceptance` 的空引用分支只能靠直构触发；`TestFailureContext` 在用例 `src/change/testing_gate.rs:262` 至 `:268` 同样是直构的值对象）。
- **公开成员**：`criterion: String`（验收标准原文，`:23`）、`test_ref: String`（测试引用，形如 tests/test_a.py:12，`:24`）。两者都拥有所有权，构造时不校验内容（空判定由 `reconcile_acceptance` 与 `path_refs` 各自做）。
- **生命周期**：值对象，派生 `Debug, Clone, PartialEq, Eq`（`:21`）；只在对账内存中存在，不落盘、不序列化。
- **典型用法**：读表 → 解析 → 对账（见 [parse_acceptance_table](#parse_acceptance_table) 与 [reconcile_acceptance](#reconcile_acceptance) 的示例）。

### check_regression_claim

- **摘要**：判定回归日志退出码与 agent 自述是否自相矛盾，并返回日志退出码。
- **行为语义**：先 `parse_baseline_log` 解析（取第一处退出码行；候选前缀与容差见 `src/change/coding_gate.rs:37` 至 `:99`），解析失败包成 `Quality("回归日志不可解析（要写 \`exit_code: N\`）：{e}")`；`exit_code != 0 && claims_green` → `Quality("回归日志退出码 {n} 与「回归通过」的自述矛盾（命令：{cmd}）")`，`cmd` 为空写「未记录」；其余一律 `Ok(exit_code)`。（🟢 CONFIRMED `src/change/testing_gate.rs:28` 至 `:42`）
- **参数**：`log_text: &str`（日志全文，不是路径）、`claims_green: bool`（agent 是否声称回归通过）。
- **返回**：`Result<i32>` — 日志里的退出码（可为负）。
- **异常**：`Quality` —— (a) 日志缺可解析的退出码行；(b) 红日志 + 称绿。两者都能被 `is_quality_failure()` 识别（`src/error.rs:74`）。
- **示例**：红 + 称绿 → `Err` 且文案带自己跑的命令（用例 `src/change/testing_gate.rs:155`）；红 + 如实 → `Ok(2)`（`:164`）；绿 → `Ok(0)`（`:171`）。

### check_regression_log

- **摘要**：读黑板上的回归日志文件，再交给 `check_regression_claim` 判。
- **行为语义**：路径 = 黑板根 join 常量 `REGRESSION_LOG`；`read_to_string` 失败（不存在 / 权限 / 非 UTF-8）→ `Quality("缺少回归证据 {path}（测试阶段必须先跑既有全套）：{e}")`，文案带拼接后的完整路径；成功转 `check_regression_claim`。无写盘、无缓存、无重试。（🟢 CONFIRMED `src/change/testing_gate.rs:45` 至 `:54`）
- **参数**：`blackboard: &Path`（黑板根目录，不是日志文件路径）、`claims_green: bool`（同前）。
- **返回**：`Result<i32>` — 日志退出码。
- **异常**：`Quality` —— 缺文件，或下游两种判定失败。
- **示例**：空临时目录 → `Err` 文案含「缺少回归证据」（用例 `src/change/testing_gate.rs:177`）；运行时调用点在 `src/change/session.rs:530`（声称通过后必过此门）。

### parse_acceptance_table

- **摘要**：把验收对账表文本解析成 `AcceptanceMatch` 列表。
- **行为语义**（🟢 CONFIRMED `src/change/testing_gate.rs:57` 至 `:74`）：
  - 逐行 `trim` → 去掉前导 `-` / `*`（可连续多个）→ 再 `trim`（`:60`）；
  - 用 `rsplit_once('→')` 按**最后一个**箭头切；该行无箭头直接跳过，不影响其它行（`:61` 至 `:63`）；
  - `criterion` = 左半 `trim`；`test_ref` = 右半 `trim` 后再剥掉包裹的反引号（`:64`、`:65`）；
  - 左或右任一为空即丢弃（因此**本函数不会产出空 `test_ref` 的条目**，`:66` 至 `:71`）；
  - 纯解析：不校验路径、不去重、保持输入行序。
- **参数**：`text: &str` — 对账表全文（通常来自黑板的 acceptance_path）。
- **返回**：`Vec<AcceptanceMatch>`，按输入行序；坏行静默丢弃。
- **异常**：`N/A`（返回裸 `Vec`，不产生 `Result`）。
- **示例**：两行表 → 2 条，第二条 `test_ref` 的反引号已被剥掉（用例 `src/change/testing_gate.rs:184`）；运行时调用点 `src/change/session.rs:536`。

### reconcile_acceptance

- **摘要**：逐条对账——计划里每条验收标准都要在表里出现，且表里不能有空引用。
- **行为语义**（🟢 CONFIRMED `src/change/testing_gate.rs:77` 至 `:112`）：
  - `criteria` 为空 → `Quality("计划里没有验收标准，无从对账（计划阶段就该写出来）")`（空标准不是通过，是失败，`:78` 至 `:82`）；
  - 命中判定是**双向包含**：`m.criterion == c || m.criterion.contains(c) || c.contains(m.criterion)`（`:85` 至 `:87`）——标准在表里被加长或截短都算命中；
  - 缺任一条 → `Quality("验收对账缺 {n} 条（缺一条即零覆盖）：{缺项以；连接}")`（`:92` 至 `:98`）；
  - 再查空引用：`test_ref` 为空的条目 → `Quality("验收项没有指到测试 path:line：{criterion 以；连接}")`（`:99` 至 `:110`；该分支只能靠手工构造 `AcceptanceMatch` 触发）；
  - 全过 → `Ok(criteria.len())`（`:111`）。幂等、无副作用、无 IO。
- **参数**：`criteria: &[String]`（计划里的验收标准原文）、`table: &[AcceptanceMatch]`（通常是 `parse_acceptance_table` 的输出）。
- **返回**：`Result<usize>` — 对上的条数，恒等于 `criteria` 长度。
- **异常**：`Quality` —— 三处（空标准 / 缺项 / 空引用）。
- **示例**：两条标准只对一条 → `Err` 含「缺 1 条」与缺项原文（用例 `src/change/testing_gate.rs:194`）；全覆盖 → `Ok(1)`（`:204`）；`&[]` → `Err` 含「没有验收标准」（`:211`）。
- **⚠ 使用前先读**：仓内**无生产调用点**（本文件外只出现在 `src/change/mod.rs:43` 的转发与单测）。运行时对账实际分两步：`session` 用 `parse_acceptance_table` 读表、逐条 `path_refs` 判引用可解析（`src/change/session.rs:536` 至 `:546`），覆盖率由 `summarize_acceptance` 精确相等汇总（`src/change/report.rs:128`，调用点 `src/change/orchestrator.rs:309`、`:372`）。

### route_back_message

- **摘要**：生成「测试未通过、回去处理」的回注消息，失败明细逐条原文内嵌。
- **行为语义**（🟢 CONFIRMED `src/change/testing_gate.rs:118` 至 `:123`）：输出为一行 `测试阶段未通过，按 <route>{target}</route> 回去处理。`、一行 `失败明细：`、再一行明细（`failure_details` 的渲染，可能多行），末尾多一个换行；`failures` 为空时明细段为空。`target` 只出现在标签里，不进任何判定。
- **参数**：`target: &str`（运行时取 `coding` 或 `plan`）、`failures: &[String]`（test agent 给的失败明细原文，含「文件:行」）。
- **返回**：`String`（回注消息全文）。
- **异常**：`N/A`（纯字符串组装，不返回 `Result`）。
- **示例**：`route_back_message("coding", &[…])` 的消息里 `path_refs` 仍能解析出「tests/test_a.py」与行 12（用例 `src/change/testing_gate.rs:217`）；plan 目标同口径（`:240`）。生产调用点 `src/change/orchestrator.rs:333`（coding）/ `:360`（plan）。

### failure_details

- **摘要**：把失败明细渲染成「一行一条、前缀 `- 」的文本。
- **行为语义**（🟢 CONFIRMED `src/change/testing_gate.rs:128` 至 `:134`）：每项输出 `- {原文}`，项间 `\n` 连接，**末尾无换行**；空切片 → 空串。不转义、不截断、不改写（回注消息与 coding 送达块共用 → 出口同源）。
- **参数**：`failures: &[String]`。
- **返回**：`String`。
- **异常**：`N/A`（纯渲染）。
- **示例**：两条明细 → 恰为 `- a.rs:1 失败\n- b.rs:2 失败`（用例 `src/change/testing_gate.rs:250`）。

### FAILURE_BLOCK_TITLE

- **名称**：`FAILURE_BLOCK_TITLE`。
- **类型**：`pub const &'static str`。
- **默认值**：`上一轮测试失败明细如下（逐条原文）`。（🟢 CONFIRMED `src/change/testing_gate.rs:137`）
- **是否必填**：编译期常量。
- **取值范围**：字面量固定。
- **影响**：它是 `coding_failure_block` 的首行（`:145`），同时是**反向断言锚点**——无失败上下文时 coding 开工语不得出现这句话（`src/change/session/session_tests/plan.rs:232`、`tests/change_failure_context/context.rs:39`）。改字面量会连带改这两处期望。
- **示例**：有失败上下文时送达块首行即它（用例 `src/change/testing_gate.rs:260`）。

### coding_failure_block

- **摘要**：coding agent **输入面**上的失败送达块——标题行 + 明细逐条原文。
- **行为语义**（🟢 CONFIRMED `src/change/testing_gate.rs:142` 至 `:148`）：输出 `{FAILURE_BLOCK_TITLE}：\n{failure_details(&failure.issues)}`；只读 `issues`，**不读 `round`**（不回显轮次）；不追加寒暄或定位提示；无 IO、无副作用。
- **参数**：`failure: &TestFailureContext`（`src/change/state.rs:44`：`round: usize` + `issues: Vec<String>`）。
- **返回**：`String`。
- **异常**：`N/A`（纯组装）。
- **示例**：两条 issues → 块含标题与两条原文、不含「请自己定位原因」「本消息不提供文件位置」「<位置略>」（用例 `src/change/testing_gate.rs:260`）；`coding_kickoff(ctx, Some(f))` 恰等于无上下文正文 + 空行 + 本块（`src/change/session/session_tests/plan.rs:286`、`tests/change_failure_context/context.rs:57` 至 `:61`）。

---

## 测试覆盖（`#[cfg(test)] mod tests`，`:150`，12 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `red_regression_contradicting_the_claim_fails` | 红 + 称绿 → `Quality`，文案带命令 | `src/change/testing_gate.rs:155` |
| `red_regression_with_honest_claim_is_fine` | 红 + 如实 → `Ok(2)` | `src/change/testing_gate.rs:164` |
| `green_regression_passes` | 绿 → `Ok(0)` | `src/change/testing_gate.rs:171` |
| `missing_regression_log_is_quality_failure` | 缺日志 → `Quality` 含「缺少回归证据」 | `src/change/testing_gate.rs:177` |
| `acceptance_table_parses_arrows` | 按最后一个箭头切、剥反引号 | `src/change/testing_gate.rs:184` |
| `one_missing_criterion_is_zero_coverage` | 缺一条即零覆盖 | `src/change/testing_gate.rs:194` |
| `full_reconciliation_passes` | 全覆盖 → `Ok(n)` | `src/change/testing_gate.rs:204` |
| `empty_criteria_is_rejected` | 空标准 → `Quality` | `src/change/testing_gate.rs:211` |
| `route_back_message_carries_the_failure_details_verbatim` | 回注逐字带定位、无零信息话术、`path_refs` 仍可解析 | `src/change/testing_gate.rs:217` |
| `route_to_plan_carries_the_same_details` | plan 目标同口径 | `src/change/testing_gate.rs:240` |
| `failure_details_render_one_issue_per_line_verbatim` | 一行一条、逐字 | `src/change/testing_gate.rs:250` |
| `coding_failure_block_carries_the_title_and_verbatim_issues` | 标题 + 全明细、无零信息话术 | `src/change/testing_gate.rs:260` |

**未覆盖**：`reconcile_acceptance` 的空引用分支（`src/change/testing_gate.rs:99` 至 `:110`）无用例——`parse_acceptance_table` 先丢弃空 `test_ref` 行，只有手工构造 `AcceptanceMatch` 才走得到。

## 运行与环境

- **环境要求**：Rust 工具链（`edition = "2024"`，需 1.85+；本机 rustup 无默认工具链，需显式 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`，否则 `cargo` 报「could not choose a version of cargo to run」）+ `tempfile`；平台无特殊要求，无网络、无端口、无外部服务。`tempfile` 按 `TMPDIR` 建临时目录——若覆盖 `TMPDIR`，该目录必须已存在，否则 `missing_regression_log_is_quality_failure`（`src/change/testing_gate.rs:177`）会 `PathError { kind: NotFound }`。
- **依赖安装**：`cargo fetch`（首次需网络；离线加 `CARGO_NET_OFFLINE=true`）。生产代码零外部 crate，测试侧只有 `tempfile`（dev-dependency）。
- **配置项**：`N/A` —— 无运行时可配键；两个公开常量是编译期字面量，不可覆盖。
- **启停与健康检查**：`N/A` —— 库内同步模块，无进程 / 端口 / 健康端点；它的「健康」体现在门禁返回值（`Quality` vs `Ok`）。
- **种子数据**：`N/A` —— 用例自带固定文本与 `tempfile` 临时目录，无外部种子；运行时需要的两条黑板数据由测试阶段自己产出（回归日志与验收对账表）。
- **测试运行**：`cargo test --lib change::testing_gate` 即全部 12 例。本轮实测（`RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=/var/tmp/u23-target TMPDIR=/var/tmp/u23-tmp CARGO_NET_OFFLINE=true`，快照 2026-10-24，源码 `src/change/testing_gate.rs` 10-21 版）：

  ```
  cargo test --lib change::testing_gate
  → 12 passed; 0 failed; 0 ignored; 601 filtered out; EXIT=0
  ```

  `filtered out` 是全库用例数减本过滤器，随仓内其他单元增删测试而变；`12` 这个 passed 数由本文件用例数决定。
- **公开接口清单**：`N/A` —— 无 HTTP / CLI 端点；可用的 Rust API 即上文「公开成员」10 项（其中 8 项经 `src/change/mod.rs:43` 至 `:46` 转发）。

## 证据与缺口

- 本轮实跑（cargo，非预构建二进制）：`cargo test --lib change::testing_gate` → 12 passed / 0 failed / 601 filtered out / EXIT=0（环境前缀见 [运行与环境](#运行与环境)）。
- 全仓调用点已用 `grep --include=*.rs` 复核，除上文列出的 `session` / `orchestrator` / 两个测试目标外无其它消费者。
- 开放问题（对账宽严不一致、空引用分支未覆盖、解析错误文案串到基线口径）记在 [_meta/gaps.md](../../_meta/gaps.md)，本文件不重复叙述。
