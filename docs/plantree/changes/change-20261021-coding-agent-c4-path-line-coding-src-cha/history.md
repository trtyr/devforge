# 变更往返历史 · change-20261021-coding-agent-c4-path-line-coding-src-cha

| 往返 | 越界文件 | 理由 | 证据 |
| --- | --- | --- | --- |
| 1 | `tests/change_failure_context.rs` | 它是本变更提交集合的一部分（测试阶段产出，步骤 7 的提交，sha `ebf5d1c`），而计划 §4 白名单只有 10 个文件、不含它。① 引擎越界门（`src/change/boundary.rs` 的 `classify`：拿 `git diff --name-only <baseline>` 与计划白名单比，差集非空且无申报即判「越界改动未申报（范围膨胀）」直接 Quality）会把本变更判死；② 计划 §5 验收第 17 条「白名单之外无任何改动」按「本变更的提交」对账，白名单不覆盖这条提交则该条验收本身无法成立。请把该文件写进 §3.1「要改的」与 §4「白名单」、在 §2 影响面与 §6 风险各记一笔，并把 §5 第 16/17 条里写死的「10 个文件」口径同步成与 §4 一致（`tests/change_ask_ring.rs` 的 `WHITELIST` 常量仍列 10 个 coding 白名单文件；测试阶段文件是本变更提交的一部分，计入 §4 但不计入 coding 白名单常量 —— 否则会破坏 a16「守卫的 WHITELIST 必须逐个列出本场白名单的 10 个文件」）。coding 步提交的文件集合本轮未变（步骤 7 只改提交信息，`git diff --name-only <baseline>` 改前改后逐字相同）。 | tests/change_failure_context.rs:1 |
