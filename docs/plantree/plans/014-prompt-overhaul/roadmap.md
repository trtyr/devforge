# P014 Roadmap · 提示词体系全面改造

> 方法论与骨架见 00-overview.md。批次按 SOP 家族划分，每批独立 commit、
> 全目标测试绿才进下一批。验收：每条过 overview §四检查表 + manifest pinning 同步。

## M0 · 骨架模板与基线 ☐

- ☐ T0.1 六块骨架模板 + 四型（分析/生成/验证/编排）适配版定稿（落 assets/prompts/_template.md，不进 manifest）
- ☐ T0.2 14 条现状基线留档（改造前快照 + token 量，供前后对比）

## M1 · change 规划 + 编码家族 ✅

- ✅ T1.1 change-plan-agent（重排六块 + 原因三件套 + 边界段四项；歧义契约逐字保留）
- ✅ T1.2 change-plan-review（八项判据保留 + 原因 + 边界段含「无法核实」语义）
- ✅ T1.3 change-coding-agent（基线门/白名单/步提交带原因 + 长时阻塞命令禁令（M3 教训）+ 边界段五项含重入与申报块字面恢复）
- ✅ T1.4 change-coding-review（对账七项保留 + 「不得以 diff 大小判」带原因 + 边界段三项）
- 验收：四条过检查表（六块/原因/边界段/契约全勾，各≤1200 token）+ pinning 断言同步（codegraph-first 锚更新 + fixture 重 dump ×2）+ 全目标 776 绿

## M2 · testing + 审计家族 ✅

- ✅ T2.1 test-agent / test-review（黑盒纪律与七条可靠性纪律全保留重排，新增两个示例；审查官七项+双路由保留，重写时遗失「环境复用」被 pinning 断言逮住回补——断言网价值实证）
- ✅ T2.2 code-review-agent（三步法/六视角/裁决前置保留；新增「空 findings 合法」边界段——幽灵发现的提示词层防线）
- 验收：三条过检查表（test-agent 特批超 1200 token：七条可靠性纪律逐条对应真实炸轮事故，不可删）+ pinning 同步 + 全目标 776 绿

## M3 · doc 家族 ✅

- ✅ T3.1 doc-agent（两条 2026-10-20 事故纪律/三态置信度/七件套自检全保留重排，清掉原文转义残留；新增边界段四项；顺带补回「总览 / 架构 / 数据流 / Runbook」句与弯引号——三个 pinning 锚逐字对齐）
- ✅ T3.2 doc-fix（第三方可测标准/冲突升级/findings 回写带原因 + 边界段四项含「finding 本身判错」诚实路径）；doc-review（DR-12 不读代码与裁决前置保持 + 五项审查面带原因 + 边界段三项含摘要造假加重）
- 验收：三条过检查表 + pinning 同步 + 全目标 776 绿

## M4 · change 测试 + coding 家族 ✅

- ✅ T4.1 change-test-agent / change-test-review（两条证明目标/对照表机械契约/反向验证保留 + 原因三件套 + 边界段四/三项；粗体挡断言的字面教训：锚短语去粗体）
- ✅ T4.2 coding-agent / coding-review（预审四条事故教训/plan 偏差协议/刚需四条/从零 boot 门禁/九项审查面全保留重排 + 边界段四/四项；「自己跑一遍」回补 + 「完全基于文档写测试」跨行断开修复）
- 验收：四条过检查表 + pinning 同步（锚修复 ×3 + fixture 重 dump） + 全目标 776 绿

## M5 · pi 侧系统提示词（pi-config 仓） ✅

- ✅ T5.1 personality + APPEND 轻审改（零行为变更）：Fleet 基准一条塞六项拆六子弹点；编辑纪律删反面重述句（堆积审计）；Don't fake understanding 补正向替代；personality dont_know 缩进修齐。审计结论：pi 侧达标度高（规则带事故注脚/禁止带替代/加载保证分水岭达标），语料库系人格 few-shot 不砍
- ✅ T5.2 pi-config 9efa5513 已推（sync-guard 自动）

## M6 · 终验与回写 ✅

- ✅ T6.1 双 EXIT 原始日志（bg_prnscokm）：TEST_EXIT=0（776 全绿 0 failed：lib 666 + 集成 19/16/31/43 + doctest 1）；CLIPPY_EXIT=0。生产代码 diff 为空（e8a4cdb..HEAD 仅 manifest_tests.rs 断言锦 1 行）→ clippy warning 列表差异系基线记录口径不全，非本批新增
- ✅ T6.2 roadmap 全勾 + overview 补前后对比（见 overview §改造后量测）+ commit

## 依赖与顺序

M0 → M1 → M2 → M3 → M4 → M6 串行（devforge 侧）；M5 独立可并行。
每批验收不过不进下一批；manifest pinning 断言随批同步，不留到终验。
