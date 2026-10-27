# 勘察事实 — unit-src-02（src/change/change_id.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/change_id.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/change_id.rs`。
- 模块在 `src/change/mod.rs` 声明（submodule `change_id`），并经 `src/change/mod.rs:28` `pub use change_id::{assign_change_id, change_root, make_change_id, slugify, validate_change_id}` 转发导出。
- 文件头模块注释：`change-id` 与黑板目录（P009 T003 / D008）。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `change_root` | `#15` | `pub fn(&Path,&str,&str) -> PathBuf`：`plans/<p>/<t>/change` |
| `current_marker` | `#20` | `pub fn(&Path,&str,&str) -> PathBuf`：change 根下 `current` |
| `slugify` | `#25` | `pub fn(&str) -> String`：小写、只留 `[a-z0-9]`、其余折 `-`、取前 40 |
| `make_change_id` | `#48` | `pub fn(&str,&str) -> String`：`change-{date}-{slug}` |
| `assign_change_id` | `#55` | `pub fn(...) -> Result<(String,PathBuf)>`：取/分配 id + 建目录，幂等 |
| `date_segment` | `#87` | `pub fn(i32,u32,u32) -> String`：`{year:04}{month:02}{day:02}` |
| `date_segment_now` | `#94` | `pub fn() -> String`：本地时钟 / `DEVFORGE_TODAY` 覆盖 |
| `validate_change_id` | `#132` | `pub fn(&str) -> Result<()>`：`change-` 前缀 + 小写/数字/`-` |

## 配置项

| 名称 | 位置 | 信息 |
|---|---|---|
| `DEVFORGE_TODAY` | `#95`（读取点） | 环境变量，形如 `YYYY-MM-DD`；`date_segment_now` 解析通过则覆盖日期段；解析失败回退本地时钟。 |

## 内部符号

- `civil_from_days`（`#118`）：`fn(i64) -> (i32,u32,u32)`，Howard Hinnant 的 civil_from_days；`date_segment_now` 用。非 pub。

## 行为/测试事实（`#[cfg(test)]`，9 例）

- `slugify_normalizes_and_bounds`（`#149`）：`"Add Book API"`→`"add-book-api"`；`"修复：分页越界!!"`→`"change"`；`"  __a__b__  "`→`"a-b"`；100 字符 slug 截到 40。
- `same_task_reuses_change_id`（`#157`）：同一 `p/t1`、不同 wording/日期 → id 与目录相同（幂等），id=`change-20261020-add-book-api`。
- `different_tasks_get_different_ids_and_dirs`（`#171`）：`p/t1` 与 `p/t2` 同 wording → id 文本可相同但 `change_root` 目录隔离。
- `path_shape_is_plans_project_task_change_id`（`#184`）：目录 = `<plans>/<project>/<task>/change/<change-id>`。
- `marker_survives_deleted_dir_without_renumbering`（`#197`）：目录被 `remove_dir_all` 清掉后，再 `assign` 复用 id（不换号）并重建目录。
- `date_segment_is_zero_padded`（`#207`）：`date_segment(2026,10,5)`→`"20261005"`。
- `civil_date_conversion_matches_known_dates`（`#212`）：`civil_from_days(0)`→`(1970,1,1)`；`18262`→`(2020,1,1)`；`-1`→`(1969,12,31)`。
- `env_override_wins_for_today`（`#219`）：只验 `date_segment(2026,10,20)`→`"20261020"`（测试不并行改全局 env）。
- `validate_rejects_weird_ids`（`#225`）：`"change-20261020-x"` ok；`"Change-20261020-x"` err（Config）；`"../evil"` err（Config）。

## 口径（代码内可见规则）

- id 形状：`change-<YYYYMMDD>-<slug>`。
- 幂等：`<change_root>/current` 记当前 id；存在且目录在 → 复用；存在但目录被清 → 复用 id 并重建目录；不存在 → 新分配。
- 目录形状：`<plans_root>/<project>/<task>/change/<change-id>/`；检查点按 010 D001 另住 `<plans_root>/<project>/<task>/change/state.json` 同层。

## crate 内调用点（消费者）

- `src/change/prepare.rs:8` 引入 `{assign_change_id, date_segment}`；`#49` 调 `assign_change_id`；`#50` 调 `change_root`。
- `src/change/mod.rs:28` `pub use` 转发。
- `src/server/change_engine.rs:260` 调 `date_segment_now`（注释提及 `DEVFORGE_TODAY` 覆盖）。

## 外部依赖

- 错误面：`crate::error::{Context, DevforgeError, Result}`（`#8`）。`assign_change_id` 的 IO 错误用 `Context`（`.ctx()`）包成 `crate::error::Result`；`validate_change_id` 错误为 `DevforgeError::Config`。
- 日期：不引第三方时间库，自实现 `civil_from_days`。
- 无其他外部 crate 依赖。
