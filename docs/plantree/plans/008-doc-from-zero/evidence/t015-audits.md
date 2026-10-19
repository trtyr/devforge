# T015–T018 · 审计夹具 + 四项确定性审计 证据

- 日期：2026-10-19
- 新增：`src/docgen/audit.rs`（覆盖对账 + Runbook 命令抽取 + 报告渲染）；`DocSopLoop::with_audit`；引擎在每轮审查前组装审计文本

## 一、五项审计的落点

| 审计 | 实现 | 判定 |
|---|---|---|
| **覆盖** | `audit::coverage_for_unit`（单元级）/ `coverage`（全局）——按 `doc_level` 取条目集合，逐条查文档是否提及 | 缺一条即 FAIL（D013） |
| **完整性** | `docgen::lint` 必填节（D014 分级 + 空节 + N/A 规则） | 缺/空 → FAIL |
| **真实性** | `docgen::lint` 实体存在性（幽灵实体/路径/行号越界） | 任一不存在 → FAIL |
| **置信度诚实性** | `docgen::lint` 🟢 必须同行给 `path:line` 或可重放命令；🔴 必须有 gaps.md | 标错绿 → FAIL（最严重） |
| **可跑** | 引擎抽取 Runbook 里 bash/sh/console 命令块注入审查输入，审查官**逐条从零 boot** | 跑不起来 → FAIL |

## 二、覆盖按档位取集合（D007/D013）

| 档位 | 要求条目 |
|---|---|
| `essential` | 仅 `entry` |
| `complete` / `detailed` | `entry` + `module` + `symbol` |

单测：`essential_level_only_requires_entry_points`（essential 下只写入口即通过）、`full_coverage_requires_modules_and_symbols`（complete 下缺符号即失败）。

## 三、审计报告由引擎生成并注入（D012）

`DocSopLoop::with_audit(provider)`：每轮审查前调用 provider(unit)，把「覆盖审计 + lint 摘要 + Runbook 命令块」拼进审查指令，并冠以「**引擎审计（确定性，勿质疑）**」。

单测：`audit_text_is_injected_into_review_instruction` 断言审查指令里出现审计文本与标题。

审查指令同时写明：审计是确定性事实，直接采信逐条验收，**不要重算**（省轮次）。

## 四、Runbook 命令抽取

`audit::runbook_commands`：从 `*runbook*.md` 的 bash/sh/console/shell/zsh 代码块抽命令（跳过注释与空行，忽略 text 块）。
单测：`runbook_commands_extracted_from_bash_blocks`（1 块命中、text 块不误收）。

> 诚实边界：引擎只负责**把命令块准确抽出来并放进审查输入**；真正执行 boot 的是审查官（它有工作区与命令权限）。"跑不起来 = FAIL" 由审查判定，属人工/模型判定而非机械判定——这是本任务与其余四项的区别，已写进审查指令。

## 五、门禁

`cargo test` **255 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
