# AI Friendly 代码架构审视（2026-10-24）

> 场景：acc2 验收跑通后，用户直觉「代码会变成一坨屎」，要求按 AI friendly 架构标准体检代码结构。
> 方法：对照 AI friendly 架构五特征（综合 OpenAI harness engineering / Anthropic context engineering /
> Cognition Devin 经验，来源 maintainable.software agentic-engineering part 2 及其引用）。
> **判据一句话**：陌生的 coding agent 能否——找到正确上下文 → 做一个有界改动 → 跑最小验证回路，
> 全程不用猜、不用读全库？
> 五特征：局部性 / 爆炸半径 / 边界完整性 / 可导航性 / 窄验证回路。

## 体量全景

30,225 行 Rust（10-16 hello world → 10-24，17 天），全程自然生长零架构性重构。
热点：server/mod.rs 2376 / sop/doc_from_zero.rs 1790 / change/session.rs 1771 /
docgen/inventory.rs 1589 / config/manifest.rs 1504——6 个文件超 1500 行。

## 好的一面（基线）

- 可导航性基建同类顶级：codegraph 索引 + AGENTS.md + docs/contexts。
- 窄验证回路优秀：cargo test --lib 3.7s 全绿，agent 可自纠。
- 顶层模块名领域化（sop/change/docgen，非 utils 垃圾场），有垂直切片雏形。

## 病灶（按五特征，带实锤）

### ARC-1 · 局部性崩坏：改一个 SOP 规则要碰 4 文件 2 模块

改「文档审查的一条规则」= doc_from_zero.rs（runner）+ manifest.rs（提示词）+
doc_protocol.rs（常量）+ doc-contract skill。规则散布于技术层之间，不住在自己的领域切片里。
社区标准：垂直切片同居一屋（slice = 变更/验证/理解的最小单元）。

### ARC-2 · 危险耦合五连中（AI 杀手耦合形态逐一对照）

| 形态 | devforge 实锤 |
| --- | --- |
| 控制耦合 | `DEVFORGE_DOC_PIPELINE` env 字符串指挥引擎分支（server/mod.rs:671） |
| 语义耦合 | `<doc-complete>` 文本协议；doc-contract「标题必须是模块路径」「七件套」——从未类型化的命名约定 |
| 时序耦合 | store 被闭包 move 后再造 store_final 的 workaround；render 先于 spawn；gate0 先于 kickoff |
| 全局状态 | `observability::root` 与 `LAST_AGENT_EVENT_MS` 进程级原子（接口外隐藏依赖） |
| 横切策略 | 重试/干预映射/事件记录散在 server+engine+observability 三处，无单点完整策略 |

### ARC-3 · 文件大小违反自家纪律

devforge 给 doc-agent 立的规矩「单次上下文 ≈4K 代码片段」，而自家 6 文件超 1500 行
（server/mod.rs 2376）。agent 改这些文件必须 codegraph 片段化定位——**修改所需隐藏上下文
超出 agent 能稳定持有的量**。AI friendly 定义下的「一坨屎」不是乱，是隐藏上下文超限。

### ARC-4 · 边界完整性：剧本不住在剧本房

run_doc_from_zero 的编排接线（env 分支/重试映射/store workaround）住在 HTTP 服务器文件
（server/mod.rs 850 行引擎区），sop/ 只有循环体——sop↔server 契约模糊。domain/ 层形同虚设
（15K），领域逻辑实际散在 server 与 sop。今晚 lead=1 bug 即发生在边界语义上。

### ARC-5 · 双 runner 影子路径【已修复：ARC-R1，commit b1a88da，2026-10-24】

doc_from_zero.rs 内串行 runner 与 pipeline runner 并存；DEVFORGE_DOC_PIPELINE 默认 off
但生产全走 pipeline——串行路径永不执行却必须同步维护（每特性双份落地）。违反家规
「不留兼容旧路径」，也是控制耦合的实体。

> **已修（ARC-R1）**：DocSopLoop 339 行删除，env 开关与 DEVFORGE_DOC_ONLY 一并退役，
> pipeline 唯一 runner；串行行为等价收编（DR-4 首单元逾期告警、审查协议违例重跑下沉
> spawn_review_worker）；顺修两处 lead=1 移植缺陷（nudge 话术被 C 段重建覆盖 / nudge
> continue 回主循环命中 B 段 break——改为 D 段内层原地重试）。全量 707 测试绿，
> 真实 run 验证写审重叠同秒复现。

## Meta 讽刺（本文件的立论）

devforge 教 doc-agent 的纪律——4K 片段、先读清单、codegraph-first、边界显式——
正是它自己代码库最需要又最缺的。它是 AI 编排系统，自己的代码却在变难改。

## 还债路线（ARC-R，全部不加功能，全部派工 devforge）

| 序 | 活 | 主治特征 |
| --- | --- | --- |
| R1 | pipeline 定唯一 runner，删串行路径与 env 开关【✅ 2026-10-24 b1a88da，操作员亲自执行（底座手术期 devforge 不自举）】 | 边界完整性 + 控制耦合退役 |
| R2 | server/mod.rs 拆解：events.rs / engine/builtin.rs（接线闭包迁 sop/）/ http.rs；watchdog 归 observability；目标 ≤500 行/文件【✅ 2026-10-24 d5a7ab3+174eb3a+64dc347+0a38dab+f8e3295，操作员亲自执行；拆出 events/watchdog/gate/http/builtin/doc_wiring/builtin_tests 七文件全 ≤500，mod.rs 342；接线闭包落 engine/doc_wiring.rs 为 R3 中转站，拆解方案与偏差记录见 ideas/arc-r2-split-plan-20261024.md】 | 爆炸半径 + 可导航性 |
| R3 | 按五阶段剧本垂直切片：sop/doc/（runner+协议+提示词+测试同居）、sop/change/ 等【✅ 2026-10-24 da3f84f+9e2b4f2+830c076+f067256，操作员亲自执行；doc 域落地 sop/doc/ 十三文件（protocol/resume/incremental/fix/state/evidence/prompts/runner/mocks/runner_tests/resume_tests/wiring/mod）全 ≤500，doc_from_zero 模块名消亡路径收正 sop::doc::*，doc_wiring 中转站落位；sop/change/ 等其余域未动，见 ideas/arc-r3-slice-plan-20261024.md 边界 4】 | 局部性 |
| R4 | 提示词外置 assets/prompts/*.md（include_str! 保单二进制） | 局部性（规则单点） |
| R5 | 横切策略（重试/干预/事件）收敛单点模块 | 横切耦合 |

## 优先级与派工

- R1 最先（最小改动立刻止损双维护）【已完成】→ R2（最大手术，趁 30K 行未到 50K）→ R3 → R4 → R5。
- 与 AF 系列（ai-friendly-findings-20261024.md）关系：AF 管协议/提示词面对 agent 的友好度，
  本文件管代码结构面对 coding agent 的友好度；AF-1/3/4 与 R2/R3 动同一批文件，
  派工时合并规划、分批验收。
- 分工纪律不变：全部派工 devforge（change SOP），操作员只管它做不了的与 bug。
