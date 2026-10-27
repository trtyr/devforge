---
type: module
module_path: src/change/prepare.rs
unit: unit-src-15
description: 门 0 预检——一次把基线、文档档位、change-id、计划目录与黑板目录备好
---

# src/change/prepare.rs

change run 的门 0 预检（P009 接线）：一次把「基线 / 文档档位 / change-id / 黑板目录」备好。**顺序有依赖**——change-id 决定计划目录 → 计划目录决定 baseline.txt 落点 → 基线又是 coding 的 diff 锚点；全是确定性动作，可单测，不碰 agent、不跑 pi（`src/change/prepare.rs:1` 至 `:4`）。

## 职责

**负责**

- 入口校验：目标仓必须是目录；档位字符串必须合法。
- 分配或复用 change-id，算出计划目录（**目标仓内**）与黑板目录（**plans 根下**）两个不同落点。
- 建计划目录 `repo/docs/plantree/changes/<change-id>`。
- 按档位执行文档动作（`none` 跳过 / `essential` 写清单 / `complete` 直接拒绝）。
- 建基线并把 sha 落到计划目录（委托 `ensure_baseline`）。
- 建黑板骨架三个子目录：`evidence`、testing/reports、coding/reports。
- 把上述结果打包成 `ChangePrep` 返回。

**不负责**

- 不创建 agent、不启动会话、不跑模型（文件头注释明说可单测、不碰 agent）。
- 不生成 change-id 的格式（`src/change/change_id.rs` 的 `assign_change_id` / `date_segment`）。
- 不做 git 细节（`src/change/gate0.rs` 的 `ensure_baseline`）。
- 不做文档内容（`src/change/docs.rs` 的 `apply`）；`complete` 档不代跑 doc-from-zero，只报错要求调用方先跑。
- 不写计划正文、不写报告：计划由 plan 阶段写，报告由 `src/change/report.rs` 写。

## 公开成员

