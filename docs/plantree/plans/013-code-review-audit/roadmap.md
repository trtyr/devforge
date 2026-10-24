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
  公开面摘要），切片策略 = **语义连贯聚合**（Magistrate Delegator：依赖邻居优先于目录边界）、
  超大单元再切
- ☐ T0.4 `src/audit/` 编排域骨架：断点状态机（沿 doc runner 血统）+ 事件埋点

**验收**：SOP 可被 API 触发走到切片完成（单元列表落黑板）；盘点/切片单测绿。

## M1 · 分阶段审查编排 ☐（2026-10-25 重设计：CodeCrow 实证——视角是提示词内检查清单，不是独立 agent；单元×视角矩阵 = 同源码装 N 遍的上下文浪费）

> 四阶段参照 CodeCrow 实战架构 + RepoAudit validator + 本仓 M4 调度血统：
>
> ```
> Stage 0 规划（1 call，纯元数据零代码——喂代码反而坏）：切片清单+行数统计 →
>         LLM 产出 ReviewPlan：优先级分组 / 可跳过件（生成物/锁文件）/ 跨单元关注假设
> Stage 1 批次审查（N calls 并行）：依赖感知批次（≤7 文件，AST 依赖图连通分量；
>         引擎确定性预查报告注入——V2 unwrap/V3 埋点缺口/V4 行数机检先行）→
>         每批一个会话**综合全六视角**（视角 = 提示词内检查清单；源码只装一次）+
>         跨批次感知（只带其他批次路径清单，不带代码）→ <audit-findings>
> Stage 2 跨单元与结构性视角（1-2 calls）：findings 瘦身（只留 file/line/severity/
>         reason——修复 diff 是噪声）+ codegraph 关系图 → V5 一致性能力对齐 / V4 架构 / 
>         验证 Stage0 假设 / 去重合并
> Stage 3 汇总分级 → 审计报告（接 M2）
> ```
>
> 成本账（CodeCrow）：多阶段 +25% token 换可预测性——单体大 prompt 在中间文件漏发现、
> 重跑更贵、无法并行。

- ☐ T1.1 批次审查提示词（六视角检查清单化 + 结构化三步法 + `<audit-findings>` 强制
  path:line 证据零证据不入）
- ☐ T1.2 Stage 0 规划器（元数据 → ReviewPlan：优先级分组/可跳过件/跨单元假设）
- ☐ T1.3 依赖感知批次器（AST/inventory 依赖图 → 连通分量 → ≤7 文件/批）
- ☐ T1.4 引擎确定性预查注入（V2 unwrap/let-_ 扫描、V3 埋点缺口比对、V4 行数机检——
  零 LLM，报告随批次注入）
- ☐ T1.5 Stage 2 跨单元分析（findings 瘦身 + codegraph 关系图 → V5 对齐/V4 架构/去重）
- ☐ T1.6 断点续跑：阶段进度落 runstate（批次粒度）

**验收**：假 session 单测——矩阵调度、协议违例重审、断点恢复全绿。

## M2 · 核验过滤 + 汇总分级 ☐

- ☐ T2.1 发现核验器（确定性零 LLM）：path:line 存在性校验（plan_audit 血统抽函数）
  + 行号顺序合法性解析验证（RepoAudit validator：引用行必须在声明行后，零 LLM 可验）
  + 可静态证实主张的检查证实；幽灵发现丢弃并计数（无 validator 时 FP +245% 的前车之鉴）
- ☐ T2.2 跨单元去重：同根因合并（同文件+同类发现的聚类规则）
- ☐ T2.3 严重度分级 + 报告组装：critical/major/minor/**nit**（Google：非阻塞打磨层）
  + 执行摘要；
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
