# 00 · Overview — 从零全量代码审计 SOP（code-review）

> 目标：拿到**任意新项目**，从头审计整个代码库——不是审 PR/diff，是审项目本身。
> 产出一份数据驱动的全量审计报告：分视角发现、path:line 证据、严重度分级。
>
> 2026-10-24 用户洞察立项：「我们不是没有 code review 的能力，而是没有单独给它
> 做出一个从 0 开始的 SOP——很多 SOP 都有这个玩意儿。」本计划 = 把散落的审查
> 能力组织成独立流水线。

## 一、现状盘点：散落在各 SOP 的审查能力（复用矩阵）

### 通用审查基建（`src/sop/`，直接复用）

| 件 | 位置 | 形态 |
|---|---|---|
| ReviewSession / Factory | sop/planning.rs（decisions/004） | 审查实例**用完即弃**、每轮全新无记忆 |
| review_with_gate 三通道 | sop/review_gate.rs（D026） | 判负回注 / 协议违例换实例（≤3）/ 实例失败重试，ReviewBudget 预算 |
| RpcReviewFactory::per_round_in | sop/rpc_bridge.rs | 生产桥：profile + session_root + blackboard |
| 里程碑观测 | 通用 | `review.open`（开庭带角色+轮次）/ `fix.start` / `fix.end` |

### 各 SOP 内嵌的审查环节（能力源）

| 环节 | 所在 | 可移植的内核 |
|---|---|---|
| diff↔计划对账（六项：依据/白名单/测试类型/依赖/文档/自检） | change coding_review | **对账式审查**：每条改动必须有依据 |
| 白名单边界 + 影响缺口申报回注 | change boundary.rs（D005） | 越界分类 + gaps 回注重计划 |
| 计划审计（六块/幽灵 path:line/要改⊆白名单/⊆契约） | change plan_audit.rs | **确定性 path 校验**血统 |
| 确定性硬门 | change coding_gate.rs | baseline/self_check/step_commits 三门 |
| 回归证据对账（命令/退出码/结论一致 + 反向验证） | testing test_review | **证据不重跑**，验一致性与撒谎 |
| 引擎事实注入、审查官不读代码（DR-12） | doc doc_review_instruction | **审查官吃确定性事实，只做语义判断** |
| 双粒度审查（单元级 + 汇编级抽样） | doc（unit review + final review） | 粒度分层 |
| 测试即行为盘点（Behavior 条目 path:line） | docgen/inventory.rs | spec: 条目可直接当测试覆盖底册 |

### 结论

审查的「手筋」全都在：无状态审查实例、三通道门、对账式审查、确定性事实注入、
path 核验、双粒度分层。缺的是**组织形态**——没有一条流水线把「整仓」当输入。

## 二、缺口（从零审计需要而未有）

1. **全仓输入形态**：现有审查吃 diff/计划/文档单元；没有「inventory → 切片 → 审查单元」的整仓输入。
2. **多视角专家**：现有审查单一职责（对账/覆盖/语义）；没有正确性/设计/测试缺口等分视角并行。
3. **跨单元去重汇总**：doc 的 final review 不做发现合并；没有「同一根因跨单元去重 → 分级」机制。
4. **独立 SOP 形态**：code-review 未注册（SopRef/提示词/profile），API 不可触发。

## 三、流水线设计（五阶段）

```
[项目 workspace]
   │
   ▼
阶段 0 盘点（确定性，零 LLM）：复用 docgen::inventory::build_inventory
   │    产出：模块/符号/依赖 + Behavior 测试清单 + 语言栈
   ▼
阶段 1 切片：inventory 单元 → 审查单元列表（每单元 = 上下文包：
   │    源码文件集 + 依赖邻居 + 对应测试清单 + 公开面摘要）
   ▼
阶段 2 并行专家审查（单元 × 视角矩阵，审查会话用完即弃）：
   │    视角：正确性（bug/边界/错误处理）/ 设计（耦合/重复/分层）/
   │          测试缺口（Behavior 对照）/ 安全（输入/凭证/注入）
   │    审查官输入 = 引擎确定性事实（inventory 切片）+ 单元源码；
   │    输出 = 结构化发现（findings：path:line/视角/严重度主张/描述）
   ▼
阶段 3 核验过滤（确定性，零 LLM）：每条发现先验证——
   │    path:line 存在性（plan_audit 血统）；主张可静态证实的跑检查证实；
   │    幽灵发现直接丢弃（CodeRabbit「先验证再交付」抗噪核心）
   ▼
阶段 4 汇总分级：跨单元去重（同根因合并）→ 严重度分级（critical/major/minor）
        → 审计报告：JSON（机器）+ Markdown（人读），含执行摘要
```

**参照**：CodeRabbit（PR 审查的头部标杆）——确定性分析先行、策展上下文不游荡、
建议先验证再交付、专家 agent 并行；我们的 doc-from-zero——inventory → 单元并行 →
汇总的编排骨架（写作换成审查）。三篇调研原文已 ingest wiki。

## 四、与既有件的关系

- **新增**：`code-review` SOP 注册（SopRef/提示词/agent 清单）+ `src/audit/` 编排域
- **复用不改**：inventory、ReviewSessionFactory、review_with_gate、RpcReviewFactory
- **复用微调**：plan_audit 的 path 校验抽成可独立调用的核验函数
- **不碰**：change/testing/doc 各 SOP（它们照常跑）

## 五、非目标

- 不做自动修复（审计只报告；修复走既有 change SOP）
- 不做 PR/diff 审查（那是 change SOP 的领地）
- 不做沙箱执行验证（作废裁决维持；可静态证实的才机器证实，其余留给证据标注）
