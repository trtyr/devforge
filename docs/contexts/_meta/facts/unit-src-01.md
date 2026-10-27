# 勘察事实 — unit-src-01（src/change/boundary.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/boundary.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/boundary.rs`（`path:#1`）。
- 模块在 `src/change/mod.rs` 声明（submodule `boundary`），并经 `pub use boundary::{...}` 向 crate 转发导出（`src/change/mod.rs:24`）。
- 文件头模块注释声明对应设计条目：P009 T009 / T010c，口径 D007/D008/D018/D026。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `MAX_ROUNDTRIPS` | `#13` | `pub const usize = 3` |
| `ImpactGap` | `#17` | `pub struct {file, reason, evidence: String}`，derive Debug/Clone/PartialEq/Eq |
| `BoundaryVerdict` | `#25` | `pub enum {InBoundary, RoutePlan{gaps}, OutOfBoundary{extra}}` |
| `BoundaryVerdict::is_fail` | `#35` | impl 方法，匹配 `OutOfBoundary` 返回 true |
| `parse_route` | `#45` | `pub fn(text:&str) -> Option<String>` |
| `parse_impact_gaps` | `#79` | `pub fn(text:&str) -> Vec<ImpactGap>` |
| `classify` | `#133` | `pub fn(changed:&[String], whitelist:&[String], reply:&str) -> BoundaryVerdict` |
| `check_roundtrip` | `#159` | `pub fn(n:usize) -> Result<()>`，超限返回 `DevforgeError::Quality` |
| `merge_gaps_into_whitelist` | `#169` | `pub fn(&mut Vec<String>, &[ImpactGap])`，push/sort/dedup |
| `replan_instruction` | `#180` | `pub fn(&[ImpactGap], usize) -> String` |

## 行为/测试事实（`#[cfg(test)]`，12 例）

- `inside_whitelist_is_clean`：改动全在 whitelist → `InBoundary`（`#208`）。
- `out_of_boundary_without_declaration_fails`：越界文件未申报 → `OutOfBoundary{extra}`（`#218`）。
- `declared_gap_routes_back_to_plan_and_whitelist_grows`：申报 route=plan + 完整 gap → `RoutePlan`，merge 后重算 `InBoundary`（`#231`）。
- `declaration_must_cover_every_extra_file`：申报只盖部分越界文件 → FAIL（`#249`）。
- `incomplete_gap_entry_counts_as_missing_declaration`：缺理由/证据的 gap 不算申报 → FAIL（`#259`）。
- `route_other_than_plan_is_not_a_declaration`：route 非 plan 的 gap 不算申报 → FAIL（`#266`）。
- `roundtrip_is_capped_at_three`：`check_roundtrip(1..=3)` 通过，`(MAX+1)` 返回 Quality 且 `is_quality_failure()`（`#273`）。
- `replan_instruction_lists_gaps_and_asks_only_for_boundary`：文本含文件、只补边界、往返序号（`#283`）。
- `route_parse_is_tolerant_of_whitespace`：`<route> plan </route>` → `plan`；无块 → `None`（`#296`）。
- `route_is_only_recognized_at_the_tail`：尾部独立行 route 才识别；散文提及 + 末尾 `<stage-complete>` → `None`（`#304`）。
- `prose_after_the_protocol_block_is_not_a_tail_position`：协议块后还有散文 → `None`（`#332`）。
- `route_block_must_stand_alone_on_its_line`：行内前缀提及 → `None`；空路由 `<route></route>` → `None`（`#343`）。

## 协议/语法事实（代码内可见规则）

- `parse_route` 解析：`trim_end` 后循环剥尾部 `<issues>`/`<impact-gap>` 块（须块后无其他内容），最后一行须是独立 `<route>...</route>`，值 trim 后非空且不含 `<`/`>`（`#45-#77`）。
- `parse_impact_gaps` 解析：取首个 `<impact-gap>...</impact-gap>` 体；每行按 `:`/`：` split；键支持 `文件|file|File` / `为什么必须改|理由|reason` / `证据|evidence`；新文件行启动条目；**完整条目 = 文件+理由+证据**，不完整被过滤（`#79-#131`）。
- `classify` 判定顺序：先算 `extra=changed − whitelist`；空 → `InBoundary`；否则需 `parse_route==plan` 且 gaps 非空；再查申报覆盖全部 extra（`g.file == f | contains`）；都满足 → `RoutePlan`，否则 `OutOfBoundary`（`#133-#157`）。
- `replan_instruction` 生成文本：头行 `(第 {roundtrip}/{MAX_ROUNDTRIPS} 次往返)` + 每 gap 一行 + 尾部要求（更新影响面与三张表、写白名单、记风险、只审增量）（`#180-#205`）。

## crate 内调用点（消费者）

- `src/change/orchestrator.rs:13` 引入 `boundary::{self, BoundaryVerdict, ImpactGap}`；`#227` 调 `classify`；`#243` 调 `check_roundtrip`；`#252` 调 `merge_gaps_into_whitelist`。
- `src/change/session.rs:11` 引入 `{parse_route, replan_instruction, ImpactGap}`；`#345` 用 `replan_instruction` 拼 `amend_plan` 消息；`#415`、`#501` 用 `parse_route` 判定路由。
- `src/change/history.rs:6` 引入 `ImpactGap`，`#21` 作 `append_roundtrip` 参数。

## 外部依赖

- 类型错误面：`crate::error::{DevforgeError, Result}`（`#8`）。`check_roundtrip` 出错类型为 `DevforgeError::Quality(format!(...))`。
- 无其他外部 crate 依赖。
