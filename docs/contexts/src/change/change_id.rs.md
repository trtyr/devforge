---
type: module
module_path: src/change/change_id.rs
unit: unit-src-02
description: change-id 生成 / 幂等分配 / 黑板目录 / 合法性校验
---

# src/change/change_id.rs

`change-id` 与黑板目录（设计条目 P009 T003 / D008）。

## 职责

本模块负责 change-id 的生成、幂等分配与黑板目录管理。

**负责**：
- 生成 id：`make_change_id`（组装 `change-<date>-<slug>`）、`slugify`（需求短名归一化）、`date_segment` / `date_segment_now`（日期段）。
- 取或首次分配 id 并确保目录存在：`assign_change_id`（幂等，`current` marker 复用）。
- 黑板路径：`change_root`（`plans/<p>/<t>/change`）、`current_marker`（current 记录文件）。
- 合法性校验：`validate_change_id`。

**不负责**：不落盘计划/历史（计划与历史落 `<change-id>/` 目录，由其他模块写）；不自持跨会话的往返计数；不判断边界（见 `src/change/boundary.rs`）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `change_root` | 函数 | [change_root](#change_root) |
| `current_marker` | 函数 | [current_marker](#current_marker) |
| `slugify` | 函数 | [slugify](#slugify) |
| `make_change_id` | 函数 | [make_change_id](#make_change_id) |
| `assign_change_id` | 函数 | [assign_change_id](#assign_change_id) |
| `date_segment` | 函数 | [date_segment](#date_segment) |
| `date_segment_now` | 函数 | [date_segment_now](#date_segment_now) |
| `validate_change_id` | 函数 | [validate_change_id](#validate_change_id) |
| `DEVFORGE_TODAY` | 配置项（env） | [DEVFORGE_TODAY](#devforge_today) |

## 依赖

- 上游 / 消费方：
  - `src/change/prepare.rs:8` 引入 `{assign_change_id, date_segment}`；`src/change/prepare.rs:49` 调 `assign_change_id`，`src/change/prepare.rs:50` 调 `change_root`。
  - `src/server/change_engine.rs:260` 调 `date_segment_now`（其注释提到 `DEVFORGE_TODAY` 可覆盖）。
  - `src/change/mod.rs:28` 经 `pub use change_id::{assign_change_id, change_root, make_change_id, slugify, validate_change_id}` 转发导出。
- 向下依赖：`src/error.rs`（`Context` / `DevforgeError` / `Result`，`src/change/change_id.rs:8`）；标准库 `std`（`Path` / `PathBuf` / `time`）。
- 外部 crate 依赖：无；日期不引第三方，自实现 `civil_from_days`（内部）。

## 核心口径

- id 形状：`change-<YYYYMMDD>-<slug>`；slug 由需求短名归一化（小写、非字母数字折成 `-`）。
- **幂等**：同一 `project` / `task` 重复触发必须复用已有 id——`<change_root>/current` 记着当前 id，存在即复用；不复用会让「同一件事」在黑板上长出第二份计划。
- 目录形状：`<plans_root>/<project>/<task>/change/<change-id>/`；检查点按 010 D001 另住 `<plans_root>/<project>/<task>/change/state.json` 同层不混。

---

### change_root

- 摘要：返回黑板 change 根目录 `plans/<project>/<task>/change`。
- 行为语义：纯路径拼接，不落盘、无 IO。幂等、无副作用。（🟢 CONFIRMED `src/change/change_id.rs:15`）
- 参数：`plans_root: &Path`（黑板根）、`project: &str`、`task: &str`。
- 返回：`PathBuf`。
- 异常：N/A。
- 示例：`change_root` 返回值被 `assign_change_id` 用于拼目录，`src/change/prepare.rs:50` 直接消费。

### current_marker

- 摘要：返回当前 change-id 的记录文件路径（change 根下的 current 文件）。
- 行为语义：在 `change_root` 下追加 `"current"`；不落盘、无 IO。幂等、无副作用。（🟢 CONFIRMED `src/change/change_id.rs:20`）
- 参数：`plans_root: &Path`、`project: &str`、`task: &str`。
- 返回：`PathBuf`。
- 异常：N/A。
- 示例：`assign_change_id` 读/写 `current_marker` 实现幂等复用（`src/change/change_id.rs:60`）。

### slugify

- 摘要：把需求短名归一化成 slug：小写、只留 `[a-z0-9]`，其余折成单个 `-`，首尾去 `-`。
- 行为语义（🟢 CONFIRMED `src/change/change_id.rs:25`）：
  - 仅 `is_ascii_alphanumeric` 保留并转小写；非 ASCII 字母数字一律折 `-`（连续折成一个）。
  - 首部连续折号不产生（首个 `-` 需前面已有内容）；尾部连续 `-` 在循环后 pop 掉。
  - 结果为空（如全非 ASCII）时兜底为字面量 `"change"`。
  - 截断到前 40 个字符（防止路径过长）。
  - 幂等、无副作用。
- 参数：`text: &str` — 需求短名。
- 返回：`String`。
- 异常：N/A。
- 示例：`slugify("Add Book API")` → `"add-book-api"`；`slugify("修复：分页越界!!")` → `"change"`；`slugify("  __a__b__  ")` → `"a-b"`；100 字符输入截到 40（`src/change/change_id.rs:149`）。

### make_change_id

- 摘要：组装 id（不落盘；`date` 由调用方给，便于测试与固定口径）。
- 行为语义：返回 `format!("change-{date}-{}", slugify(slug))`；**内部已对 slug 再归一化**，传入的 `slug` 无需先处理。幂等、无副作用。（🟢 CONFIRMED `src/change/change_id.rs:48`）
- 参数：`date: &str`（形如 `20261020`，与 id 同口径）、`slug: &str`。
- 返回：`String`。
- 异常：N/A。
- 示例：`make_change_id("20261020", "Add Book API")` → `change-20261020-add-book-api`。

### assign_change_id

- 摘要：取（或首次分配）change-id，并确保目录存在。**幂等**。
- 行为语义（🟢 CONFIRMED `src/change/change_id.rs:55`）：
  - 读 `current_marker`；若读得非空 id 且对应目录尚在 → 直接复用（返回 `(id, dir)`），不改号也不重写 marker。
  - 若 marker 有 id 但目录被清过 → 以已有 id 重建目录（用 `Context` 包装 IO 错误），不换号。
  - 首次（marker 无/空）→ `make_change_id(date, slug)` 新分配：建 `.../change/<id>/` 目录、建 marker 父目录、写 `"{id}\n"`。
  - 幂等：同 `project` / `task` 在 marker 存在时始终返回同一 id。
- 参数：`plans_root: &Path`、`project: &str`、`task: &str`、`slug: &str`、`date: &str`（形如 `20261020`，仅首次分配使用）。
- 返回：`Result<(String, PathBuf)>` — `(id, 目录)`。
- 异常：IO 错误用 `Context` 包装成 `Result`（来自 `src/error.rs`），文案含「重建黑板目录」「建黑板目录」「写当前 change-id」；marker 的父目录用 `expect` 断言（文案「marker 必有父目录」）。
- 示例：同任务重复触发返回同一 id/目录（`src/change/change_id.rs:157`）；目录被清后复用 id 并重建（`src/change/change_id.rs:197`）。调用点 `src/change/prepare.rs:49`。

### date_segment

- 摘要：`2026-10-20` 风格的 (年,月,日) → id 段（`20261020`）。
- 行为语义：`format!("{year:04}{month:02}{day:02}")`，月份/日期零填充两位。幂等、无副作用。（🟢 CONFIRMED `src/change/change_id.rs:87`）
- 参数：`year: i32`、`month: u32`、`day: u32`。
- 返回：`String`（8 字符）。
- 异常：N/A。
- 示例：`date_segment(2026, 10, 5)` → `"20261005"`（`src/change/change_id.rs:207`）。

### date_segment_now

- 摘要：今天的日期段（本地时钟；不引第三方时间库）。
- 行为语义（🟢 CONFIRMED `src/change/change_id.rs:94`）：
  - 若环境变量 `DEVFORGE_TODAY`（`YYYY-MM-DD`）存在且能按 `-` 拆成 3 段并解析为 `i32`/`u32` → 用其覆盖日期段。
  - 否则取 `SystemTime` 的 `now()` 距 `UNIX_EPOCH` 的秒数，`div_euclid(86_400)` 得天数，经内部 `civil_from_days` 转 (年,月,日)。
  - 无副作用（读环境变量与时钟可视为只读）。
- 参数：N/A（无参数）。
- 返回：`String`（8 字符日期段）。
- 异常：N/A（`unwrap_or(0)` 兜底时钟误差）。
- 示例：`date_segment_now()` 结果用于 prepare 首次分配（`src/server/change_engine.rs:260` 间接消费）。

### DEVFORGE_TODAY

- 名称：`DEVFORGE_TODAY`（环境变量）。类型：字符串，形如 `YYYY-MM-DD`。
- 默认值：未设置时用本地时钟（`date_segment_now` 回退）。
- 是否必填：可选。
- 取值范围：`YYYY-MM-DD`，三段均需可解析为整数；格式不满足时整段被忽略并回退本地时钟。
- 影响：覆盖「今天」的日期段，用于 CI / 固定口径（让同一天运行产出确定性的 change-id）；测试无法并行改全局 env，仅验 `date_segment` 本身（`src/change/change_id.rs:219`）。
- 示例：设 `DEVFORGE_TODAY=2026-10-20` 后，`date_segment_now()` 返回 `"20261020"`（🟢 CONFIRMED `src/change/change_id.rs:95`, `src/server/change_engine.rs:258`）。

### validate_change_id

- 摘要：id 合法性校验（防止手工塞进奇怪名字）。
- 行为语义（🟢 CONFIRMED `src/change/change_id.rs:132`）：以 `"change-"` 开头，且全部字符为小写 ASCII、数字或 `-`，否则返回 `DevforgeError` 的 `Config` 变体（文案 `非法 change-id：{id}`）。只校验语法形态，不校验日期合法性。幂等、无副作用。
- 参数：`id: &str`。
- 返回：`Result<()>`；合法 → `Ok(())`，否则 `Err`。
- 异常：`Config` 变体（`DevforgeError`）— 触发条件：非 `change-` 前缀、含大写/非 ASCII/`/`/`.` 等。客户端应把 id 视为非法并拒收（如手工黑板路径）。
- 示例：合法 id `change-20261020-x` 通过（`Ok`）；`Change-20261020-x` 与路径穿越串（如 ../evil，含斜杠）被拒（`Err`）。（🟢 CONFIRMED `src/change/change_id.rs:225`）

### civil_from_days（内部）

- 摘要：自 1970-01-01 起的天数 → (年, 月, 日)。Howard Hinnant 的 `civil_from_days` 实现。（🟢 CONFIRMED `src/change/change_id.rs:118`）
- 行为语义：标准 civil 算法，支持负天数；被 `date_segment_now` 消费。非 pub（内部符号）。
- 参数：`z: i64` — 距 epoch 的天数。
- 返回：`(i32, u32, u32)` = (年, 月, 日)。
- 异常：N/A（纯算术）。
- 示例：`civil_from_days(0)` → `(1970,1,1)`；`civil_from_days(18262)` → `(2020,1,1)`；`civil_from_days(-1)` → `(1969,12,31)`（`src/change/change_id.rs:212`）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `slugify_normalizes_and_bounds` | 归一化 + 兜底 + 截断 40 | `src/change/change_id.rs:149` |
| `same_task_reuses_change_id` | 幂等复用 id/目录 | `src/change/change_id.rs:157` |
| `different_tasks_get_different_ids_and_dirs` | id 可同、目录隔离 | `src/change/change_id.rs:171` |
| `path_shape_is_plans_project_task_change_id` | 目录形状 | `src/change/change_id.rs:184` |
| `marker_survives_deleted_dir_without_renumbering` | 目录被清不换号 | `src/change/change_id.rs:197` |
| `date_segment_is_zero_padded` | 零填充 | `src/change/change_id.rs:207` |
| `civil_date_conversion_matches_known_dates` | civil 换算已知日期 | `src/change/change_id.rs:212` |
| `env_override_wins_for_today` | env 口径（只验拼接） | `src/change/change_id.rs:219` |
| `validate_rejects_weird_ids` | 合法/非法 id | `src/change/change_id.rs:225` |
