---
type: module
module_path: src/change/coding_gate.rs
unit: unit-src-04
description: 变更 coding 阶段的两道机械质量门（回归前置 + 实施步骤按步提交）
---

# src/change/coding_gate.rs

变更 coding 阶段的两道机械门（P009 T010 / T010b）。

## 职责

本模块负责变更流程中 coding 阶段的两道机械质量门：**回归前置**与**实施步骤按步提交**。

**负责**：
- 回归前置：校验基线证据文件存在且退出码为 0（`check_baseline_log`），不绿不许动代码。
- 自证前置（「先证后报」）：校验自证证据文件存在且退出码为 0（`check_self_check`）。
- 证据日志解析：`parse_baseline_log` / 内部 `parse_evidence_log` 解析命令行与退出码（多写法、列表项/粗体容差）。
- 实施步骤对账：从实施步骤文件解析步骤（`parse_steps` / `read_steps`），并要求基线后每个提交声明 `change` 前缀加步骤号，步骤号与步骤序列一一对应（`check_step_commits`）。
- git 辅助查询：`changed_files`（`git diff --name-only <baseline>`）、`commits_since`（`git log` 取基线后 subject）。
- 常量/路径：`BASELINE_LOG`、`SELF_CHECK_LOG`、`IMPL_STEPS`、`COMMIT_PREFIX`、`impl_steps_path`。
- 数据结构：`BaselineEvidence`（结论）、`Step`（实施步骤）。

