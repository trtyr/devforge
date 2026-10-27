# 勘察事实 — unit-src-08（src/change/gate0.rs + src/change/history.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/gate0.rs.md`、`docs/contexts/src/change/history.rs.md` 与 `_meta/gaps.md`。

## 文件一：src/change/gate0.rs

- 模块声明：`src/change/mod.rs:14` `pub mod gate0;`；`src/change/mod.rs:31` `pub use gate0::{Baseline, DocLevel, ensure_baseline};`。
- 文件头注释：门 0（P009）——基线、文档档位、change-id 的确定性部分。

### 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `Baseline` | `#17` | `#[derive(Debug,Clone,PartialEq,Eq)] pub struct { pub sha:String, pub created_repo:bool }` |
| `DocLevel` | `#24` | `#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub enum { None/Essential/Complete }` |
| `DocLevel::parse` | `#34` | `pub fn(&str) -> Result<Self>` |
| `DocLevel::as_str` | `#45` | `pub fn(&self) -> &'static str` |
| `DocLevel::needs_docs` | `#54` | `pub fn(&self) -> bool` |
| `ensure_baseline` | `#66` | `pub fn(&Path,&Path) -> Result<Baseline>` |

### 内部符号

- `BASELINE_FILE`（const `&str` = `"baseline.txt"`，相对计划目录）。
- `create_baseline_commit`（`fn(&Path)->Result<String>`）：`git add -A` + 固定身份提交 `devforge: baseline` + 取 HEAD。
- `head_sha`（`fn(&Path)->Option<String>`）：`git rev-parse HEAD`，可解析才返回。
- `run_git`（`fn(&Path,&[&str])->Result<()>`）：执行 git 并判 status。
- `write_baseline`（`fn(&Path,&str)->Result<()>`）：建目录 + 写 `baseline.txt` 为 `"{sha}\n"`。

### 行为/测试事实（`#[cfg(test)]`，6 例）

- `existing_baseline_record_is_reused_even_after_head_advances`（`#222`）：已记录 `baseline.txt` 非空 → 续跑复用已记录 sha，不重记当前 HEAD（HEAD 已因按步提交前进）。
- `bare_dir_gets_init_plus_single_baseline_commit`（`#253`）：无 `.git` → `git init` + 恰好一条提交 `devforge: baseline`，`baseline.txt` 记该 sha。
- `existing_repo_only_records_head_and_never_moves_it`（`#275`）：已有 `.git`（有提交）→ 记当时 HEAD，不新增提交、不动历史。
- `repo_with_git_but_no_commit_gets_baseline_commit`（`#293`）：有 `.git` 但空仓 → 补一条基线提交，`created_repo=false`。
- `missing_repo_is_config_error`（`#307`）：目标仓目录不存在 → `Config` 错误（文案含「不存在」）。
- `doc_level_parses_three_tiers_and_defaults_to_none`（`#314`）：`""`/`none`→None；`Essential`→Essential；`complete`→Complete；`full`→Err；`None.needs_docs()`=false，Essential/Complete 为 true。

### 口径（代码内可见规则）

- 基线（D001）：无 `.git` → init + 全量提交 `devforge: baseline`（固定身份 `devforge`/`devforge@localhost`，不依赖机器 git 配置）；已有 `.git` → 只记 HEAD，不动历史；HEAD 不可解析（空仓）→ 补基线提交；`baseline.txt` 已存在非空 → 直接复用（基线 = 变更起点不可变锚点，续跑不得把基线顶成当前 HEAD）。
- doc_level（D002）：`none`（默认）/`essential`/`complete`，解析大小写不敏感，未知值 → `Config` 错误。
- 错误面：`DevforgeError`：目录缺失/未知档位 → `Config`；git 命令启动失败/非零退出/建目录写文件失败 → `System`。
- 依赖：`crate::error::{DevforgeError, Result}`；`std::process::Command`（git）；`std::path`。

## 文件二：src/change/history.rs

- 模块声明：`src/change/mod.rs:15` `pub mod history;`；`src/change/mod.rs:32` `pub use history::{append_roundtrip, history_path};`。
- 文件头注释：往返历史（P009 T013）——每次 coding→plan 回退落一行到 `docs/plantree/changes/<change-id>/history.md`。

### 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `history_path` | `#10` | `pub fn(&Path,&str) -> PathBuf` |
| `append_roundtrip` | `#17` | `pub fn(&Path,&str,usize,&[ImpactGap]) -> Result<PathBuf>` |

### 行为/测试事实（`#[cfg(test)]`，3 例）

- `first_roundtrip_creates_history_with_header`（`#57`）：首次 → 建文件带表头 `# 变更往返历史 · <id>` + 表格（往返/越界文件/理由/证据）+ 该行。
- `later_roundtrips_append_without_losing_history`（`#68`）：后续 → 追加行，表头只出现一次，`a.rs`/`b.rs` 都保留。
- `multiple_gaps_in_one_roundtrip_all_recorded`（`#79`）：同一次往返多个 gaps → 每 gap 一行，`| 1 |` 出现次数 = gap 数。

### 口径（代码内可见规则）

