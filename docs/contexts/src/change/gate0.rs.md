---
type: module
module_path: src/change/gate0.rs
unit: unit-src-08
description: 门 0 的确定性部分——基线锚点（Baseline / ensure_baseline）与文档档位（DocLevel）
---

# src/change/gate0.rs

门 0（P009）的确定性部分：**基线锚点**与**文档档位**。文件头注释把口径写成三条（基线 D001 / doc_level D002 / change-id D008，`src/change/gate0.rs:1` 至 `src/change/gate0.rs:9`）。

## 职责

**负责**

- 基线：`ensure_baseline` 让目标仓拥有一条可 diff、可回滚的起点提交，并把该 sha 落到计划目录下的记录文件（文件名由 `BASELINE_FILE` 给定，见「内部符号」）。四种输入状态（记录文件已存在 / 有 git 元数据目录且 HEAD 可解析 / 有 git 元数据目录但空仓 / 无 git 元数据目录）各有确定行为，见「核心口径」的状态表。
- 档位：`DocLevel` 三变体 + 方法 `parse` / `as_str` / `needs_docs`。
- 值对象：`Baseline`（sha 与「是否本次新建仓库」）。

**不负责**

- 不生成 change-id（`src/change/change_id.rs` 负责；本文件只在头注释里声明该口径，`src/change/gate0.rs:8`）。
- 不执行文档动作：档位到动作的翻译在 `src/change/docs.rs` 的 `apply`（`src/change/docs.rs:31`），本文件只解析与表示档位。
- 不判断边界越界（`src/change/boundary.rs`）、不写往返历史（`src/change/history.rs`）。
- 不改写已有提交、不动既有历史：已有仓库时只读 HEAD；「不得新增提交」被写成断言（`src/change/gate0.rs:275` 至 `src/change/gate0.rs:290`）。

## 公开成员

