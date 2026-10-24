# AI Friendly 架构审视（2026-10-24）

> 场景：acc2 自举验收跑通后（写审重叠实证、~7min/单元）的系统自省。
> 方法：对照「引擎缩小世界、agent 只见单元」的哲学，逐面检查信息流向。
> 结论：缩小世界做到了，**把引擎已知事实直供给 agent 没做**——agent 在大量重复发现引擎早已知道的东西。
> 五条按杠杆排序；AF-1 已拍板立案，AF-2..5 待逐条讨论。

## AF-1 ·（已拍板 2026-10-24）kickoff 直供单元清单：消灭开场全量读清单

- **现状**：`doc_kickoff`（src/sop/doc_from_zero.rs:172）开场只给指针——「清单：{META_DIR}/inventory.json（先读它）」。
  devforge 自己的 inventory 是 883 条/几百 KB，agent 每个单元开场都全量吞一遍，再靠脑子过滤出
  本单元的几十条——34 个单元交 34 遍上下文税；漏读条目只能靠审查 FAIL 打回返工。
- **倒置本质**：「本单元要写什么」的完整答案（unit_id 索引）在引擎发 kickoff 那一刻就在引擎手里
  （`coverage_for_unit` 就是一行 `filter(|e| e.unit_id == unit)`），查询却让 agent 来做。
- **期望**：kickoff 渲染前 filter unit_id，把本单元的源文件/公开符号/入口点/档位覆盖明细
  （约 20-40 行）直接写进 kickoff 消息；agent 收到即动笔，零发现成本。
- **落点**：`doc_kickoff` 加 inventory 参数（gate0 已持有）；过滤复用 docgen/audit 的口径；
  kickoff 消息里附「按档位需覆盖的条目数」。
- **验收**：acc2 事件流转录里，doc-agent 开场 N 步内不再出现 inventory.json 全量读；
  kickoff 消息长度含本单元明细。
- **连带**：AF-2（审计前置）是本条的姊妹篇，可同批做——kickoff 同时附确定性覆盖清单（写手自查标准前置）。

## AF-2 ·（待议）确定性审计前置给写手：真相只给法官不给嫌疑人

- 现状：coverage_for_unit/lint 审计只注入审查官 kickoff（DR-12），写手盲写→被确定性审计打回→fix 返工。
- 期望：同一份覆盖清单进写手 kickoff（「引擎认为本单元该有这些条目」），预防性命中，FAIL 回合锐减。

## AF-3 ·（待议）工具权限靠剥夺不靠劝阻

- 实证：2026-10-24 acc2 实测 doc-review 用 bash 干了 78 次调用（cat/grep/ls 照样探索源码）。
  DR-12 摘了 codegraph 但 bash 还在手里。
- 原则：让它做错不了 > 告诉它别做错。审查官工具面应收紧到 read（限 docs 目录）+ 报告 write + verdict。
- 依赖：pi profile 的工具白名单能力（mcp 之外的原生工具管控）。

## AF-4 ·（待议）常量纪律上移 AGENTS.md，kickoff 只留变量

- 现状：doc_kickoff 530 字节里穿插节奏/批量写/30K 预算/纪律全是每次相同的常量，每单元重复注入，
  且埋在任务消息里权重被稀释。
- 期望：常量纪律写进 profile 的 AGENTS.md（会话级持久加载），kickoff 只留变量（单元号+明细）。

## AF-5 ·（中期）协议结构化：文本咒语 → 结构化通道

- 现状：<doc-complete>/<doc-verdict> 是自由文本魔法块；输错编号→nudge 烧回合、漏输出→nudge、
  截断→作废（DR-13）；parse_doc_complete 的 rfind/大小写容忍全是给文本协议打的补丁。
- 期望：失败模式在引擎侧不可达（结构化输出/工具调用收尾），而非 prompt 劝阻。
- 依赖：pi RPC 的结构化输出能力，改动面大，中期评估。

## 附：验收与量纲

- AF-1/2 的验收数据源 = acc2 事件流 + L4 transcript（DR-9 已建）；开场 inventory 全量读的出现次数是硬指标。
- 本文件由 2026-10-24 架构讨论沉淀；AF-1 用户拍板「可以，优化一下吧」。