- 历史文件：`<repo>/docs/plantree/changes/<change-id>/history.md`。
- 表头：`# 变更往返历史 · <change-id>` + 表头行 `| 往返 | 越界文件 | 理由 | 证据 |`。
- 追加格式：每 gap 一行 `| <往返> | `file` | <reason> | <evidence> |`。
- 父目录用 `expect`（文案「history 必有父目录」）；IO 错误包 `System`（建目录/写）。
- 依赖：`crate::change::boundary::ImpactGap`（`src/change/boundary.rs:17`）；`crate::error::{DevforgeError, Result}`；`std::path`。

## crate 内调用点（消费者）

- `src/change/prepare.rs:10` `use crate::change::gate0::{ensure_baseline, Baseline, DocLevel}`；`:22` `pub doc_level: DocLevel`；`:45` `DocLevel::parse(doc_level)?`；`:65` `ensure_baseline(repo, &plan_dir)?`。
- `src/change/session.rs:16` `use crate::change::gate0::DocLevel`；`:41` `pub doc_level: DocLevel`；`:749` `DocLevel::None`。
- `src/change/orchestrator.rs:246` `crate::change::history::append_roundtrip(...)`；`:681` `crate::change::history::history_path(...)`。
- `src/change/mod.rs:14/15/31/32` 声明与转发导出。

## 外部依赖

- 生产：`crate::change::boundary::ImpactGap`、`crate::error`；`std::process::Command`（git）、`std::path`。测试：`tempfile`。

## 复核（本次整单元重写，2026-10-24）

### 清单归属

`_meta/inventory.json` 当前把 `unit-src-08` 指到 **src/change/gate0.rs（13 条）+ src/change/history.rs（6 条）**，与 `_meta/state.json` 里旧一轮记录的 `mod.rs.md`/`orchestrator.rs.md` 不同——以 inventory 为准，本单元只产出这两个文件的模块文档。

### 行号复核（逐条 grep 对过当前树）

- gate0：`Baseline` `:17`、`DocLevel` `:24`（变体 `:26`/`:28`/`:30`）、`parse` `:34`、`as_str` `:45`、`needs_docs` `:54`、`ensure_baseline` `:66`、`create_baseline_commit` `:114`、`head_sha` `:142`、`run_git` `:155`、`BASELINE_FILE` `:171`、`write_baseline` `:173`、测试模块 `:183`、辅助 `git` `:187` / `commit_all` `:201`、6 个用例 `:222`/`:253`/`:275`/`:293`/`:307`/`:314`。
- history：`history_path` `:10`、`append_roundtrip` `:17`、测试模块 `:44`、夹具 `gap` `:48`、3 个用例 `:57`/`:68`/`:79`。
- 消费方：`mod.rs:14`/`:15`/`:31`/`:32`；`prepare.rs:10`/`:21`/`:22`/`:45`/`:52`/`:65`；`docs.rs:12`/`:31`/`:33`；`session.rs:16`/`:41`/`:83`；`server/change_engine.rs:101`/`:109`/`:115`/`:116`/`:199`/`:252`；`orchestrator.rs:243`/`:244`/`:246`；`orchestrator_tests.rs:288`；`boundary.rs:13`（`MAX_ROUNDTRIPS`=3）/`:17`/`:159`。

### 测试实跑（仓外 CARGO_TARGET_DIR，仓库目录未被写）

- `cargo test --lib change::gate0` → **6 passed; 0 failed; 592 filtered out**，EXIT=0。
- `cargo test --lib change::history` → **3 passed; 0 failed; 595 filtered out**，EXIT=0。
- 工具链：需显式 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable`（默认 toolchain 未配置），cargo 1.99.0；git 2.53.0。

### 门禁

- `devforge doc-lint docs/contexts --repo .` → 当时退出码 1，全库 16 条失败，**均不在本单元两篇文档**（`src/change/mod.rs.md`、`orchestrator.rs.md`、`session.rs.md`、`state.rs.md`、`testing_gate.rs.md`、`src/config/installer.rs.md`、`src/config/mod.rs.md`，原因都是已迁移的测试路径被写成 `tests/change_failure_context.rs` 这类旧文件形式）。
  - **后续（round 门禁回修）**：这 16 条已按 `PATH_MIGRATIONS` 全量重定位（`tests/change_failure_context/main.rs`、`tests/f6_observability/fixture.rs`、`src/server/tests/mod.rs`、`tests/change_review_protocol/main.rs`），复跑 `doc-lint` → **退出码 0（0 违规）**。
- 只看本单元两篇：0 违规（按文件过滤 lint 输出无 gate0/history 条目）。

### 重写依据（上一版文档的问题）

上一版（review-7 PASS 的稿）在 corpus 层被两条隐性漂移带红：①`DocLevel::parse` / `ChangeCtx.doc_level` 这类「点/双冒号路径」写法被实体检查判为幽灵实体；②`out_dir/baseline.txt`、`repo/.git` 被判为幽灵路径。本次改为「类型名 + 字段名分写」与「文件名不加反引号」，并把两处测试命令从 🟡 升级为 🟢（实跑复现），补了内部 helper 的摘要+行为语义与 `DEVFORGE_DOC_LEVEL` 配置项条目。
