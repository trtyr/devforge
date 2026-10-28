---
type: module
module_path: src/change/history.rs
unit: unit-src-08
description: 往返历史——每次 coding→plan 回退把越界文件逐行追加到变更的 history.md
---

# src/change/history.rs

往返历史（P009 T013）：每次 coding→plan 回退都落一行到 `docs/plantree/changes/<change-id>/history.md`，供事后审计「边界是怎么被撑大的」（`src/change/history.rs:1` 至 `src/change/history.rs:2`）。

## 职责

**负责**

- `history_path`：按仓库根与 change-id 拼出历史文件路径。
- `append_roundtrip`：把一次往返的每个 `ImpactGap` 追加为表格一行；文件不存在则带表头新建。

**不负责**

- 不判定是否越界、是否该回退（`src/change/boundary.rs` 的 `BoundaryVerdict` 决定）。
- 不维护往返计数本身：`roundtrip` 由调用方传入，计数在状态里递增（`src/change/orchestrator.rs:244`）。
- 不校验往返上限：`check_roundtrip` 在调用点先跑（`src/change/orchestrator.rs:243`）。
- 不生成报告正文；不做去重（重复调用会累积重复行）。

## 公开成员

| 成员 | 种类 | 位置 | 条目 |
|---|---|---|---|
| `history_path` | pub fn | `src/change/history.rs:10` | [history_path](#history_path) |
| `append_roundtrip` | pub fn | `src/change/history.rs:17` | [append_roundtrip](#append_roundtrip) |

模块内无其它公开符号。

## 依赖

**上游 / 消费方**

| 消费方 | 用法 | 位置 |
|---|---|---|
| `src/change/mod.rs` | 声明子模块并转发导出两个函数 | `src/change/mod.rs:15`、`src/change/mod.rs:32` |
| `src/change/orchestrator.rs` | 边界判定为「回退到计划」时：先 `check_roundtrip`、再 `bump_roundtrip`、再 `append_roundtrip` | `src/change/orchestrator.rs:243`、`src/change/orchestrator.rs:244`、`src/change/orchestrator.rs:246` |
| `src/change/orchestrator_tests.rs` | 用 `history_path` 定位并读回历史，断言越界文件已落盘 | `src/change/orchestrator_tests.rs:288` |

**向下依赖**

- `ImpactGap`（导入见 `src/change/history.rs:6`；结构定义在 `src/change/boundary.rs:17`，字段 file / reason / evidence 见 `src/change/boundary.rs:18` 至 `src/change/boundary.rs:20`）。
- `DevforgeError` 与 `Result`（`src/change/history.rs:7`）。
- 标准库：`Path` 与 `PathBuf`（`src/change/history.rs:4`）以及文件读写。无 git、无网络、无外部 crate。
- 测试：`tempfile`（dev-dependency，`Cargo.toml:27`）。

## 核心口径

- **文件布局**（🟢 CONFIRMED `src/change/history.rs:10` 至 `:13`）：history 落在台账根（plantree 根，2026-10-25 起**不进仓**）下：
  ```text
  <台账根>/changes/<change-id>/history.md
  ```
  台账根由 `prepare::default_plantree_root(project)` 给出：DEVFORGE_HOME（缺省用户主目录下 .devforge）/plantree/&lt;project&gt;（`src/change/prepare.rs:39` 至 `:45`）；旧的仓内 docs/plantree 路径已废弃（history.rs 头注释记了这次反转）。
- **表头（仅首次建文件时写）**（🟢 CONFIRMED `src/change/history.rs:31`）：
  ```text
  # 变更往返历史 · <change-id>

  | 往返 | 越界文件 | 理由 | 证据 |
  | --- | --- | --- | --- |
  ```
- **数据行**（每个 gap 一行，🟢 CONFIRMED `src/change/history.rs:34` 至 `src/change/history.rs:37`）：`` | <往返> | `<file>` | <reason> | <evidence> | ``，即文件名加反引号、其余字段原样插入。
- **与往返上限的配合**：上限常量为 3（`src/change/boundary.rs:13`），超限即 `Quality` 错误并要求问人（`src/change/boundary.rs:159` 至 `src/change/boundary.rs:165`）；配合调用点顺序（`src/change/orchestrator.rs:243` 至 `src/change/orchestrator.rs:250`）可知历史最多被写到第 3 次回退（🟡 INFERRED：由「先校验上限、再递增计数、再落历史」的调用顺序推得）。
- **markdown 未转义**：reason 与 evidence 原样拼进表格单元格，不处理竖线与换行（🟡 INFERRED：代码里只有文本格式化拼接，无清洗步骤，`src/change/history.rs:34`）；含竖线的理由会打乱列——登记为缺口（`_meta/gaps.md`）。

---

### history_path

- **摘要**：返回某个 change 的往返历史文件路径。
- **行为语义**（🟢 CONFIRMED `src/change/history.rs:10` 至 `src/change/history.rs:14`）：纯路径拼接成「仓库根 / docs/plantree/changes / change-id / history.md」；无 IO、无副作用、幂等，同一输入恒定输出同一路径。不校验 change-id 合法性（空串会拼出多余分隔符形态的路径，🟡 INFERRED：仅由路径拼接语义推得）。
- **参数**：
  - `repo: &Path` — 仓库根（目标仓，计划与历史都落这里）。
  - `change_id: &str` — 变更 id，作为子目录名。
- **返回**：`PathBuf` —— 历史文件路径；文件可以尚不存在。
- **异常**：N/A —— 无 IO，不可能失败。
- **示例**：编排层测试用它在临时仓里定位历史文件，再读回断言内容（`src/change/orchestrator_tests.rs:288` 至 `src/change/orchestrator_tests.rs:293`）。

### append_roundtrip

- **摘要**：把一次往返的越界文件逐行追加进历史文件，返回该文件路径。
- **行为语义**（🟢 CONFIRMED `src/change/history.rs:17` 至 `src/change/history.rs:42`）：
  1. 用 `history_path` 定位，取父目录（以 expect 断言「history 必有父目录」，路径至少三层故恒成立）。
  2. 建父目录；失败 → `System`（文案带目录路径，`src/change/history.rs:26`）。
  3. 文件存在 → 整文件读入；**读失败时静默当空串**（`src/change/history.rs:29`），于是不补表头、只追数据行。文件不存在 → 以表头模板作初值（`src/change/history.rs:31`）。
  4. 按切片顺序，为每个 `ImpactGap` 追加一行。
  5. 整文件覆盖写回；失败 → `System`（文案带文件路径，`src/change/history.rs:40`）。
  - **幂等性**：无。重复调用会重复追加；同一往返传两次相同 gap 会得到两条一模一样的行。
  - **副作用**：可能创建 `docs/plantree/changes/<change-id>/` 目录链；只写这一个文件。
  - **并发**：读-改-写无锁（🟡 INFERRED：编排层串行调用，代码里无锁也无追加模式写入）。
- **参数**：
  - `repo: &Path` — 仓库根。
  - `change_id: &str` — 变更 id（同时用于路径与表头文案）。
  - `roundtrip: usize` — 往返序号，原样写进第一列；调用方从状态取（`src/change/orchestrator.rs:246` 至 `src/change/orchestrator.rs:251`）。
  - `gaps: &[ImpactGap]` — 本次越界文件及其理由/证据；空切片时只建文件（或读原文）不追加行，仍返回路径（🟡 INFERRED：循环体不执行，随后写回原文本）。
- **返回**：`Result<PathBuf>` —— 实际写入的历史文件路径（与 `history_path` 返回值相同）。
- **异常**：`DevforgeError` 的 `System` 变体 —— 建父目录失败（`src/change/history.rs:26`）或写文件失败（`src/change/history.rs:40`）。读失败不上抛，降级为空串。
- **示例**：
  - 首次追加 → 建文件，表头只出现一次，行内出现往返序号与文件名（`src/change/history.rs:57`）。
  - 第二次追加（往返 2）→ 旧行保留、表头计数仍为 1（`src/change/history.rs:68`）。
  - 同一次往返两个 gap → 往返序号出现两次（`src/change/history_tests.rs`）。

## 测试覆盖（`#[cfg(test)]`，3 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `first_roundtrip_creates_history_with_header` | 首次建文件带表头 + 数据行 | `src/change/history.rs:57` |
| `later_roundtrips_append_without_losing_history` | 追加不丢旧行；表头只出现一次 | `src/change/history.rs:68` |
| `multiple_gaps_in_one_roundtrip_all_recorded` | 同一往返多 gap → 每 gap 一行 | `src/change/history_tests.rs` |

测试模块在 `src/change/history.rs:44` 起；夹具 `gap`（`src/change/history.rs:48` 至 `src/change/history.rs:54`）直接构造 `ImpactGap`：文件名作 file、固定理由「接口在这」、证据为「文件名加冒号 9」。断言全部基于写出的 markdown 文本子串，不依赖格式细节。

## 测试运行与环境

- **工具链前提**：edition 2024，需 Rust 1.85+。多 toolchain 且无默认值时显式指定：
  ```bash
  RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable cargo --version
  ```
- **依赖安装**：仅 `tempfile`（dev-dependency，`Cargo.toml:27`），跑测试时自动获取；不需要 git、端口、外部服务或网络。
- **隔离**：每个用例建独立临时目录（用 `tempfile` 的 `tempdir`），所有写入落在临时目录，不碰真实台账（changes 目录现已在仓库外）；可并行。
- **命令**（🟢 CONFIRMED，本单元实跑：`cargo test --lib change::history`）：
  ```bash
  CARGO_TARGET_DIR=$(mktemp -d)/target cargo test --lib change::history
  ```
  结果：3 passed、0 failed、595 filtered out，退出码 0；构建产物写在仓外临时目录，仓内 target 目录未被写入。
- **端到端落点**：`src/change/orchestrator_tests.rs:288` 在真实编排流程里读回历史文件校验内容。
- **不适用声明**：无服务、无端口、无健康检查（N/A —— 纯函数模块）；无配置项（N/A —— 不读环境变量、不读配置文件，路径全部由参数传入）；无种子数据（N/A —— 历史文件按需新建，测试自建临时目录）；公开接口清单即本文「公开成员」表（N/A —— 无 HTTP/CLI 端点）。

## 相关文档

- 同单元：[src/change/gate0.rs](./gate0.rs.md)（门 0 基线与档位）
- 上下游：[src/change/boundary.rs](./boundary.rs.md)（`ImpactGap`、往返上限、`check_roundtrip`）、[src/change/orchestrator.rs](./orchestrator.rs.md)（唯一生产调用点）、[src/change/state.rs](./state.rs.md)（往返计数）
- 缺口：`_meta/gaps.md`（markdown 未转义、读失败静默降级、重复调用不去重，三条已登记）
