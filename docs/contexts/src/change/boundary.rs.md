---
type: module
module_path: src/change/boundary.rs
unit: unit-src-01
description: Coding 越界判定与 plan↔coding 往返控制
---

# src/change/boundary.rs

越界二分与往返（设计条目 P009 T009 / T010c，口径 D007 / D008 / D018 / D026）。

## 职责

本模块负责判断一次 coding 阶段的产出是否越出「计划白名单」边界，并据此决定下一步。

**负责**：
- 判定改动文件是否全在 `whitelist` 内（`classify`）。
- 解析 coding 回复里的协议块 `<route>…</route>`（`parse_route`）与 `<impact-gap>…</impact-gap>`（`parse_impact_gaps`）。
- 在「越界且已正确申报」时裁定回 plan 补边界（`RoutePlan`），在「越界未申报 / 申报不完整」时裁定 FAIL（`OutOfBoundary`）。
- 控制 plan↔coding 往返次数上限（`MAX_ROUNDTRIPS` / `check_roundtrip`）。
- 把已申报的文件并回白名单（`merge_gaps_into_whitelist`）、生成打回 plan 的指令（`replan_instruction`）。

**不负责**：不执行 git diff（`changed` 由调用方传入）、不写历史（`append_roundtrip` 由 orchestrator 调）、不自持往返计数（状态归 orchestrator）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `MAX_ROUNDTRIPS` | 常量 | [常量](#max_roundtrips) |
| `ImpactGap` | struct | [ImpactGap](#impactgap) |
| `BoundaryVerdict` | enum | [BoundaryVerdict](#boundaryverdict) |
| `is_fail`（impl） | 方法 | [is_fail](#is_fail) |
| `parse_route` | 函数 | [parse_route](#parse_route) |
| `parse_impact_gaps` | 函数 | [parse_impact_gaps](#parse_impact_gaps) |
| `classify` | 函数 | [classify](#classify) |
| `check_roundtrip` | 函数 | [check_roundtrip](#check_roundtrip) |
| `merge_gaps_into_whitelist` | 函数 | [merge_gaps_into_whitelist](#merge_gaps_into_whitelist) |
| `replan_instruction` | 函数 | [replan_instruction](#replan_instruction) |

## 依赖

- 上游 / 消费方：
  - `src/change/orchestrator.rs:13` 引入 `boundary::{self, BoundaryVerdict, ImpactGap}`；`src/change/orchestrator.rs:227` 调 `classify`，`src/change/orchestrator.rs:243` 调 `check_roundtrip`，`src/change/orchestrator.rs:252` 调 `merge_gaps_into_whitelist`。
  - `src/change/session.rs:11` 引入 `{parse_route, replan_instruction, ImpactGap}`；`src/change/session.rs:345` 用 `replan_instruction` 组装 `amend_plan` 消息，`src/change/session.rs:415` 与 `src/change/session.rs:501` 用 `parse_route` 判路由。
  - `src/change/history.rs:6` 引入 `ImpactGap`，`src/change/history.rs:21` 用作 `append_roundtrip` 参数。
  - `src/change/mod.rs:24` 经 `pub use boundary::{…}` 转发导出（含 `classify as classify_boundary` 别名）。
- 向下依赖：`src/error.rs`（错误面 `DevforgeError` / `Result`，`src/change/boundary.rs:8`）。
- 外部 crate 依赖：无。

## 核心口径

- **两种越界**：coding 超出正确计划边界（范围膨胀）；或计划本身漏改必须改的文件。
- **区分信号**：是否先回 plan 申报（`<route>plan</route>` + `<impact-gap>`）。
- 越界 + 申报 → 回 plan 补边界，补完重进 coding；越界未申报 → FAIL。
- 往返上限 `MAX_ROUNDTRIPS = 3`，第 4 次须停下，防止边界被逐轮撑大。

---

### MAX_ROUNDTRIPS

- 类型：常量 `usize`，值 `3`。（🟢 CONFIRMED `src/change/boundary.rs:13`）
- 取值：固定 `3`，public const 不可变。
- 影响：`check_roundtrip` 用它判顶；`replan_instruction` 用它拼「第 n/3 次往返」；orchestrator 据此在 n > 3 时打回并停止自动扩边界。
- 示例：n 取 `MAX_ROUNDTRIPS + 1` 时 `check_roundtrip` 返回 Quality 错误（见 [check_roundtrip](#check_roundtrip)）。

### ImpactGap

- 职责：一条「计划漏了」的申报，含文件、理由、证据；是 `<impact-gap>` 协议块的解析产物与申报载体。（🟢 CONFIRMED `src/change/boundary.rs:17`）
- 字段（均 `pub String`，均可空）：
  - `file` — 越界文件路径（示例路径，非仓内文件）。
  - `reason` — 为何必须改。
  - `evidence` — 证据（常为文件:行号）。
- 构造：无显式构造器，直接 struct 字面量（测试内 `ImpactGap{}`，`src/change/boundary.rs:283`）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。
- 生命周期：由 `parse_impact_gaps` 生产；被 `classify` 经 `RoutePlan` 变体携带；被 `merge_gaps_into_whitelist` 消费；被 orchestrator 传入 `append_roundtrip`。

### BoundaryVerdict

- 职责：一次 coding 产出的边界判定结果（三态）。（🟢 CONFIRMED `src/change/boundary.rs:25`）
- 变体：
  - `InBoundary` — 改动全在白名单内。
  - `RoutePlan` — 越界且已正确申报 → 回 plan 补边界；载荷 `gaps: Vec<ImpactGap>`。
  - `OutOfBoundary` — 越界且未申报 / 申报不完整 → 范围膨胀 FAIL；载荷 `extra: Vec<String>`（未申报的越界文件）。
- 派生 trait：`Debug, Clone, PartialEq, Eq`。
- 典型用法：orchestrator 对 `classify` 结果分派（`src/change/orchestrator.rs:227`）：`InBoundary` → 进 Testing；`OutOfBoundary` → 返回 Quality 错误；`RoutePlan` → 补边界往返。

### is_fail

- 摘要：返回是否 FAIL（范围膨胀）。
- 行为语义：仅 `OutOfBoundary` 返回 true；`InBoundary` / `RoutePlan` 返回 false。无副作用、幂等。（🟢 CONFIRMED `src/change/boundary.rs:35`）
- 参数：N/A（`&self`，无额外参数）。
- 返回：`bool`。
- 异常：N/A（不改动错误面）。
- 示例：测试 `declaration_must_cover_every_extra_file` 断言 `is_fail()` 为真（`src/change/boundary.rs:249`）。

### parse_route

- 摘要：解析回复中的 `<route>…</route>`，**只认尾部协议位**。
- 行为语义（🟢 CONFIRMED `src/change/boundary.rs:45`）：
  - 先 `trim_end`；循环剥掉尾部 `<issues>…</issues>` / `<impact-gap>…</impact-gap>` 协议块（可多个）；协议块之后若还有非空白内容，则不算尾部协议位。
  - 剩余文本的最后一行须是独立一行的 `<route>…</route>`；块内 trim 后非空且不含 `<` / `>`，否则返回 `None`。
  - 散文里仅在行内/正文「提及」 `<route>` 不算（c5 场真因：正文「未触发 `<route>coding</route>`」被误当回退指令 → 空 issues → 占位文案 → Quality）。
  - 幂等、无副作用。
- 参数：`text: &str` — coding 回复全文。
- 返回：`Option<String>` — 路由值（如 `plan` / `coding`），无有效 route 位则 `None`。
- 异常：N/A。
- 示例（`src/change/boundary.rs:296`）：

```text
parse_route("<route> plan </route>")          // Some("plan")，容忍空白
parse_route("没有路由块")                       // None
```

### parse_impact_gaps

- 摘要：解析 `<impact-gap>…</impact-gap>` 块，产出完整申报条目列表。
- 行为语义（🟢 CONFIRMED `src/change/boundary.rs:97`，2026-10-27 自审对齐）：
  - **只认尾部协议位**：取 `trim_end` 后**最后一个** `<impact-gap>`，且其后（闭合标签之后）只允许空白；块后仍有正文 → 不认这次申报（warn `boundary.gap_ignored`）。无标签返回空列表。
  - 闭合标签缺失 = 被截断的块，定义上延伸到文末，照常解析。
  - 每行 trim 后去掉 `-` / `*` 前缀，按 `:` 或 `：` 切出键值；键匹配 `文件|file|File`、`为什么必须改|理由|reason`、`证据|evidence`。
  - 字段值先剥包裹用的反引号/引号再 trim。
  - `文件` 行启动新条目，并把**此前攒下的**理由/证据落进去（字段顺序不敏感：理由/证据写在文件行之前也有效）。
  - 文件值须像仓内相对路径（非空、无空白、无尖括号、非绝对路径、无 `..`），否则丢弃该条。
  - 仅保留完整条目（文件 + 理由 + 证据均非空）；不完整条目被过滤。
  - 幂等、无副作用。
- 参数：`text: &str` — 回复全文。
- 返回：`Vec<ImpactGap>`。
- 异常：N/A。
- 示例协议（测试 `REPLY_DECLARE`，`src/change/boundary.rs:205`）：

```text
<route>plan</route>
<impact-gap>
- 文件：src/extra.rs
- 为什么必须改：需求依赖的接口在这里
- 证据：src/extra.rs:42
</impact-gap>
```

### classify

- 摘要：判定一次 coding 产出的边界情况。
- 行为语义（判定顺序，🟢 CONFIRMED `src/change/boundary.rs:167`，2026-10-27 自审对齐）：
  1. 两侧先归一化（去 `./` 前缀 + 迁移表换算）；`extra` = `changed` 中**归一化后不等于**任何白名单项的文件（精确相等，不是子串）。
  2. `parse_route(reply) == Some("plan")` → 一律走 `RoutePlan` 分支（即使 `extra` 为空——不得借「无越界」直通 testing）。此分支下：申报须覆盖每个越界文件；被截断的尾部申报视为无法核实但决策有效；`extra` 空或全覆盖或截断 → `RoutePlan{gaps, reason}`，否则 `OutOfBoundary`。
  3. 非 `route=plan`：`extra` 空 → `InBoundary`；非空 → `OutOfBoundary`（裸 `<impact-gap>` 不算申报）。
  4. 每次判定落 `boundary.verdict` 事件（verdict / changed / extra_or_gaps）。
  - 幂等、无副作用。
- 参数：
  - `changed: &[String]` — 改动文件（调用方由 `git diff --name-only` 于 baseline 提供）。
  - `whitelist: &[String]` — 计划白名单。
  - `reply: &str` — coding 回复。
- 返回：`BoundaryVerdict`。
- 异常：N/A。
- 示例：改动全在白名单 → `InBoundary`（`src/change/boundary.rs:208`）；越界未申报 → `OutOfBoundary`（`src/change/boundary.rs:218`）；越界 + 完整申报 → `RoutePlan`（`src/change/boundary.rs:231`）。

### check_roundtrip

- 摘要：第 n 次往返是否还允许（n 从 1 起）。
- 行为语义：n <= `MAX_ROUNDTRIPS` → `Ok(())`；否则返回 `Err`（`DevforgeError` 的 `Quality` 变体，文案含「已经往返 n 次（上限 3）」）。幂等、无副作用。（🟢 CONFIRMED `src/change/boundary.rs:159`）
- 参数：`n: usize` — 计划中的往返次数（1 基）。
- 返回：`Result<()>`（`src/error.rs` 的 `Result`）。
- 异常：`Quality` — 触发条件 n > `MAX_ROUNDTRIPS`；该错误满足 `is_quality_failure()`（测试 `roundtrip_is_capped_at_three`，`src/change/boundary.rs:273`）。
- 示例：`check_roundtrip(4)` → `Err`（Quality，提示已往返 4 次达上限）。

### merge_gaps_into_whitelist

- 摘要：把已申报的文件并进白名单（供「补完边界后重新校验」与审计展示）。
- 行为语义：遍历 `gaps`，仅当文件不在白名单时才 push；最后 `sort()` + `dedup()`。幂等（多次调用结果一致）、有副作用（修改传入的 `&mut Vec<String>`）。（🟢 CONFIRMED `src/change/boundary.rs:169`）
- 参数：`whitelist: &mut Vec<String>`、`gaps: &[ImpactGap]`。
- 返回：N/A（单元 `()`）。
- 异常：N/A。
- 示例：白名单只含 `a` + gap 指向 `extra` → merge 后白名单含 `extra`，重算 `classify` 变 `InBoundary`（`src/change/boundary.rs:231`）。

### replan_instruction

- 摘要：打回 plan 时给 plan agent 的指令——把这些缺口补进计划并重新交审。
- 行为语义（🟢 CONFIRMED `src/change/boundary.rs:180`）：
  - 首行为「coding 阶段发现计划缺口（第 `roundtrip` / `MAX_ROUNDTRIPS` 次往返）。请只补边界，不要重写整份计划：」，随后每 gap 一行。
  - 尾部要求：更新「影响面」与「三张表」、把文件写进白名单、在「风险」里记录缺口、补完重新交审（只审增量仍不看代码）。
  - 幂等、无副作用。
- 参数：`gaps: &[ImpactGap]`、`roundtrip: usize`。
- 返回：`String`。
- 异常：N/A。
- 示例：`replan_instruction` 输出的文本含文件路径、含「只补边界」、含「第 2/3 次往返」（`src/change/boundary.rs:283`）。调用点 `src/change/session.rs:345` 用它组装 `amend_plan` 消息。
- 说明：指令里「三张表」「影响面」「风险」指 plan 文档既有章节结构；该结构定义在 `src/change/plan_audit.rs`（跨模块引用，🔴 GAP 登记见 `_meta/gaps.md`）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `inside_whitelist_is_clean` | 全在内 → InBoundary | `src/change/boundary.rs:208` |
| `out_of_boundary_without_declaration_fails` | 越界未申报 → FAIL | `src/change/boundary.rs:218` |
| `declared_gap_routes_back_to_plan_and_whitelist_grows` | 申报 → RoutePlan + 白名单增长 | `src/change/boundary.rs:231` |
| `declaration_must_cover_every_extra_file` | 申报不全 → FAIL | `src/change/boundary.rs:249` |
| `incomplete_gap_entry_counts_as_missing_declaration` | 缺理由/证据 → FAIL | `src/change/boundary.rs:259` |
| `route_other_than_plan_is_not_a_declaration` | route 非 plan 不算申报 | `src/change/boundary.rs:266` |
| `roundtrip_is_capped_at_three` | 上限 3，超限 Quality | `src/change/boundary.rs:273` |
| `replan_instruction_lists_gaps_and_asks_only_for_boundary` | 指令内容 | `src/change/boundary.rs:283` |
| `route_parse_is_tolerant_of_whitespace` | 容忍空白 / 无块 None | `src/change/boundary.rs:296` |
| `route_is_only_recognized_at_the_tail` | 仅尾部协议位 | `src/change/boundary.rs:304` |
| `prose_after_the_protocol_block_is_not_a_tail_position` | 块后散文不算尾位 | `src/change/boundary_tests.rs` |
| `route_block_must_stand_alone_on_its_line` | 独立行 + 非空值 | `src/change/boundary_tests.rs` |