| 成员 | 种类 | 位置 | 条目 |
|---|---|---|---|
| `ChangePrep` | pub struct | `src/change/prepare.rs:15` | [ChangePrep](#changeprep) |
| `prepare` | pub fn | `src/change/prepare.rs:30` | [prepare](#prepare) |

模块内无其它公开符号；私有辅助只有测试夹具 `repo_with_code`（`src/change/prepare.rs:88`）。

## 依赖

**上游 / 消费方**

| 消费方 | 用法 | 位置 |
|---|---|---|
| `src/change/mod.rs` | `pub mod prepare;` 与转发导出（别名 `prepare_change`，仓内无调用点） | `src/change/mod.rs:18`、`src/change/mod.rs:37` |
| `src/server/change_engine.rs` | 门 0 唯一生产调用点：从环境变量取档位、取今天日期、调 `prepare` | `src/server/change_engine.rs:10`、`src/server/change_engine.rs:101`、`src/server/change_engine.rs:102`、`src/server/change_engine.rs:103` |
| `src/server/change_engine.rs` | 消费 `ChangePrep`：日志用 `change_id`/`baseline`/`doc_level`；回读计划用 `change_dir`；`build_ctx` 把字段搬进 `ChangeCtx` | `src/server/change_engine.rs:114`、`src/server/change_engine.rs:121`、`src/server/change_engine.rs:124`、`src/server/change_engine.rs:246` 至 `:254` |
| `src/server/change_engine.rs` | 单测内构造一次 | `src/server/change_engine.rs:317` |

**向下依赖**

| 依赖 | 用途 | 位置 |
|---|---|---|
| `assign_change_id` / `date_segment` | 生成/复用 change-id 与日期段 | `src/change/prepare.rs:8`、`src/change/change_id.rs:55`、`src/change/change_id.rs:87` |
| `change_root` | 算黑板根（走模块路径，未 import） | `src/change/prepare.rs:50`、`src/change/change_id.rs:15` |
| `apply` 与 `DocAction` | 按档位执行/表示文档动作 | `src/change/prepare.rs:9`、`src/change/docs.rs:31`、`src/change/docs.rs:17` |
| `ensure_baseline` / `Baseline` / `DocLevel` | 建基线、值对象、档位解析 | `src/change/prepare.rs:10`、`src/change/gate0.rs:66`、`src/change/gate0.rs:17`、`src/change/gate0.rs:24` |
| `DevforgeError` / `Result` | 错误面 | `src/change/prepare.rs:11` |
| `Path` / `PathBuf` | 参数与字段类型 | `src/change/prepare.rs:6` |

外部 crate：生产无；测试用 `tempfile`（dev-dependency，`Cargo.toml:27`）。

## 核心口径

**执行顺序与产物**（🟢 CONFIRMED `src/change/prepare.rs:30` 至 `:81`）

| # | 动作 | 产物 / 落点 | 失败面 |
|---|---|---|---|
| 1 | 校验目标仓是目录 | — | `Config`（文案含「不存在」） |
| 2 | 解析档位 | — | `Config`（未知档位，见 `src/change/gate0.rs:34`） |
| 3 | 日期段 + 分配/复用 change-id | 黑板下 `current` 记录文件 | `System`（建目录/写记录失败，由 `assign_change_id` 报） |
| 4 | 建计划目录 | `repo/docs/plantree/changes/<change-id>` | `System`（建目录失败） |
| 5 | 按档位执行文档动作 | `none` 无；`essential` 写 `docs/contexts/_meta/` 下清单与词表；`complete` 拒绝 | `Quality`（complete）；`System`（essential 写盘失败） |
| 6 | 建基线 | 计划目录下 baseline.txt（git 侧可能新建仓库/提交） | `Config`/`System`（见 `src/change/gate0.rs`） |
| 7 | 建黑板骨架 | `plans/<project>/<task>/change/` 下 `evidence`、testing/reports、coding/reports | `System`（建目录失败） |

**两个落点不能混**（🟢 CONFIRMED `src/change/prepare.rs:51` 至 `:55`、用例 `src/change/prepare.rs:96`）

- 计划目录 = 目标仓内 `docs/plantree/changes/<change-id>`（D003：计划要可提交、可审查、可回查）。
- 黑板 = plans 根下 `<plans_root>/<project>/<task>/change`（状态、证据、实施步骤）。
- 用例显式断言 `change_dir` **不以** `blackboard` 为前缀（`src/change/prepare.rs:114` 至 `:117`）。

**幂等**（🟢 CONFIRMED `src/change/prepare.rs:204` 至 `:217`）

- 同一（plans_root, project, task）重复调用：`change_id` 与 `change_dir` 不变（marker 记录复用，见 `src/change/change_id.rs:62` 至 `:73`），**需求文本与日期不同也不换号**。
- 基线不重建：第二次 `created_repo=false`、sha 不变（baseline.txt 已存在即复用，见 `src/change/gate0.rs:73` 至 `:82`）。
- 目录创建全部 `create_dir_all`，可重复执行。

**失败面**（🟢 CONFIRMED `src/change/prepare.rs:41` 至 `:80`）

| 变体 | 触发条件 |
|---|---|
| `Config` | 目标仓不是目录（`src/change/prepare.rs:41` 至 `:45`）；档位字符串非法（`src/change/gate0.rs:39`） |
| `Quality` | 档位为 `complete`——要求调用方先跑 doc-from-zero，本层不代跑（`src/change/prepare.rs:59` 至 `:62`） |
| `System` | 建计划目录失败、黑板子目录失败、`assign_change_id` 写盘失败、基线相关 git/IO 失败 |

---

### ChangePrep

- **职责**：门 0 的结论包——把后续各阶段要用的五个落点/状态一次带出。纯数据，无方法、无 IO。
- **构造 / 初始化**：只能由 `prepare` 返回（字段虽然全 `pub`，但生产路径不手工构造）。派生 `Debug, Clone, PartialEq, Eq`（🟢 CONFIRMED `src/change/prepare.rs:14`）。
- **公开成员**：
  - `change_id`（`String`）— 本次变更 id，形如 `change-<YYYYMMDD>-<slug>`（用例给出 `change-20261020-add-book-api`，`src/change/prepare.rs:104`）。
  - `blackboard`（`PathBuf`）— 黑板根：`<plans_root>/<project>/<task>/change`（`src/change/prepare.rs:17` 至 `:18`）。
  - `change_dir`（`PathBuf`）— 计划目录：目标仓内 `repo/docs/plantree/changes/<change_id>`（`src/change/prepare.rs:19` 至 `:20`）。
  - `baseline`（`Baseline`）— 基线锚点（sha + 是否本次新建仓库）。
  - `doc_level`（`DocLevel`）— 解析后的档位（`Copy` 值）。
  - `doc_action`（`DocAction`）— 文档动作结论（`Skipped` / `EssentialDone` / `FullChainRequired`）。
- **生命周期**：值对象，随 `prepare` 返回即固定；服务层随即把字段搬进 `ChangeCtx`（`src/server/change_engine.rs:246` 至 `:254`），此后不再回写。
- **典型用法**：`change_engine` 拿到后立刻读三处——`change_id`/`baseline`/`doc_level` 打日志（`src/server/change_engine.rs:114` 至 `:116`）、`blackboard` 作为会话工作与证据根（`src/server/change_engine.rs:121`）、`change_dir` 拼接 change-plan.md 回读计划（`src/server/change_engine.rs:124`）。
- **注意**：`doc_action` 在**实际返回的** `ChangePrep` 里只可能是 `Skipped` 或 `EssentialDone`——`FullChainRequired` 分支在返回前就转成 `Quality` 错误了（🟢 CONFIRMED `src/change/prepare.rs:58` 至 `:62`）。见「缺口」。

### prepare

- **摘要**：门 0 预检——校验、发号、建目录、跑档位动作、建基线、建黑板骨架，返回 `ChangePrep`。
- **行为语义**（🟢 CONFIRMED `src/change/prepare.rs:30` 至 `:81`）：
  1. `repo` 不是目录 → 立刻 `Err(Config)`，不产生任何副作用。
  2. `DocLevel::parse(doc_level)` 失败 → `Err(Config)`（同样无副作用）。
  3. `date_segment(today 三元组)` 得 `YYYYMMDD`，交 `assign_change_id` 取/建 change-id；`assign_change_id` 返回的第二项（黑板内 id 目录）被丢弃。
  4. 建计划目录（目标仓内）。
  5. `docs::apply(repo, level)`：`none` → `Skipped`；`essential` → 写 `docs/contexts/_meta/` 下 inventory.json 与 glossary.md 并返回 `EssentialDone`；`complete` → 返回 `FullChainRequired`，本函数随即 `Err(Quality)`。
  6. `ensure_baseline(repo, plan_dir)`：基线 sha 落**计划目录**下的 baseline.txt。
  7. 建黑板三个子目录。
  8. 组装并返回 `ChangePrep`。
  - 副作用：建目录、写 `current` 记录、可能新建 git 仓库与提交、`essential` 档会写文档清单、可能报错（此时前面的副作用已生效）。
  - 幂等：同任务重复调用不换号、基线复用（见「核心口径 · 幂等」）。
  - 并发：无锁；同一任务并发两次可能同时写 `current` 与 baseline.txt（🟡 INFERRED：未见文件锁，只有 marker 存在性判断）。
- **参数**：
  - `plans_root: &Path` — plans 根（黑板前缀）；生产传服务实例自己的 plans 根（`src/server/change_engine.rs:104`）。
  - `repo: &Path` — 目标仓库根，必须已存在；生产传任务 workspace（`src/server/change_engine.rs:105`）。
  - `project: &str` — 项目名，参与黑板路径。生产传任务项目（`src/server/change_engine.rs:106`）。
  - `task: &str` — 任务 id，参与黑板路径。生产传任务 id（`src/server/change_engine.rs:107`）。
  - `requirement: &str` — 需求原文，**只用于 slug**（不落盘）；生产传需求文件内容（`src/server/change_engine.rs:108`）。
  - `doc_level: &str` — 档位字符串（`none`/`essential`/`complete`，大小写不敏感）；生产来自环境变量 `DEVFORGE_DOC_LEVEL`，缺省 `none`（`src/server/change_engine.rs:101`）。
  - `today: (i32, u32, u32)` — 年月日三元组，为可测而外置；生产用 `today_parts()`（`src/server/change_engine.rs:102`）。不做范围校验（见「缺口」）。
- **返回**：`Result<ChangePrep>` —— 见「ChangePrep」字段表；`change_dir` 保证已存在且其中已有 baseline.txt。
- **异常**：
  - `DevforgeError` 的 `Config`：目标仓不存在（文案「目标仓不存在：…」）、档位非法（文案含 `doc_level`）。
  - `DevforgeError` 的 `Quality`：档位 `complete`（文案含 `doc-from-zero`，要求调用方先跑文档链）。
  - `DevforgeError` 的 `System`：建计划目录 / 黑板子目录失败、`assign_change_id` 写盘失败、`ensure_baseline` 的 git 或 IO 失败。
- **示例**（每条对应一个用例，均可在临时目录重放）：
  - 干净目录 + `none` + `(2026,10,20)` + 需求 `add book api` → `change_id` 为 `change-20261020-add-book-api`、`baseline.created_repo=true`、三个黑板子目录就位（`src/change/prepare.rs:96`）。
  - `none` → 目标仓不会出现 `docs/contexts`（`src/change/prepare.rs:152`）。
  - `essential` → `doc_action` 为 `EssentialDone` 且目标仓出现 `docs/contexts/_meta/inventory.json`（`src/change/prepare.rs:169`）。
  - `complete` → `Err`，文案含 `doc-from-zero`（`src/change/prepare.rs:187`）。
  - 同任务第二次（需求与日期都变）→ `change_id`、`change_dir` 不变，`created_repo=false`、sha 不变（`src/change/prepare.rs:204`）。
  - 目标仓路径不存在 → `Err` 含「不存在」（`src/change/prepare.rs:220`）。
  - 档位 `full` → `Err` 含 `doc_level`（`src/change/prepare.rs:236`）。

## 测试覆盖（`#[cfg(test)]`，7 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `bare_repo_gets_baseline_and_blackboard_skeleton` | 无 git 仓 → change-id 成形、计划目录与黑板两处分离、baseline.txt 落计划目录且等于 sha、`created_repo=true`、三个子目录 | `src/change/prepare.rs:96` |
| `none_level_does_not_touch_docs` | `none` 档不建 `docs/contexts` | `src/change/prepare.rs:152` |
| `essential_level_emits_inventory_then_proceeds` | `essential` → `EssentialDone` + 清单文件存在 | `src/change/prepare.rs:169` |
| `complete_level_is_refused_with_reason` | `complete` → 拒绝且文案指路 doc-from-zero | `src/change/prepare.rs:187` |
| `repeated_prepare_reuses_change_id` | 同任务两次（需求/日期都变）不换号、基线复用 | `src/change/prepare.rs:204` |
| `missing_repo_is_rejected` | 目标仓缺失 → `Config` | `src/change/prepare.rs:220` |
| `bad_doc_level_is_rejected` | 非法档位 → `Config` 且文案含 `doc_level` | `src/change/prepare.rs:236` |

测试模块在 `src/change/prepare.rs:84` 起；夹具 `repo_with_code`（临时目录 + src/a.rs，`src/change/prepare.rs:88`）提供「有代码但无 git」的最小仓。

## 测试运行与环境

- **工具链前提**：edition 2024，需 Rust 1.85+。多 toolchain 且未设默认值时显式指定：
  ```bash
  RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable cargo --version
  ```
- **依赖安装**：`tempfile`（dev-dependency，`Cargo.toml:27`），跑测试时自动获取；用例会起 git 子进程（`ensure_baseline` 建基线），PATH 上需有 git；无需网络、端口、外部服务。
- **隔离**：每个用例两个临时根——plans 根（`tempfile` 临时目录下 `plans`）与目标仓（另一个 tempdir）；git 仓库与 baseline.txt 都落在临时目录，不碰真实仓。
- **命令**（🟢 CONFIRMED，本单元实跑：`cargo test --lib change::prepare`）：
  ```bash
  CARGO_TARGET_DIR=$(mktemp -d)/target cargo test --lib change::prepare
  ```
  结果：7 passed、0 failed、591 filtered out，退出码 0；构建产物写在仓外临时目录，仓内 target 目录未被写入。
- **不适用声明**：无服务、无端口、无健康检查（N/A —— 库内函数）；无运行时配置项由本模块读取（N/A —— 档位字符串由调用方传入，环境变量读取在服务层 `src/server/change_engine.rs:101`）；无种子数据（N/A —— 测试自建临时仓）；公开接口清单即本文「公开成员」表（N/A —— 无 HTTP/CLI 端点）。

## 相关文档

- 同链路：[src/change/gate0.rs](./gate0.rs.md)（基线 + 档位）、[src/change/change_id.rs](./change_id.rs.md)（change-id 与黑板根）、[src/change/docs.rs](./docs.rs.md)（档位 → 文档动作）
- 下游：[src/change/session.rs](./session.rs.md)（消费 `ChangeCtx`）、[src/change/orchestrator.rs](./orchestrator.rs.md)（编排门 0 之后的流程）
- 缺口：`_meta/gaps.md`（`doc_action` 的 `FullChainRequired` 在返回值里不可达、失败前的部分副作用、`today` 不校验、并发写 marker 无锁，本单元登记）
