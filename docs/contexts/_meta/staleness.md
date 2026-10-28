# 文档对齐现状（2026-10-28）

本文件记录 docs/contexts 与代码现状的差距和本轮已做的修正，给下一趟对齐当起点。

## 校验命令

```
devforge doc-audit docs/contexts --level detailed --repo .
```

2026-10-28 收口时：**58 个失败**（本轮开始时 295 个）。

## 本轮已修（不涉及内容判断，纯事实纠错）

1. **测试文件路径系统性错误**（131 处）：文档把外置测试写成目录式路径
   - src/config/manifest/manifest_tests.rs → 实为 src/config/manifest_tests.rs（56 处）
   - src/change/orchestrator/orchestrator_tests.rs → 实为 src/change/orchestrator_tests.rs（38 处）
   - src/config/render/render_tests.rs → 实为 src/config/render_tests.rs（34 处）
2. **测试用例行号全部失效**（117 处）：ARC-R6 测试分离后用例迁到同名 `_tests.rs`，
   文档仍按主文件行号标注，全部指向 EOF 之后。已改为指向外置测试文件、不再逐条标行号。

## 仍然存在（58 条，逐条都需要对源码核）

- `docs/plantree/...` 引用（11 处）：台账已按「台账不进仓库」口径迁到
  ~/.devforge/plantree/devforge/，文档里的仓内路径已不存在。需要改写为仓外台账位置或降级为史实引用。
- 幽灵实体（21 处）：
  - testing_gate 文档仍描述 reconcile_acceptance（该函数已不在代码中）；
  - 若干用例名已改名或删除（coding_review_failure_fails_the_stage、
    testing_review_is_invoked_and_can_fail、one_missing_criterion_is_zero_coverage、
    full_reconciliation_passes、empty_criteria_is_rejected、
    eleven_steps_with_prose_digits_reconcile_cleanly）；
  - boundary 文档引用的事件名/谓词名与代码不符（is_fail、boundary.gap_ignored、boundary.verdict）。
- 路径越界（7 处）：文档正文用行内代码写 /tmp、../、/var/tmp 这类示例路径，lint 拒绝检查。
  需要把示例改为非路径行内代码。
- 置信标记未入册（13 处）：引用块标了高危标记但 gaps.md 没有对应条目，要么补缺口条目，要么降级。

## 为什么没在这一轮做完

上面 58 条每一条都要读文档上下文 + 核当前源码才能定改法（不是机械替换），
与「每处改动后必须 doc-audit 归零」的门禁一起构成一趟独立的文档对齐工作，
不适合在代码收尾的尾巴上顺手糊完——半对半错的文档比明确过期的文档更危险。
