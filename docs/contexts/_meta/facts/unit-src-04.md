# 勘察事实 — unit-src-04（src/change/coding_gate.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/coding_gate.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/coding_gate.rs`。
- 模块声明：`src/change/mod.rs:12` `pub mod coding_gate;`；`src/change/mod.rs:29` `pub use coding_gate::{SELF_CHECK_LOG, check_baseline_log, check_self_check, check_step_commits, read_steps};`。
- 文件头模块注释：变更 coding 阶段的两道机械门（P009 T010 / T010b）——回归前置 + 实施步骤按步提交；失败即 `DevforgeError::Quality`。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `BASELINE_LOG` | `#16` | `pub const &str` = `"evidence/baseline-test.log"`（证据文件相对黑板） |
| `SELF_CHECK_LOG` | `#18` | `pub const &str` = `"evidence/self-check.log"` |
| `IMPL_STEPS` | `#20` | `pub const &str` = `"impl-steps.md"` |
| `COMMIT_PREFIX` | `#22` | `pub const &str` = `"change:"`（提交信息里的 change-id 标记） |
| `BaselineEvidence` | `#26` | `#[derive(Debug,Clone,PartialEq,Eq)] pub struct { pub exit_code:i32, pub command:String }` |
| `parse_baseline_log` | `#104` | `pub fn(&str) -> Result<BaselineEvidence>` 包 `parse_evidence_log(text, BASELINE_LOG, "基线")` |
| `check_baseline_log` | `#109` | `pub fn(&Path) -> Result<BaselineEvidence>`：读文件+判绿，红→Quality |
| `check_self_check` | `#135` | `pub fn(&Path) -> Result<BaselineEvidence>`：读自证文件+判绿 |
| `Step` | `#160` | `#[derive(Debug,Clone,PartialEq,Eq)] pub struct { pub number:u32, pub title:String }` |
| `parse_steps` | `#165` | `pub fn(&str) -> Vec<Step>`：从 heading 解析步骤号 |
| `read_steps` | `#191` | `pub fn(&Path) -> Result<Vec<Step>>`：读 `impl-steps.md`，空→Quality |
| `changed_files` | `#210` | `pub fn(&Path,&str) -> Result<Vec<String>>`：`git diff --name-only <baseline>` |
| `commits_since` | `#231` | `pub fn(&Path,&str) -> Result<Vec<String>>`：`git log --format=%s <baseline>..HEAD` |
| `step_numbers_in` | `#256` | `pub fn(&str) -> Vec<u32>`：提交信息里紧跟标记的步骤号 |
| `check_step_commits` | `#283` | `pub fn(&Path,&str,&str,&[Step]) -> Result<usize>`：步骤↔提交一一对应 |
| `impl_steps_path` | `#323` | `pub fn(&Path) -> PathBuf`：`blackboard.join(IMPL_STEPS)` |

## 内部符号

- `parse_evidence_log`（`#37`）：`fn(&str,&str,&str) -> Result<BaselineEvidence>`，非 pub；两道门共用解析形状；错误文案指向 `label` 对应文件。
- 测试辅助（`#[cfg(test)]`）：`git`、`commit`、`repo_with_baseline`。

## 行为/测试事实（`#[cfg(test)]`，27 例）

- 基线门：绿 `exit_code: 0` 过（`#379`）；红 `exit_code: 2` 拒且文案带命令（`#393`）；缺文件 Quality（`#408`）；无退出码拒（`#417`）；错误文案仍指向基线文件（`#426`）。
- 自证门：缺文件 Quality 且指向自证文件、不得指向基线（`#438`）；无退出码拒（`#450`）；红 `exit_code:1` 拒（`#469`）；绿过（`#487`）。
- 解析容差：`parse_baseline_log` 保持单参数签名（`#501`）；接受 markdown 列表项 `- exit_code:`（`#507`）；列表项缺退出码仍拒（`#516`）；粗体 `**0**` 接受（`#526`）；粗体带散文取前导整数（`#537`）；首中即停，反向 `**101**` 不覆盖主结果（`#544`）。
- 文案契约：`回归日志不可解析` 开头不变（`#555`，testing_gate）；决策台账登记 D023（`#566`，读 `docs/plantree/plans/009-change-sop/decisions/README.md`）；两门各读各文件（`#580`）。
- 步骤门：从 heading 解析（`#599`）；步数与提交一一对上（`#610`）；缺一步对不上拒（`#626`）；重复步/缺前缀拒（`#640`）；多余提交无步号拒（`#650`）；空 impl-steps Quality（`#660`）。
- step_numbers_in：中文/重复去重（`#669`）；标记后正文数字不算（`#675→#676`）；11 步带散文数字对账 `1..=11`（`#689`）。

## 口径（代码内可见规则）

- 证据日志允许写法：`exit_code: 0` / `exit: 0` / `退出码: 0` / `退出码：0`（大小写不敏感）；列表符号 `-/*/+` 容差；粗体 `**N**` 容差；`command:` / `命令:` 行记录命令。
- 首中即停：主退出码取文件最前；后续反向验证 `exit_code` 不覆盖。
- 步骤格式：`## 步骤 1` / `### 步骤 2` / `## Step 3`（`步骤`/`Step`/`step` 前缀，冒号/顿号/句号分隔）。
- 提交信息格式：`change: <change-id> step N` / `步骤 N`；数字必须紧跟标记（F15），正文数字不算（F16 幻影步号防护）。
- 错误类型：缺文件/红/对不上 → `DevforgeError::Quality`；`git diff` 启动失败 → `DevforgeError::System`；`git log` 启动失败 → System、命令失败 → Quality。

## crate 内调用点（消费者）

- `src/change/mod.rs:12/29` 模块声明与转发导出。
- `src/change/session.rs:13-14` 引入；`:98` `BASELINE_LOG`；`:120` `SELF_CHECK_LOG`；`:160` `SELF_CHECK_LOG`；`:206` `parse_baseline_log`（复用门解析口径）；`:389` `check_baseline_log`；`:404` 基线条目；`:428` `check_self_check`；`:447` `read_steps`；`:449` `check_step_commits`；`:585` `BASELINE_LOG`。
- `src/change/testing_gate.rs:13` 引入 `parse_baseline_log`；`:29` 用 `parse_baseline_log` 包裹回归日志；`:28` `check_regression_claim`（文案「回归日志不可解析」`:31` 与其同源）。

## 外部依赖

- 错误面：`crate::error::{DevforgeError, Result}`（`src/change/coding_gate.rs:8`）。Quality / System 变体。
- `std::path`、`std::process::Command`（git）。
- 测试：`tempfile`（tempdir）。
- 无其他外部 crate 依赖。
