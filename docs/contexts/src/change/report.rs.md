---
type: module
module_path: src/change/report.rs
unit: unit-src-16
description: 变更报告（引擎写）——diff 统计、审查轮次、往返、用量、验收对账渲染与落盘
---

# src/change/report.rs

变更报告（P009 T012）：**引擎写，不派 agent**。只放可复算的东西；对账没过时报告里必须一眼看得出来。

## 职责

**负责**：
- diff 统计：`diff_stats` 跑 `git diff --numstat` 汇总文件数/增删行数。
- 验收对账汇总：`summarize_acceptance` 按验收标准与已完成项算 `total`/`done`/`missing`，`AcceptanceSummary` 的 `passed` 给出结论。
- 报告数据结构与渲染：`ChangeReport` + `render` 产 markdown（含未对账清单小节）。
- 报告落盘：`write_report` 写 `<repo>/docs/plantree/changes/<change-id>/report.md`。

**不负责**：不采集用量（由 `src/server/change_engine.rs:54` 的 `finalize` 从会话账汇总后回填）、不计墙钟（`finalize` 填 `wall_secs`）、不判阶段推进（见 `src/change/orchestrator.rs`）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `DiffStats` | 结构体 | [DiffStats](#diffstats) |
| `AcceptanceSummary` | 结构体 | [AcceptanceSummary](#acceptancesummary)（含 `passed`） |
| `ChangeReport` | 结构体 | [ChangeReport](#changereport)（含 `render`） |
| `diff_stats` | 函数 | [diff_stats](#diff_stats) |
| `summarize_acceptance` | 函数 | [summarize_acceptance](#summarize_acceptance) |
| `write_report` | 函数 | [write_report](#write_report) |

## 依赖

- 上游 / 消费方：
  - `src/change/mod.rs:19` `pub mod report;`；`:38`-40 `pub use report::{AcceptanceSummary, ChangeReport, DiffStats, diff_stats, summarize_acceptance, write_report};`。
  - `src/change/orchestrator.rs:14` 引入 `report` / `AcceptanceSummary` / `ChangeReport` / `summarize_acceptance`；`:151` `ChangeOutcome` 的 `report` 字段；`:309` 用 `summarize_acceptance` 判验收缺项；`:371` 调 `diff_stats`（`unwrap_or_default()`）；`:372` 汇总验收；`:373` 组装 `ChangeReport`。
  - `src/server/change_engine.rs:11` 引入 `write_report`；`:56` 在 `finalize` 中写报告（`:54` 用量回填、`:55` 墙钟回填）。
- 向下依赖：`crate::error::{DevforgeError, Result}`（`src/change/report.rs:8`）、标准库 `Path` 与子进程 `Command`（git）。
- 外部 crate：生产无；测试用 `tempfile`。

## 核心口径

- 报告字段全部来自引擎采集/复算，agent 自述不进报告（用量取自会话账目录汇总，`src/server/change_engine.rs:54`）。
- 验收结论三态：`通过` / `未通过：done / total 条对账通过，缺 N 条` / `无验收标准（异常）`（`total == 0`）。
- `passed()` 要求 `missing` 为空**且** `total > 0`——空验收标准不算通过（报告里标「异常」）。
- 验收匹配是**字符串精确相等**（`done` 里存在与 criteria 完全相同的串），不做前缀/包含匹配。

---

### DiffStats

- 职责：相对基线的 diff 统计（文件数 + 增删行）。
- 构造 / 初始化：由 `diff_stats` 返回；`Default` 可直构；字段 `pub`。
- 公开成员：`files: usize`（改动文件数）、`insertions: usize`（新增行）、`deletions: usize`（删除行）。
- 生命周期：值对象；由 `ChangeReport` 的 `diff` 字段持有。
- 典型用法：`let stats = diff_stats(repo, baseline).unwrap_or_default();`（`src/change/orchestrator.rs:371`）。
- 派生 trait：`Debug, Clone, Default, PartialEq, Eq`。（🟢 CONFIRMED `src/change/report.rs:13`）

### AcceptanceSummary

- 职责：验收对账结果（总数 / 已通过 / 缺失条目原文）。
- 构造 / 初始化：由 `summarize_acceptance` 返回；字段 `pub` 可直构。
- 公开成员：
  - `total: usize` — 验收标准条数。
  - `done: usize` — 已通过条数（`total - missing.len()`）。
  - `missing: Vec<String>` — 未对账的条目原文（顺序同 criteria）。
  - `passed(&self) -> bool` — `missing` 为空且 `total > 0`；无参数（作用于 `&self`）。
- 生命周期：值对象；`ChangeReport` 的 `acceptance` 字段持有，`render` 与编排层共用。
- 典型用法：`if !summary.passed() { … }`（`src/change/orchestrator.rs:309`）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/report.rs:21`）

### ChangeReport

- 职责：一次变更运行的可复算报告内容。
- 构造 / 初始化：由编排层组装（`src/change/orchestrator.rs:373`），`finalize` 回填 `usage`/`wall_secs`。
- 公开成员：
  - `change_id: String`、`run_id: String`、`baseline_sha: String` — 标识与基线锚点。
  - `wall_secs: u64` — 墙钟秒数（调用方计时）。
  - `diff: DiffStats` — 相对基线 diff 统计。
  - `review_rounds: usize` — 审查轮次（plan/coding/testing 各自累加）。
  - `roundtrips: usize` — plan↔coding 往返次数。
  - `usage: String` — 用量行（空串写在报告里为「未记录」，由编排层在组装时替换）。
  - `acceptance: AcceptanceSummary` — 验收对账。
  - `render(&self) -> String` — markdown：标题 `# 变更报告 · <change_id>`；列表行 run / 基线 / 墙钟 / diff / 审查轮次 / plan↔coding 往返 / 用量 / 验收对账；`missing` 非空时追加 `## 未对账的验收标准` 与逐条 `- <条目>`。无参数。
- 生命周期：值对象；由 `ChangeOutcome` 携带，`finalize` 后写盘。
- 典型用法：`write_report(workspace, &report.change_id, &report)`（`src/server/change_engine.rs:56`）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。（🟢 CONFIRMED `src/change/report.rs:35`）

### diff_stats

- 摘要：跑 `git diff --numstat <baseline>` 汇总改动统计（只看工作区相对基线的差异）。
- 行为语义（🟢 CONFIRMED `src/change/report.rs:95`）：启动 git 失败 → `System("执行 git diff 失败：…")`；命令非零退出 → `System("git diff --numstat <baseline> 失败：…")`；成功则逐行按制表符切 `add`/`del`/`path`，`path` 为空跳过，否则 `files += 1`，`insertions`/`deletions` 累加可解析的数字（二进制文件的 `-` 解析失败记 0）。无副作用（只读 git）。
- 参数：`repo: &Path` — git 仓库根；`baseline: &str` — 基线 commit 引用。
- 返回：`Result<DiffStats>`。
- 异常：`System`（git 不可执行 / `git diff` 非零退出）。
- 示例：改一个文件 + 新增一个已入索引的文件 → `files=2`、`insertions=3`、`deletions=0`（`src/change/report_tests.rs`）。

### summarize_acceptance

- 摘要：按验收标准与已完成项汇总对账结果。
- 行为语义（🟢 CONFIRMED `src/change/report.rs:125`）：`missing` = criteria 中**不在** done 里的条目（字符串精确相等），保持 criteria 顺序；`total = criteria.len()`；`done = total - missing.len()`；返回 `AcceptanceSummary`。幂等、无副作用。
- 参数：`criteria: &[String]` — 计划验收标准；`done: &[String]` — 已对账通过项。
- 返回：`AcceptanceSummary`。
- 异常：N/A（纯计算）。
- 示例：criteria `["a","b"]`、done `["a"]` → `total=2`、`done=1`、`missing=["b"]`、`passed()` 为假（`src/change/report_tests.rs`）。

### write_report

- 摘要：把报告写到目标仓（目录不存在先建）。
- 行为语义（🟢 CONFIRMED `src/change/report.rs:139`）：`dir = <repo>/docs/plantree/changes/<change_id>`；建目录失败 → `System("建目录 … 失败")`；写 report.md（内容为 `report` 的 `render()` 输出）失败 → `System("写报告 … 失败")`；成功返回报告路径。覆盖写（同路径重复调用直接重写）。
- 参数：`repo: &Path` — 目标仓；`change_id: &str` — 变更 id（目录名）；`report: &ChangeReport`。
- 返回：`Result<PathBuf>` — 写出的 report.md 绝对/相对路径。
- 异常：`System`（建目录或写文件失败）。
- 示例：`write_report(tmp, "change-20261020-x", &report)` → 文件存在且内容含「变更报告」（`src/change/report_tests.rs`）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `render_carries_every_required_field` | 报告字段齐全 | `src/change/report.rs:180` |
| `failed_reconciliation_is_visible_in_report` | 未通过可见 + 缺失清单小节 | `src/change/report_tests.rs` |
| `empty_criteria_is_reported_as_anomaly` | 空验收标准标「异常」 | `src/change/report_tests.rs` |
| `summarize_counts_missing` | total/done/missing 计数 | `src/change/report_tests.rs` |
| `diff_stats_reads_numstat` | numstat 解析（改+新增、二进制跳过） | `src/change/report_tests.rs` |
| `write_report_creates_directory_and_file` | 建目录 + 写 report.md | `src/change/report_tests.rs` |

## 测试运行与环境

- 测试与被测代码同文件：`#[cfg(test)] mod tests`（`src/change/report.rs:149` 起），`#[test]` 共 6 例。
- 运行单模块单测：`cargo test --lib change::report`（🟡 INFERRED：Rust 单测默认入口；本次勘察环境 rustup 未配置默认 toolchain，未实跑复现）。
- 运行依赖：`tempfile`（dev-dependency，`Cargo.toml:27`）；`diff_stats_reads_numstat` 会起 `git` 子进程（init/add/commit/rev-parse + 新增文件 `git add`），本机须有可用 `git`；无外部服务/端口/网络。
