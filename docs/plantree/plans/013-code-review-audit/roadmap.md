# Roadmap — P013 从零全量代码审计 SOP

> 状态图例：☐ 待做 / ◐ 进行中 / ✅ 完成 / ⛔ 阻塞
> 验收口径：测试级（2026-10-25 用户裁决延续——单测+集成测试全绿即过；不做端到端长跑；
> 唯一例外 = T3.3 devforge 自审是**交付物本身**，不是测试）。

## M0 · SOP 骨架 + 盘点切片（先行）☐

- ☐ T0.1 `code-review` SOP 注册：SopRef（`sop/mod.rs` 注册表）+ 提示词资产
  （`assets/prompts/code-review-agent.md` 等分视角提示词）+ agent 清单（复用 doc-review 形态）
- ☐ T0.2 盘点阶段：复用 `docgen::inventory::build_inventory` → 审查工作台输入
  （模块/符号/Behavior 测试清单/语言栈）
- ☐ T0.3 切片器：inventory 单元 → 审查单元（源码文件集 + 依赖邻居 + 测试清单 +
  公开面摘要），切片策略 = 模块边界优先、超大单元再切
- ☐ T0.4 `src/audit/` 编排域骨架：断点状态机（沿 doc runner 血统）+ 事件埋点

**验收**：SOP 可被 API 触发走到切片完成（单元列表落黑板）；盘点/切片单测绿。

## M1 · 并行专家审查编排 ☐

- ☐ T1.1 分视角提示词：正确性 / 设计 / 测试缺口 / 安全（每视角一份审查指令，
  输出协议沿 verdict 块家族：`<audit-findings>` 结构化发现）
- ☐ T1.2 单元 × 视角矩阵编排：并行审查会话（用完即弃 + review_with_gate 三通道复用）
- ☐ T1.3 结构化发现解析：findings 块（path:line/视角/严重度主张/描述）→ 落黑板
- ☐ T1.4 断点续跑：审查会话进度落 runstate（单元/视角粒度）

**验收**：假 session 单测——矩阵调度、协议违例重审、断点恢复全绿。

## M2 · 核验过滤 + 汇总分级 ☐

- ☐ T2.1 发现核验器（确定性零 LLM）：path:line 存在性校验（plan_audit 血统抽函数）
  + 可静态证实主张的检查证实；幽灵发现丢弃并计数
- ☐ T2.2 跨单元去重：同根因合并（同文件+同类发现的聚类规则）
- ☐ T2.3 严重度分级 + 报告组装：critical/major/minor + 执行摘要；
  产出 `{plans}/<p>/<t>/audit-report.json` + `audit-report.md`
- ☐ T2.4 报告端点：run detail 透出审计报告路径与统计（沿 phases/事件血统）

**验收**：核验过滤单测（幽灵 path 拦截/正常发现放行）+ 报告组装单测绿。

## M3 · devforge 自审（真任务交付）☐

- ☐ T3.1 对本仓（`/home/trtyr/code-work/devforge`）跑一次全量审计
- ☐ T3.2 交付审计报告（JSON+MD），质量底线：含真实 path:line 证据的高严重度发现 ≥ 1、
  无幽灵发现（核验过滤零残留）、分级合理
- ☐ T3.3 自审发现的 top 问题整理回 plantree（作为 AF/后续 plan 的输入）

**验收**：报告文件存在 + 质量底线三条逐条核。

## 依赖

M0 → M1 → M2 → M3（串行推进；M3 依赖前三期全绿）。