**不负责**：不自己运行测试/编译（那是 agent 判断）；不落盘计划、历史、检查点；不做边界判定（见 `src/change/boundary.rs`）；不生成报告（见 `src/change/report.rs`）。两道门失败即 `Quality`（`DevforgeError` 变体），不进自动重启（由编排层遵守）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `BASELINE_LOG` | 常量 | [BASELINE_LOG](#baseline_log) |
| `SELF_CHECK_LOG` | 常量 | [SELF_CHECK_LOG](#self_check_log) |
| `IMPL_STEPS` | 常量 | [IMPL_STEPS](#impl_steps) |
| `COMMIT_PREFIX` | 常量 | [COMMIT_PREFIX](#commit_prefix) |
| `BaselineEvidence` | 结构体 | [BaselineEvidence](#baselineevidence) |
| `Step` | 结构体 | [Step](#step) |
| `parse_baseline_log` | 函数 | [parse_baseline_log](#parse_baseline_log) |
| `check_baseline_log` | 函数 | [check_baseline_log](#check_baseline_log) |
| `check_self_check` | 函数 | [check_self_check](#check_self_check) |
| `parse_steps` | 函数 | [parse_steps](#parse_steps) |
| `read_steps` | 函数 | [read_steps](#read_steps) |
| `changed_files` | 函数 | [changed_files](#changed_files) |
| `commits_since` | 函数 | [commits_since](#commits_since) |
| `step_numbers_in` | 函数 | [step_numbers_in](#step_numbers_in) |
| `check_step_commits` | 函数 | [check_step_commits](#check_step_commits) |
| `impl_steps_path` | 函数 | [impl_steps_path](#impl_steps_path) |

## 依赖

- 上游 / 消费方：
  - `src/change/mod.rs:12` `pub mod coding_gate;`；`src/change/mod.rs:29` `pub use` 转发 `SELF_CHECK_LOG`、`check_baseline_log`、`check_self_check`、`check_step_commits`、`read_steps`。
  - `src/change/session.rs`：`:12-15` 的 `use` 块引入 `changed_files`、`check_baseline_log`、`check_self_check`、`check_step_commits`、`parse_baseline_log`、`read_steps`、`BASELINE_LOG`；`:98` `BASELINE_LOG`；`:120` 与 `:160` `SELF_CHECK_LOG`；`:206` 复用 `parse_baseline_log`（解析逻辑只有一份）；`:389` `check_baseline_log`；`:416` 与 `:474` `changed_files`；`:428` `check_self_check`；`:447` `read_steps`；`:449` `check_step_commits`；`:585` `BASELINE_LOG`（`baseline_log_path`）。
  - `src/change/testing_gate.rs:13` 引入 `parse_baseline_log`，`:29` 用它包裹回归日志；其「回归日志不可解析」文案与本门缺退出码文案同源（回归路径依赖本门文案，见 `src/change/coding_gate.rs:555`）。
- 向下依赖：`src/error.rs`（错误面 `DevforgeError` / `Result`，`src/change/coding_gate.rs:8`）；标准库（路径 `Path`/`PathBuf`、子进程 `Command`、git）。
- 外部 crate：生产无；测试用 `tempfile`（tempdir）。

## 核心口径

- **回归前置**：动手前必须有过一次「既有测试全绿」的证据；引擎不自己跑，只验证据。由 `check_baseline_log` 判定 `BASELINE_LOG` 指向的证据退出码为 0。
- **自证前置**：提交前留下命令原文 + 退出码 + 摘要（`SELF_CHECK_LOG`）；`check_self_check` 判定其为绿。
- **按步提交**：`IMPL_STEPS` 的步骤数必须与基线之后提交的步骤号一一对上；对不上 = 说一套做一套，FAIL。
- 两者均 **质量门**：失败即 `Quality`（`DevforgeError` 变体），不进自动重启。
- 证据解析允许写法（任一）：`exit_code: 0` / `exit code: 0` / `exit: 0` / `退出码: 0` / `退出码：0`；markdown 列表符号（`-`/`*`/`+`）与粗体（`**N**`）为格式容差，不参与语义；首中即停（文件最前的有效退出码为主，后续反向验证数字不覆盖）。

---

### BASELINE_LOG

- 类型：`pub const BASELINE_LOG: &str`。（🟢 CONFIRMED `src/change/coding_gate.rs:16`）
- 取值（字面值，相对黑板根）：
```text
evidence/baseline-test.log
```
- 含义：「基线证据记录」文件；子目录 `evidence` 是前缀，`baseline-test.log` 是其中的文件名，两者共同构成完整相对路径，不是裸文件名占位。
- 影响：`check_baseline_log` 与 `parse_baseline_log` 用它定位/命名证据文件；`src/change/session.rs:98` 与 `src/change/session.rs:585` 引用。
- 示例：黑板下基线证据完整相对路径就是上方取值的整串（子目录 `evidence` 之下的 `baseline-test.log`）；读取点 `src/change/session.rs:98` 与 `src/change/session.rs:585` 按此拼接定位。

### SELF_CHECK_LOG

- 类型：`pub const SELF_CHECK_LOG: &str`。（🟢 CONFIRMED `src/change/coding_gate.rs:18`）
- 取值（字面值，相对黑板根）：
```text
evidence/self-check.log
```
- 含义：「先证后报」的机械门证据文件，形状与基线门一致；子目录 `evidence` 是前缀，`self-check.log` 是其中的文件名。
- 影响：`check_self_check` 读取；`src/change/session.rs:120` 与 `src/change/session.rs:160` 引用。
- 示例：黑板下自证证据完整相对路径就是上方取值的整串（子目录 `evidence` 之下的 `self-check.log`）。

### IMPL_STEPS

- 类型：`pub const IMPL_STEPS: &str`。（🟢 CONFIRMED `src/change/coding_gate.rs:20`）
- 取值（字面值，相对黑板根）：
```text
impl-steps.md
```
- 含义：coding 阶段按步提交的步骤清单；文件名**含扩展名**（markdown 文档，字面值见上方取值整串），`read_steps` 严格按 `IMPL_STEPS` 此文件名读取。
- 影响：`read_steps`（读）与 `impl_steps_path`（拼路径）使用。
- 示例：黑板下实施步骤文件即上方取值的整串（含扩展名；`src/change/coding_gate.rs:191` 的 `read_steps` 按此读取，写文件时缺扩展名会命中错误文件名导致缺/空判 Quality）。

### COMMIT_PREFIX

- 类型：`pub const COMMIT_PREFIX: &str`。（🟢 CONFIRMED `src/change/coding_gate.rs:22`）
- 取值：文本 `"change:"`（ASCII `change` 后接冒号）。
- 含义：提交信息里要求出现的 change-id 标记（便于机械归属）。
- 影响：`check_step_commits` 用去掉尾部冒号的 `change` 判定提交是否可归属。
- 示例：合规提交信息 `change: change-20261020-x step 1`（以 `change` 加冒号开头，后接 `change-20261020-x` 与 `step 1`）。

### BaselineEvidence

- 职责：基线/自证证据的解析结论（退出码 + 命令）。只承载结论，不落盘。
- 构造：由 `parse_baseline_log` / 内部 `parse_evidence_log` / `check` 系列返回；字段 `pub` 可直接构造。
- 公开成员：
  - `exit_code: i32` — 证据日志中的退出码（0 = 绿）。
  - `command: String` — 证据日志中的命令行（`command` 前缀行），空串表示未记录。
- 生命周期：值对象，随函数返回/丢弃；不持有 IO。
- 典型用法：`check_baseline_log` 返回后，编排层按退出码判定能否动代码；错误文案里回填命令字段。
- 派生 trait：`Debug, Clone, PartialEq, Eq`（🟢 CONFIRMED `src/change/coding_gate.rs:26`）。

### Step

- 职责：实施步骤文件里解析出的单条实施步骤。
- 构造：由 `parse_steps` / `read_steps` 返回；字段 `pub` 可直接构造。
- 公开成员：
  - `number: u32` — 步骤号（来自 heading，如 `## 步骤 1`）。
  - `title: String` — 步骤标题（heading 数字后的正文）。
- 生命周期：值对象。
- 典型用法：`check_step_commits` 以 `&[Step]` 构造期望步骤号序列与提交声明对账。
- 派生 trait：`Debug, Clone, PartialEq, Eq`（🟢 CONFIRMED `src/change/coding_gate.rs:160`）。

### parse_baseline_log

- 摘要：解析基线证据日志文本为结论；期望里面有命令行与退出码。不读盘（输入为文本）。
- 行为语义：委托内部 `parse_evidence_log`，`file=BASELINE_LOG`、`label=基线`。支持 `exit_code: 0` / `exit code: 0` / `exit: 0` / `退出码: 0` / `退出码：0`（ASCII 大小写不敏感），`command` 或 `命令`（后接 ASCII 冒号或全角冒号）行记录命令；markdown 列表符号与粗体仅作格式容差。幂等、无副作用。（🟢 CONFIRMED `src/change/coding_gate.rs:104`，解析实现在 `src/change/coding_gate.rs:37`）
- 参数：`text: &str` — 证据日志全文。
- 返回：`Result<BaselineEvidence>`；无有效退出码 → `Err`。
- 异常：`Quality`（`DevforgeError` 变体）— 触发条件：日志里扫不到可解析的数字退出码（缺退出码）。文案形如「基线证据缺退出码（<file> 里要写 exit_code: N），无法判定基线是否绿」，其中 <file> 为 `BASELINE_LOG` 字面值，即错误文案指向基线文件本身。
- 示例：`parse_baseline_log` 保持单参数签名（`src/change/coding_gate.rs:501`），`testing_gate` 依此调用。

### check_baseline_log

- 摘要：读基线证据文件并判定基线是否绿。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:109`）：
  - 读黑板下 `BASELINE_LOG` 指向的文件；文件缺失 → `Quality`（文案 `缺少基线证据 …` 并带路径与 IO 错误）。
  - 经 `parse_baseline_log` 解析；缺退出码 → `Quality`（文案指向基线文件，`src/change/coding_gate.rs:426` 断言）。
  - 退出码 ≠ 0 → `Quality`（文案 `基线不是绿的（exit_code=N，命令：…）——不许在红基线上动代码`，命令为空回填「未记录」）。
  - 退出码 = 0 → 返回 `BaselineEvidence`。
- 参数：`blackboard: &Path` — 黑板根。
- 返回：`Result<BaselineEvidence>`。
- 异常：`Quality`（`DevforgeError` 变体）。缺退出码路径的载荷以「基线证据缺退出码」开头（回归路径依赖，`src/change/coding_gate.rs:426`）；缺文件路径以「缺少基线证据」开头。
- 示例：基线证据文件写 `command: cargo test` 与 `exit_code: 0` → `Ok`（`src/change/coding_gate.rs:379` 断言退出码 0 与命令）；写 `exit_code: 2`（命令 `pytest -q`）→ `Err` 且文案含命令（`src/change/coding_gate.rs:393`）。

### check_self_check

- 摘要：读自证证据文件并判定「先证后报」是否兑现（证据存在 + 退出码为 0）。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:135`）：与基线门同策略；读黑板下 `SELF_CHECK_LOG` 指向的文件；缺失 → `Quality`（文案 `缺少自证证据 …（提交前必须留下命令原文 + 退出码 + 摘要）`）；解析缺退出码 → `Quality`；退出码 ≠ 0 → `Quality`（文案 `自证不是绿的（…）——先证后报：证据红着不许提交`）；绿 → `Ok`。全部失败文案指向自证文件，不得指向基线文件。
- 参数：`blackboard: &Path`。
- 返回：`Result<BaselineEvidence>`。
- 异常：`Quality`（`DevforgeError` 变体）。
- 示例：自证文件写 `exit_code: 1` → `Err` 且文案含 `exit_code=1`（`src/change/coding_gate.rs:469`）；写 `exit_code: 0` 与 `455 passed` → `Ok`（`src/change/coding_gate.rs:487`）。缺文件时文案含 `SELF_CHECK_LOG` 且不含 `BASELINE_LOG`（`src/change/coding_gate.rs:438`）。

### parse_steps

- 摘要：从实施步骤 markdown 文本解析步骤序列。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:165`）：扫描以 `#` 开头的行；去掉所有前导 `#` 后，正文须以 `步骤` / `Step` / `step` 任一前缀开头（`strip_prefix`，非前缀即跳过该行），随后允许空白/冒号/顿号/句点分隔；紧跟的连续 ASCII 数字解析为 `number`，数字之后的正文为 `title`（标题缺省时为空串；数字后紧跟的分隔符被 `splitn` 丢弃，不进入 `title`）；数字缺失或不是 `u32` 的行跳过。幂等、无副作用。
- 参数：`md: &str` — 实施步骤 markdown。
- 返回：`Vec<Step>`（空向量表示没有步骤 heading）。
- 异常：N/A（不失败，靠 `read_steps` 判空）。
- 示例：`parse_steps` 从 `## 步骤 1` 与 `## 步骤 2` 与 `### Step 3` 解析出 `[1,2,3]`，对应 title 分别为「改接口」「补测试」「收尾」（`src/change/coding_gate.rs:599`）。

### read_steps

- 摘要：读黑板上的实施步骤文件并解析。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:191`）：读黑板下 `IMPL_STEPS` 指向的文件；文件缺失 → `Quality`（`缺少实施步骤 …（先写步骤再动代码）`）；`parse_steps` 结果为空 → `Quality`（文案 `…没有任何 ## 步骤 N 条目`）；否则返回步骤。
- 参数：`blackboard: &Path`。
- 返回：`Result<Vec<Step>>`。
- 异常：`Quality`（`DevforgeError` 变体）（缺失 / 空）。空内容（仅标题无步骤条目）→ `Err` 且 `is_quality_failure()`（`src/change/coding_gate.rs:660`）。
- 示例：`read_steps` 被 `src/change/session.rs:447` 在按步提交校验前调用。

### changed_files

- 摘要：相对基线的改动文件（含已提交与未提交、不含未跟踪）。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:210`）：执行 `git diff --name-only <baseline>`（`current_dir=repo`）；启动失败 → `System`（`DevforgeError` 变体，文案 `执行 git diff 失败`）；命令非零退出 → `System`（带 stderr）；成功则把 stdout 逐行 trim、去空行。
- 参数：`repo: &Path` — git 仓库根；`baseline: &str` — 基线 commit 引用。
- 返回：`Result<Vec<String>>` — 改动文件相对路径列表。
- 异常：`System` — git 不可执行 / git diff 非零退出。
- 示例：`src/change/session.rs:416`（审查指令里列出改动文件）与 `src/change/session.rs:474`（后续对账）消费它。

### commits_since

- 摘要：基线之后的提交信息（`git log` 取 subject）。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:231`）：执行 `git log --format=%s <baseline>..HEAD` 取 subject；启动失败 → `System`（文案 `执行 git log 失败`）；命令非零退出 → `Quality`（带 stderr）；成功 → 逐行 trim 去空行。
- 参数：`repo: &Path`；`baseline: &str`。
- 返回：`Result<Vec<String>>` — 提交 subject 列表。
- 异常：`System`（启动失败）/ `Quality`（git log 报错）。
- 示例：`check_step_commits` 用它取基线后 commits 与步骤号对账。

### step_numbers_in

- 摘要：从提交信息里提取紧跟标记的步骤号（`step N` / `步骤 N`）。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:256`）：`to_ascii_lowercase` 后扫 `step` 与 `步骤` 标记（仅 ASCII 折叠大小写，中文标记不受影响）；标记后跳过空白/全角半角冒号/顿号/句点，紧跟的连续 ASCII 数字解析为步号；正文里任意位置的数字不算步号（F15：数字必须紧跟标记，防幻影步号）；去重（已含的不重复 push）。幂等、无副作用。
- 参数：`subject: &str` — 提交信息。
- 返回：`Vec<u32>` — 出现的步骤号（去重，顺序按出现）。
- 异常：N/A（纯字符串解析，无失败面；找不到标记时返回空向量）。
- 示例（🟢 CONFIRMED `src/change/coding_gate.rs:669` 与 `:676`）：`step 3` → `[3]`；`步骤 2` → `[2]`；`step 2 step 2` → `[2]`（重复去重）；`step 8` 后正文含「…心跳的步骤字段…撞 409」→ `[8]`（正文 409 不中）；仅「步骤字段里提到 409」→ 空。

### check_step_commits

- 摘要：校验「步骤 ↔ 提交」一一对应。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:283`）：
  - 取 `commits_since`；对每个提交，subject 必须同时含 `change_id` 与 `COMMIT_PREFIX` 去尾冒号的 `change`，否则 `Quality`（`缺 change-id 或 change 前缀，无法机械归属`）。
  - 每个提交须经 `step_numbers_in` 提出 ≥1 个步号，否则 `Quality`（`没写步骤号（应含 step N）`）。
  - 步号不得重复（`步骤 N 被提交了不止一次（一歩一提交）`）。
  - 收集声明步号排序后须与 `steps` 的 number 序列完全相等，否则 `Quality`（`实施步骤与提交对不上：impl-steps 有 …，提交声明 …`）。注意只有提交声明被排序，`steps` 的期望序列按解析顺序参与比较——实施步骤文件里的步骤号须按升序书写，否则即使集合相同也会判对不上。
  - 全对 → 返回声明步号数。
- 参数：`repo: &Path`（git 仓库）、`baseline: &str`、`change_id: &str`、`steps: &[Step]`。
- 返回：`Result<usize>` — 对应上的声明步号个数。
- 异常：`Quality`（前缀缺失 / 无步号 / 步号重复 / 序列对不上）；转发 `commits_since` 的 `System`/`Quality`。
- 示例（🟢 CONFIRMED `src/change/coding_gate.rs:610` / `:626` / `:640` / `:650`）：两提交 `step 1`、`step 2` 对步骤 `{1,2}` → `Ok(2)`；只提交 `step 1` 却对步骤 `{1,2}` → `Err`（对不上）；提交「随手改一下」（无 change 前缀）→ `Err`（无法机械归属）；提交有前缀但无步号（如「顺手改」）→ `Err`（没写步骤号）。

### impl_steps_path

- 摘要：黑板到实施步骤文件的路径（编排层用）。
- 行为语义：`blackboard.join(IMPL_STEPS)`，纯路径拼接、无 IO。幂等、无副作用。（🟢 CONFIRMED `src/change/coding_gate.rs:323`）
- 参数：`blackboard: &Path`。
- 返回：`PathBuf`。
- 异常：N/A（纯路径拼接，无 IO 失败面）。
- 示例：编排层用它定位实施步骤全文路径以渲染 / 提示。

### parse_evidence_log（内部）

- 摘要：两道门共用的证据解析（命令行 + 退出码）。
- 行为语义（🟢 CONFIRMED `src/change/coding_gate.rs:37`）：`file` 与 `label` 是参数——同一形状服务基线/自证两道门，错误文案指向各自 `file`。逐行扫：`command`（或中文 `命令`，后带冒号）行记录命令；以 `exit_code`、`exit code`、`exit`、`退出码` 为前缀（键后带冒号、大小写不敏感）取紧随数字（可选负号）；markdown 列表符号（`-`/`*`/`+`）与粗体（`**N**`）容差；粗体后散文取前导整数；首中即停（文件最前的有效退出码为主，后续反向验证不覆盖）。扫完无退出码 → `Quality`（文案 `{label}证据缺退出码（{file} 里要写 exit_code: N）…`）。
- 参数：`text: &str`、`file: &str`（证据文件名）、`label: &str`（「基线」/「自证」）。
- 返回：`Result<BaselineEvidence>`。
- 异常：`Quality`（`DevforgeError` 变体）（缺退出码）。
- 示例：`- exit_code: **0**（44 passed / 0 failed）`（列表项 + 粗体 + 散文）→ 退出码 0（`src/change/coding_gate.rs:537`）；后续反向 `- exit_code: **101**` 不覆盖首行 0（`src/change/coding_gate.rs:544`）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `green_baseline_log_passes` | 绿基线过 | `src/change/coding_gate.rs:379` |
| `red_baseline_blocks_coding` | 红基线拒 + 文案带命令 | `src/change/coding_gate.rs:393` |
| `missing_log_is_quality_failure` | 缺基线文件 Quality | `src/change/coding_gate.rs:408` |
| `log_without_exit_code_is_rejected` | 无退出码拒 | `src/change/coding_gate.rs:417` |
| `baseline_log_error_text_still_points_at_the_baseline_file` | 文案仍指向基线文件 | `src/change/coding_gate.rs:426` |
| `missing_self_check_log_is_quality_failure` | 缺自证文件 Quality、不指向基线 | `src/change/coding_gate.rs:438` |
| `self_check_without_exit_code_is_rejected` | 自证无退出码拒 | `src/change/coding_gate.rs:450` |
| `red_self_check_is_rejected_with_exit_code` | 红自证拒 | `src/change/coding_gate.rs:469` |
| `green_self_check_passes` | 绿自证过 | `src/change/coding_gate.rs:487` |
| `parse_baseline_log_keeps_its_single_argument_signature` | 签名契约（testing_gate 依赖） | `src/change/coding_gate.rs:501` |
| `parse_accepts_markdown_list_item_evidence` | 列表项容差 | `src/change/coding_gate.rs:507` |
| `parse_still_rejects_list_item_without_exit_code` | 列表项缺退出码仍拒 | `src/change/coding_gate.rs:516` |
| `parse_accepts_bold_list_item_exit_code` | 粗体容差 | `src/change/coding_gate.rs:526` |
| `parse_bold_exit_code_with_prose_tail_takes_leading_int` | 粗体后散文取前导整数 | `src/change/coding_gate.rs:537` |
| `parse_first_exit_code_wins_over_later_reverse_validation` | 首中即停 | `src/change/coding_gate.rs:544` |
| `regression_claim_text_is_unchanged` | 回归文案不变（testing_gate 同源） | `src/change/coding_gate.rs:555` |
| `decision_ledger_registers_d023` | 决策台账登记 D023 | `src/change/coding_gate.rs:566` |
| `self_check_and_baseline_logs_are_independent_files` | 两门独立各读各文件 | `src/change/coding_gate.rs:580` |
| `steps_are_parsed_from_headings` | heading 解析 | `src/change/coding_gate.rs:599` |
| `step_count_matches_commits` | 步数=提交数过 | `src/change/coding_gate.rs:610` |
| `missing_step_commit_fails` | 缺一步拒 | `src/change/coding_gate.rs:626` |
| `duplicate_step_or_missing_prefix_fails` | 缺 `change` 前缀的提交拒（「无法机械归属」；重复步号分支在函数内但该例未单独触发） | `src/change/coding_gate.rs:640` |
| `extra_commit_without_step_number_fails` | 多余提交无步号拒 | `src/change/coding_gate.rs:650` |
| `empty_impl_steps_is_quality_failure` | 空实施步骤 Quality | `src/change/coding_gate.rs:660` |
| `step_numbers_in_subject_handles_chinese_and_repeats` | 中文 + 重复去重 | `src/change/coding_gate.rs:669` |
| `step_numbers_ignore_digits_in_prose_after_a_marker` | 正文数字不算 | `src/change/coding_gate.rs:676` |
| `eleven_steps_with_prose_digits_reconcile_cleanly` | 11 步散文数字对账 | `src/change/coding_gate.rs:689` |

## 测试运行与环境

- 测试与被测代码同文件：`#[cfg(test)] mod tests`（`src/change/coding_gate.rs:327` 起），共 27 例，上表逐例给出位置。
- 运行单模块单测：`cargo test --lib change::coding_gate`（🟡 INFERRED：Rust 单测的默认入口；本次勘察环境 rustup 未配置默认 toolchain，未能实跑复现，命令形态以 `cargo test --lib <过滤子串>` 为准）。
- 运行依赖：`tempfile`（dev-dependency，`Cargo.toml:27`，用于 `tempdir`）；步骤门的 4 个用例会起 `git` 子进程（`git init/commit/rev-parse`，`src/change/coding_gate.rs:331`/`:344`/`:364`），本机须有可用 `git`。
- 无外部服务、无网络、无固定端口——纯文件系统 + 临时 git 仓库，可并行跑。
