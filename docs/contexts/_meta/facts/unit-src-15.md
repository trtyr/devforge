# 勘察事实 — unit-src-15（src/change/prepare.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/prepare.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/prepare.rs`（251 行）。
- 模块声明：`src/change/mod.rs:18` `pub mod prepare;`；`src/change/mod.rs:37` `pub use prepare::{ChangePrep, prepare as prepare_change};`（仓内无别名调用点）。
- 文件头注释：change run 的门 0 预检（P009 接线）——一次把「基线 / 文档档位 / change-id / 黑板目录」备好；顺序有依赖：change-id → 计划目录 → baseline.txt 落点 → diff 锚点；全确定性、可单测。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `ChangePrep` | `#15` | `#[derive(Debug,Clone,PartialEq,Eq)] pub struct`：字段 `change_id:String`、`blackboard:PathBuf`、`change_dir:PathBuf`、`baseline:Baseline`、`doc_level:DocLevel`、`doc_action:DocAction` |
| `prepare` | `#30` | `pub fn(&Path,&Path,&str,&str,&str,&str,(i32,u32,u32)) -> Result<ChangePrep>` |

## `prepare` 内部步骤（按源码顺序）

1. `repo.is_dir()` 为假 → `Config("目标仓不存在：{repo}")`。
2. `DocLevel::parse(doc_level)?`（未知值 → `Config`，见 `src/change/gate0.rs:34`）。
3. `date_segment(today.0, today.1, today.2)` → `YYYYMMDD`（`src/change/change_id.rs:87`）。
4. `assign_change_id(plans_root, project, task, requirement, &date)?` → `(change_id, id_dir)`，`id_dir` 被丢弃（`_id_dir`）；注意 `id_dir` = `change_root/<id>`（黑板内子目录，`src/change/change_id.rs:76`-78 由 `assign_change_id` 自建），与 marker 文件（`change_root/current`，`:20`-22）配套；marker 在但目录被删时按旧 id 重建不换号（`:62`-73）。
5. `blackboard = change_root(plans_root, project, task)` → `plans/<project>/<task>/change`。
6. `plan_dir = repo.join("docs/plantree/changes").join(&change_id)`；`create_dir_all` 失败 → `System`。
7. `doc_action = docs::apply(repo, level)?`；若为 `FullChainRequired` → `Quality("doc_level=complete 需要先跑 doc-from-zero …")`。
8. `baseline = ensure_baseline(repo, &plan_dir)?`（`baseline.txt` 落计划目录）：`src/change/gate0.rs:66`-111 四种情形——① `baseline.txt` 非空 → 复用（`created_repo=false`）；② 有 git 元数据目录且有 HEAD → 只记 HEAD；③ 有 git 元数据目录但无提交 → 补基线提交；④ 无 git 元数据目录 → `git init` + 基线提交（`created_repo=true`）。
9. 建黑板子目录 `evidence`、`testing/reports`、`coding/reports`（失败 → `System`）。
10. 返回 `ChangePrep { change_id, blackboard, change_dir: plan_dir, baseline, doc_level: level, doc_action }`。

## 行为/测试事实（`#[cfg(test)]`，7 例）

- `bare_repo_gets_baseline_and_blackboard_skeleton`（`#96`）：无 `.git` 仓 + date `(2026,10,20)` + 需求 `add book api` → `change_id == change-20261020-add-book-api`；`change_dir` 在目标仓、不等于黑板；`baseline.txt` 在计划目录且内容 = `baseline.sha`；`created_repo=true`；三个黑板子目录存在；黑板以 `proj/task1/change` 结尾。
- `none_level_does_not_touch_docs`（`#152`）：`none` → 目标仓无 `docs/contexts`。
- `essential_level_emits_inventory_then_proceeds`（`#169`）：`doc_action == EssentialDone` 且 `docs/contexts/_meta/inventory.json` 存在。
- `complete_level_is_refused_with_reason`（`#187`）：`complete` → `Err`，文案含 `doc-from-zero`。
- `repeated_prepare_reuses_change_id`（`#204`）：同 task 两次（需求文本不同、日期不同）→ `change_id`/`change_dir` 相同；第二次 `created_repo=false` 且 sha 不变。
- `missing_repo_is_rejected`（`#220`）：目标仓不存在 → `Err` 含「不存在」。
- `bad_doc_level_is_rejected`（`#236`）：`full` → `Err` 含 `doc_level`。

## crate 内调用点（消费者）

- `src/server/change_engine.rs:10` `use crate::change::prepare::prepare`；`:103` 生产调用点（`:101` 从环境变量 `DEVFORGE_DOC_LEVEL` 取档位、缺省 `none`；`:102` `today_parts()`）；`:246` `build_ctx` 接收 `&ChangePrep`；`:317` 为单测内调用（`:279` 起是 `#[cfg(test)]`）。
- `src/change/mod.rs:18/37` 声明与转发（别名 `prepare_change` 仓内无引用）。

## 事实补充（2026-10-21 复核）

- `impl_steps_path`（`src/change/coding_gate.rs:322`-325）：`grep -rn impl_steps_path src/` 只命中定义本身 → 仓内**无调用点**。impl-steps.md 属黑板的旁证：`src/change/session.rs:33`、`:151`、`:976`。
- `report.md` 落点：`write_report` 写 `repo/docs/plantree/changes/<id>/report.md`（`src/change/report.rs:139`-143）。
- `plan_path` = `change_dir/change-plan.md`（`src/change/session.rs:45`-47）；`acceptance_path` = `blackboard/testing/acceptance.md`（`:49`-51）。

## 可重放命令（2026-10-21 实跑）

- `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=<仓外独立目录> cargo test --lib change::prepare` → `7 passed; 0 failed; 593 filtered out`，EXIT=0；`rustc 1.99.0`；edition 2024（`Cargo.toml:5`）。

## 外部依赖

- `crate::change::change_id::{assign_change_id, date_segment}`（`:8`）、`crate::change::change_id::change_root`（`:50` 模块路径调用，未 import）；`crate::change::docs::{self, DocAction}`（`:9`）；`crate::change::gate0::{ensure_baseline, Baseline, DocLevel}`（`:10`）；`crate::error::{DevforgeError, Result}`（`:11`）。
- `std::path::{Path, PathBuf}`；无外部 crate；测试用 `tempfile`。

## 复核（本单元重写，2026-10-24）

- 行号复核：`ChangePrep` `:15`（字段 `:16`/`:18`/`:20`/`:21`/`:22`/`:24`）、`prepare` `:30`、测试模块 `:84`、夹具 `:88`、7 例 `:96`/`:152`/`:169`/`:187`/`:204`/`:220`/`:236`——与清单 10 条逐条对齐。
- 生产调用点复核：`src/server/change_engine.rs:101`（`DEVFORGE_DOC_LEVEL`，缺省 `none`）、`:102`（`today_parts()`）、`:103`（`prepare(` 调用跨到 `:111`）、`:246` 至 `:254`（`build_ctx` 搬字段）、`:317`（单测）；`src/change/mod.rs:18`（`pub mod prepare;`）、`:37`（别名 `prepare_change`，仓内无引用）。
- 实跑：`cargo test --lib change::prepare` → **7 passed; 0 failed; 591 filtered out**，EXIT=0（`CARGO_TARGET_DIR` 指向仓外临时目录，仓内 `target/` 未被写）。
- 门禁：`devforge doc-lint docs/contexts --repo .` → **EXIT=0，doc-lint: 通过（0 违规）**。
- 清单覆盖：10/10（module×1 + symbol×2 + behavior×7；7 个 behavior 由测试覆盖表逐例承接）。