| 成员 | 种类 | 位置 | 条目 |
|---|---|---|---|
| `Baseline` | pub struct | `src/change/gate0.rs:17` | [Baseline](#baseline) |
| `DocLevel` | pub enum | `src/change/gate0.rs:24` | [DocLevel](#doclevel) |
| `parse` | `DocLevel` 的关联函数 | `src/change/gate0.rs:34` | [DocLevel 的 parse](#doclevel-的-parse) |
| `as_str` | `DocLevel` 的方法 | `src/change/gate0.rs:45` | [DocLevel 的 as_str](#doclevel-的-as_str) |
| `needs_docs` | `DocLevel` 的方法 | `src/change/gate0.rs:54` | [DocLevel 的 needs_docs](#doclevel-的-needs_docs) |
| `ensure_baseline` | pub fn | `src/change/gate0.rs:66` | [ensure_baseline](#ensure_baseline) |

模块内另有 1 个私有常量与 4 个私有 helper，见「内部符号」。

## 依赖

**上游 / 消费方**

| 消费方 | 用法 | 位置 |
|---|---|---|
| `src/change/mod.rs` | 声明子模块并转发导出 `Baseline`、`DocLevel`、`ensure_baseline` | `src/change/mod.rs:14`、`src/change/mod.rs:31` |
| `src/change/prepare.rs` | 导入三者；`ChangePrep` 持有 `baseline` 与 `doc_level` 两个字段；解析档位；建基线 | `src/change/prepare.rs:10`、`src/change/prepare.rs:21`、`src/change/prepare.rs:22`、`src/change/prepare.rs:45`、`src/change/prepare.rs:65` |
| `src/change/docs.rs` | `apply` 把档位翻成 `DocAction`：None 跳过、Essential 写清单、Complete 要求外部 doc-from-zero | `src/change/docs.rs:12`、`src/change/docs.rs:31`、`src/change/docs.rs:33` |
| `src/change/session.rs` | `ChangeCtx` 的 `doc_level` 字段；把档位原文写进 plan 阶段指令 | `src/change/session.rs:16`、`src/change/session.rs:41`、`src/change/session.rs:83` |
| `src/server/change_engine.rs` | 从环境变量取档位字符串；门 0 完成后记日志；把 `Baseline` 的 sha 填进 `ChangeCtx` | `src/server/change_engine.rs:101`、`src/server/change_engine.rs:115`、`src/server/change_engine.rs:199`、`src/server/change_engine.rs:252` |
| 集成测试 | 直接构造 None 档作为夹具 | `tests/change_ask_ring.rs:15`、`tests/change_ask_ring.rs:105`、`tests/f6_observability/main.rs:19`、`tests/f6_observability/change.rs:88`、`tests/change_review_protocol/main.rs:19`、`tests/change_review_protocol/support.rs:234`、`tests/change_failure_context/main.rs:19`、`tests/change_failure_context/support.rs:191`、`src/change/session/session_tests/mod.rs:137` |

**向下依赖**

- crate 内：`DevforgeError` 与 `Result`（`src/change/gate0.rs:14`）。
- 标准库：`Path`（`src/change/gate0.rs:11`）、`Command`（`src/change/gate0.rs:12`）。
- 外部进程：git 命令行（不是 crate 依赖，生产路径不链接任何 git 库）。
- 测试：`tempfile`（dev-dependency，`Cargo.toml:27`）。

## 核心口径

**基线（D001）四态**（🟢 CONFIRMED `src/change/gate0.rs:66` 至 `src/change/gate0.rs:112`）

| 输入状态 | 行为 | 是否新建仓库 | 证据 |
|---|---|---|---|
| 记录文件可读且 trim 后非空 | 直接复用文件里的 sha，不读 git | 否 | `src/change/gate0.rs:73` 至 `src/change/gate0.rs:82`；用例 `src/change/gate0.rs:222` |
| 有 git 元数据目录，`git rev-parse HEAD` 成功 | 只记当时 HEAD，不新增提交、不动历史 | 否 | `src/change/gate0.rs:84` 至 `src/change/gate0.rs:92`；用例 `src/change/gate0.rs:275` |
| 有 git 元数据目录，HEAD 不可解析（空仓） | 补一条基线提交再记 HEAD | 否 | `src/change/gate0.rs:93` 至 `src/change/gate0.rs:101`；用例 `src/change/gate0.rs:293` |
| 无 git 元数据目录 | 初始化仓库 → 暂存全部 → 提交 → 记 HEAD | 是 | `src/change/gate0.rs:105` 至 `src/change/gate0.rs:111`；用例 `src/change/gate0.rs:253` |

- 顺序是**先查记录文件、后看 git**：续跑时 HEAD 已因「按步提交」前进，若重记会把已提交的步从门禁视野里抹掉（自锁）。动机写在函数注释里（`src/change/gate0.rs:59` 至 `src/change/gate0.rs:65`），并由用例锁住（🟢 CONFIRMED `src/change/gate0.rs:222`）。
- 基线提交用固定身份 devforge / devforge@localhost（以 `-c` 传参），不依赖机器全局 git 配置（`src/change/gate0.rs:114` 至 `src/change/gate0.rs:140`）；提交带 `--allow-empty`，空目录也能成（🟢 CONFIRMED `src/change/gate0.rs:125`）。

**文档档位（D002）**（🟢 CONFIRMED `src/change/gate0.rs:24` 至 `src/change/gate0.rs:56`）

- 三档：none（默认）/ essential / complete；解析大小写不敏感、未知值硬报错。同名枚举在 `src/docgen/audit.rs:14` 另有定义且语义不同（那边未知值回落默认档），差异登记在 `_meta/gaps.md:63`。
- 档位是**调用方的决定**：引擎不探测目标仓里有没有文档（`src/change/gate0.rs:6` 至 `src/change/gate0.rs:7`）。
- 默认值的实际落点在服务层：环境变量未设置时取字符串 none（🟢 CONFIRMED `src/server/change_engine.rs:101`）。

**错误面**（🟢 CONFIRMED `src/change/gate0.rs:67` 至 `src/change/gate0.rs:71`、`src/change/gate0.rs:131` 至 `src/change/gate0.rs:139`）

- `DevforgeError` 的 `Config` 变体：目标仓目录不存在（`src/change/gate0.rs:67`）、未知档位（`src/change/gate0.rs:39`）。
- `DevforgeError` 的 `System` 变体：git 子进程启动失败或非零退出（`src/change/gate0.rs:131`、`src/change/gate0.rs:132`、`src/change/gate0.rs:160`、`src/change/gate0.rs:161`）、建目录或写记录文件失败（`src/change/gate0.rs:174` 至 `src/change/gate0.rs:179`）、基线提交后仍取不到 HEAD（`src/change/gate0.rs:138`）。

---

### Baseline

- **职责**：门 0 的结论值对象——sha 是后续 diff / 回滚的对照点，第二个字段标记本次是否新建了仓库。不做校验、不带方法。
- **构造 / 初始化**：由 `ensure_baseline` 返回；两个字段都是 `pub`，也可直接构造（测试与 `ChangePrep` 都用字段访问）。派生 `Debug, Clone, PartialEq, Eq`（🟢 CONFIRMED `src/change/gate0.rs:16`）。
- **公开成员**：
  - `sha`（`String`）— 基线提交的完整哈希（`git rev-parse HEAD` 输出经 trim）；生产路径只读。
  - `created_repo`（`bool`）— 仅「无 git 元数据目录 → 初始化仓库」这一条分支为 true（`src/change/gate0.rs:110`）；空仓补提交仍为 false（🟢 CONFIRMED `src/change/gate0.rs:99`）。
- **生命周期**：值对象，随 `ensure_baseline` 返回即固定；持久化形态只有 sha 一行（文件名见 `BASELINE_FILE`），`created_repo` 不落盘，只用于本次调用的日志/上报（`src/server/change_engine.rs:116` 附近记录档位与基线）。
- **典型用法**：`prepare` 取到后放进 `ChangePrep` 的 `baseline` 字段（`src/change/prepare.rs:21`），服务层再把 sha 抄进 `ChangeCtx` 的 `baseline` 字段（`src/server/change_engine.rs:199`、`src/server/change_engine.rs:252`），各门用它做 diff 起点。

### DocLevel

- **职责**：表示文档完整度要求的三个档位；只做表示与字符串互转，不执行任何文档动作。
- **构造 / 初始化**：`parse` 从字符串解析（推荐路径）；枚举变体也可直接写（集成测试全部直接写 None 变体，`tests/change_ask_ring.rs:105`）。派生 `Debug, Clone, Copy, PartialEq, Eq`（🟢 CONFIRMED `src/change/gate0.rs:23`）。
- **公开成员（变体）**：
  - `None` — 不碰文档（默认档）。`src/change/gate0.rs:26`
  - `Essential` — 轻量：清单 + 入口 + Runbook 级覆盖。`src/change/gate0.rs:28`
  - `Complete` — 完整：整条文档链，等价 doc-from-zero complete；引擎不代跑，`prepare` 会直接报 `Quality` 错误要求先跑文档链（`src/change/prepare.rs:59` 至 `src/change/prepare.rs:61`）。`src/change/gate0.rs:30`
- **生命周期**：`Copy` 值对象，跨步骤按值传递（从 `ChangePrep` 的 `doc_level` 字段到 `ChangeCtx` 的同名字段）。
- **典型用法**：`prepare` 解析一次存进 `ChangePrep`（`src/change/prepare.rs:45`），传给 `apply` 决定文档动作（`src/change/docs.rs:31`），并在 plan 阶段把档位原文写进给 agent 的指令（`src/change/session.rs:83`）。

### DocLevel 的 parse

- **摘要**：把档位字符串解析成 `DocLevel`，大小写不敏感、空串视为默认。
- **行为语义**（🟢 CONFIRMED `src/change/gate0.rs:34` 至 `src/change/gate0.rs:43`）：先 trim 再转小写，然后精确匹配：空串与 none → `None`，essential → `Essential`，complete → `Complete`，其余一律 `Err`（`Config` 变体）。幂等、无副作用、不读环境、不读文件。
- **参数**：`s: &str` — 档位原文；允许空串（等价 none），允许前后空白与大小写混写（如 Essential）。
- **返回**：`Result<DocLevel>`。
- **异常**：`DevforgeError` 的 `Config` 变体 — 值不在三档之内（如 full、detailed）；文案形如「未知 doc_level `full`（只认 none/essential/complete）」（`src/change/gate0.rs:39` 至 `src/change/gate0.rs:41`）。调用方应把这类输入当非法配置拒收，而不是回落默认（`src/change/prepare_tests.rs` 的用例 `bad_doc_level_is_rejected` 断言的正是这条）。
- **示例**（🟢 CONFIRMED `src/change/gate0.rs:314` 至 `src/change/gate0.rs:322`）：空串 → None；none → None；Essential → Essential；complete → Complete；full → Err。

### DocLevel 的 as_str

- **摘要**：档位转回规范小写字符串（与 `parse` 互逆）。
- **行为语义**（🟢 CONFIRMED `src/change/gate0.rs:45` 至 `src/change/gate0.rs:51`）：三重穷举 match，输出 none / essential / complete；无失败路径、无分配。幂等、无副作用。
- **参数**：N/A —— 只作用于 `&self`，无入参。
- **返回**：`&'static str`（静态字面量）。
- **异常**：N/A —— 三变体穷举，没有 `Err` 分支。
- **示例**：`Complete` 档转字符串得 complete；生产消费点把结果直接插进 plan 指令文本（`src/change/session.rs:83`），并写入日志字段（`src/server/change_engine.rs:116`）。

### DocLevel 的 needs_docs

- **摘要**：该档位是否要求先跑文档阶段。
- **行为语义**（🟢 CONFIRMED `src/change/gate0.rs:54` 至 `src/change/gate0.rs:56`）：对 None 取反——只有 None 为 false。纯判断、幂等、无副作用。
- **参数**：N/A —— 只作用于 `&self`。
- **返回**：`bool`。
- **异常**：N/A —— 无失败路径。
- **示例**：None 档为 false，Essential 与 Complete 为 true（🟢 CONFIRMED `src/change/gate0.rs:320` 至 `src/change/gate0.rs:321`）。

### ensure_baseline

- **摘要**：确保目标仓有基线锚点，并把 sha 写入计划目录下的记录文件；幂等——已有记录就直接复用。
- **行为语义**（🟢 CONFIRMED `src/change/gate0.rs:66` 至 `src/change/gate0.rs:112`）：
  1. `repo` 不是目录 → 立即 `Err`（`Config`），不做任何写入。
  2. 记录文件可读且 trim 后非空 → 返回记录里的 sha、`created_repo=false`（**不重记当前 HEAD**）。
  3. 有 git 元数据目录：HEAD 可解析 → 记 HEAD；HEAD 不可解析（空仓）→ `create_baseline_commit` 补一条，再记。
  4. 无 git 元数据目录：初始化仓库（`src/change/gate0.rs:105`）→ `create_baseline_commit` → 记，`created_repo=true`。
  5. 写记录文件前先建目录，内容格式为「sha 加一个换行」。
  - 副作用：可能新建 git 仓库、新增提交、创建目录与文件；**不会**改写既有提交或删除文件。
- **参数**：
  - `repo: &Path` — 目标仓库根，必须已存在（服务层把任务 workspace 传进来，`src/server/change_engine.rs:105`；`prepare` 在 `src/change/prepare.rs:65` 转发给本函数）。
  - `out_dir: &Path` — 记录文件所在目录；生产调用方传的是目标仓内的计划目录（`src/change/prepare.rs:52`、`src/change/prepare.rs:65`），测试里也用它当临时输出目录（`src/change/gate0.rs:307`）。
- **返回**：`Result<Baseline>` —— 已记录到盘的 sha（与记录文件内容一致，`src/change/gate0.rs:253` 的用例断言二者相等）。
- **异常**：
  - `DevforgeError` 的 `Config` 变体：`repo` 不存在（文案含「不存在」，`src/change/gate0.rs:68` 至 `src/change/gate0.rs:71`）。
  - `DevforgeError` 的 `System` 变体：初始化 / 暂存 / 提交三条 git 命令启动失败或非零退出（stderr 拼进文案）、建目录失败、写记录文件失败、基线提交后取不到 HEAD。空仓是**正常分支**，不会以错误形式冒出来（`head_sha` 用 `Option` 表达）。
- **示例**（5 条都可复现，逐条对应测试）：
  - 无 git 元数据目录 → `created_repo=true`，仓库里恰好一条 devforge: baseline 提交（`src/change/gate0.rs:253`）。
  - 已有提交的仓库 → sha 等于调用时 HEAD，提交数不变（`src/change/gate0.rs:275`）。
  - 有 git 元数据目录但空仓 → 补一条基线提交，`created_repo=false`（`src/change/gate0.rs:293`）。
  - 记录文件已存在 + HEAD 已前进 → 返回旧 sha（`src/change/gate0.rs:222`）。
  - 目录不存在 → `Config` 错误，文案含「不存在」（`src/change/gate0.rs:307`）。

## 配置项：DEVFORGE_DOC_LEVEL

| 项 | 值 |
|---|---|
| 名称 | DEVFORGE_DOC_LEVEL |
| 类型 | 字符串（环境变量） |
| 默认值 | 字符串 none（未设置或读取失败时，🟢 CONFIRMED `src/server/change_engine.rs:101`） |
| 是否必填 | 否 |
| 取值范围 | none / essential / complete（大小写不敏感，前后空白被 trim；其它值 → `Config` 错误，`src/change/gate0.rs:34`） |
| 影响 | 决定 `prepare` 分派哪条文档动作：none 不建 docs 目录、essential 引擎写清单与词表、complete 直接要求先跑 doc-from-zero（否则 `Quality` 错误，`src/change/prepare.rs:59`） |
| 示例 | 设成 essential 时，门 0 之后引擎自建文档清单（🟢 CONFIRMED `src/change/docs.rs:31`） |

该变量只被服务层读取并作为字符串下传（`src/server/change_engine.rs:101`、`src/server/change_engine.rs:109`）；本模块自身不读环境变量。

## 内部符号

**`BASELINE_FILE`**（`src/change/gate0.rs:171`）

- 摘要：记录文件名常量，取值为 baseline.txt（相对 `out_dir`）。
- 行为语义：`write_baseline` 用它拼写入路径；`ensure_baseline` 用它拼读取路径做幂等判断。私有常量，不对外导出。

**`create_baseline_commit`**（`src/change/gate0.rs:114`）

- 摘要：暂存全部改动后以固定身份提交 devforge: baseline，返回新提交的 HEAD。
- 行为语义：暂存失败即 `System`（`src/change/gate0.rs:115`）；提交命令带固定 user.name / user.email、`-q`、`--allow-empty` 与固定提交信息（`src/change/gate0.rs:117` 至 `src/change/gate0.rs:128`）；提交失败把 stderr 拼进 `System`（`src/change/gate0.rs:132` 至 `src/change/gate0.rs:137`）；提交后仍取不到 HEAD 也报 `System`（`src/change/gate0.rs:138` 至 `src/change/gate0.rs:139`）。无幂等保证——调用方负责只在「需要建基线」的分支调用它。

**`head_sha`**（`src/change/gate0.rs:142`）

- 摘要：读当前 HEAD，成功且非空才返回 `Some`。
- 行为语义（`src/change/gate0.rs:142` 至 `src/change/gate0.rs:153`）：进程启动失败、非零退出、输出空白 → `None`。用 `Option` 而不是 `Result`，因为「空仓 / 无提交」是预期状态而非异常——`ensure_baseline` 的空仓分支依赖这一点（`src/change/gate0.rs:93` 至 `src/change/gate0.rs:101`）。

**`run_git`**（`src/change/gate0.rs:155`）

- 摘要：在给定目录下跑一条 git 子命令，失败转 `System`。
- 行为语义（`src/change/gate0.rs:155` 至 `src/change/gate0.rs:168`）：把参数原样拼进错误文案；启动失败与非零退出（带 stderr）分别成两条错误信息；成功时丢弃标准输出。用于初始化与暂存两步。

**`write_baseline`**（`src/change/gate0.rs:173`）

- 摘要：建目录并写入「sha 加换行」。
- 行为语义（`src/change/gate0.rs:173` 至 `src/change/gate0.rs:181`）：建目录幂等；写失败报 `System` 且文案带完整路径。覆盖写，不追加。

## 测试覆盖（`#[cfg(test)]`，6 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `existing_baseline_record_is_reused_even_after_head_advances` | 记录文件存在 → 复用旧 sha；HEAD 前进也不重记 | `src/change/gate0.rs:222` |
| `bare_dir_gets_init_plus_single_baseline_commit` | 无 git 元数据目录 → `created_repo=true`、恰好一条基线提交、记录文件等于该 sha | `src/change/gate0.rs:253` |
| `existing_repo_only_records_head_and_never_moves_it` | 已有提交的仓库只记 HEAD，提交计数不变 | `src/change/gate0.rs:275` |
| `repo_with_git_but_no_commit_gets_baseline_commit` | 空仓补一条基线提交，`created_repo=false` | `src/change/gate0.rs:293` |
| `missing_repo_is_config_error` | 目标仓缺失 → `Config`，文案含「不存在」 | `src/change/gate0.rs:307` |
| `doc_level_parses_three_tiers_and_defaults_to_none` | 三档解析 + 空串默认 + 未知值报错 + `needs_docs` | `src/change/gate0.rs:314` |

测试模块自身在 `src/change/gate0.rs:183` 起；辅助函数 `git`（断言命令成功并取标准输出，`src/change/gate0.rs:187`）与 `commit_all`（用固定身份 t / t@localhost 提交，`src/change/gate0.rs:201`）用于模拟「coding agent 按步提交」。

## 测试运行与环境

- **工具链前提**：本 crate 用 edition 2024，需 Rust 1.85+。若机器有多个 toolchain 且未设默认值，显式指定：
  ```bash
  RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable cargo --version
  ```
- **依赖安装**：`tempfile` 是 dev-dependency（`Cargo.toml:27`），跑测试时自动获取；6 例中的 4 例会起 git 子进程，PATH 上必须有 git（本单元环境为 git 2.53.0）。无需网络、端口、外部服务。
- **隔离**：测试一律用 `tempfile` 的 `tempdir` 建临时目录，git 仓库也在临时目录里，提交身份用 `-c` 显式给，不读写用户全局 git 配置；因此可并行，也能在只读的仓库目录下跑。
- **命令**（🟢 CONFIRMED，本单元实跑：`cargo test --lib change::gate0`）：
  ```bash
  CARGO_TARGET_DIR=$(mktemp -d)/target cargo test --lib change::gate0
  ```
  结果：6 passed、0 failed、592 filtered out，退出码 0。构建产物写在仓外临时目录，仓内 target 目录未被写入，符合「目标仓只读」约束。
- **只跑单例**：`cargo test --lib change::gate0::tests::missing_repo_is_config_error -- --exact`。
- **不适用声明**：无服务、无端口、无健康检查（N/A —— 库内模块，只有函数调用）；无种子数据（N/A —— 测试自建临时仓库）；无运行时配置键需在启动前给（本模块不读环境变量，档位字符串由服务层传入，见上文「配置项」）；公开接口清单即本文「公开成员」表（N/A —— 无 HTTP/CLI 端点）。

## 相关文档

- 同单元：[src/change/history.rs](./history.rs.md)（往返历史）
- 上下游：[src/change/prepare.rs](./prepare.rs.md)（调用方）、[src/change/docs.rs](./docs.rs.md)（档位 → 文档动作）、[src/change/session.rs](./session.rs.md)（`ChangeCtx` 携带档位）、[src/change/boundary.rs](./boundary.rs.md)（`ImpactGap` 与往返上限）
- 缺口：`_meta/gaps.md`（同名 `DocLevel` 语义差异，`_meta/gaps.md:63`）
