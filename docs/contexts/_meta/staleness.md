# 文档对齐收口记录（2026-10-28）

本文件记录 docs/contexts 与代码现状的两轮对齐，给后续对齐当起点。

## 校验命令

```
devforge doc-audit docs/contexts --level detailed --repo .
```

**2026-10-28 收口：doc-lint 0 违规（通过）**。本轮开始时 295 个失败。

## 第一轮：机械漂移（295 → 58）

只做事实纠错，不碰口径判断：
1. 测试文件路径系统性错误 131 处——目录式路径（如 src/config/manifest/manifest_tests.rs）
   改为真实平铺路径（src/config/manifest_tests.rs）。
2. 测试用例行号全失效 117 处——ARC-R6 测试分离后用例迁到同名 `_tests.rs`，
   主文件行号全部指向 EOF 之后；改为指向外置测试文件、不再逐条标行号。

## 第二轮：逐条核对（58 → 0）

每条都对过当前源码：

- **testing_gate.rs.md 重写**：reconcile_acceptance 已删除（宽松对账语义由 session 的
  path_refs/ref_resolves 与 report 的 summarize_acceptance 承担）；补上新增的两个公开函数
  snapshot_tracked_changes / check_no_product_code_edits 的完整条目；测试表重写为
  testing_gate_tests.rs 的 17 例实测清单；回归证据 freshness（not_before）与 untrusted 块
  两处口径补录；实测 17 passed 落档。
- **boundary.rs.md**：is_fail（impl 方法）已删，条目移除；tracing 事件名
  boundary.gap_ignored / boundary.verdict 改为散文描述（事件名不是代码实体，
  行内代码会被实体检查误判）；测试表重写为 boundary_tests.rs 的 24 例。
- **session.rs.md + session_tests/runtime.rs.md**：两个改名用例更新为
  coding_review_failure_returns_issues_instead_of_failing_the_stage（:237）与
  testing_review_fail_reroutes_to_coding_instead_of_failing_the_run（:279），
  新增 reroute_still_runs_the_product_code_gate（:345）与
  red_regression_without_route_block_is_nudged_not_judged_a_liar（:401）两行，
  全表行号按现树重核；实测 runtime 21 passed 落档。
- **coding_gate.rs.md**：eleven_steps_with_prose_digits_reconcile_via_check_step_commits
  （改名 + 真仓直调版，coding_gate_tests.rs:585）。
- **台账迁出仓库（2026-10-25 拍板）**：history/plan_audit/state/installer/skill_assets
  五篇里的仓内 docs/plantree/... 引用改写为「台账在仓库外 plantree 根下」的散文出处
  （行内代码写路径会被路径检查判幽灵/越界，散文描述是正确形态）。
- **行内绝对路径**：/tmp、/var/tmp、../ 等 7 处改为散文（「系统临时目录」「干净树」等）。
- **gaps.md**：加「缺口登记总则」头（本文件是登记真源 + reviews 归档索引）；
  unit-src-01/25/30 三节标题补文档文件名；unit-src-23 的三条缺口标注 ✅ 闭环
  （reconcile 删除 ×2、解析标签修复），unit-src-17 的「判负形状不一致」标注 ✅ 闭环
  （change #4 落地）；unit-src-19 两条行号刷新。

## 仍然已知、未在本轮处理的

- **覆盖审计（coverage）仍不过**：inventory.json 是 doc-from-zero 流水线上次运行
  （unit-src-34 pending-review 时停下）的旧清单，之后的代码增长（sop/marker、
  engine/policy、docgen/lint 拆分等）没有对应文档页 → detailed 档要求 1663 条 / 已覆盖
  869 条。这不是「修文档」能解决的，需要跑一轮 doc 流水线增量（或重跑 doc-inventory
  刷新分母后逐单元补页）。lint 的实体索引扫的是**活源码**，不受旧清单影响。
- **行内 path:line 引用只验「行在范围内」不验语义**：lint 抓不到「行号合法但指错地方」。
  本轮改过的段落都按现树重核过，未触碰段落的旧行号仍是快照——按符号名定位最可靠。
