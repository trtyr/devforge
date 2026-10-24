# AI Friendly 架构审视（2026-10-24）

> 场景：acc2 自举验收跑通后（写审重叠实证、~7min/单元）的系统自省。
> 方法：对照「引擎缩小世界、agent 只见单元」的哲学，逐面检查信息流向。
> 结论：缩小世界做到了，**把引擎已知事实直供给 agent 没做**——agent 在大量重复发现引擎早已知道的东西。
> 五条按杠杆排序；AF-1 已拍板定稿 v2，AF-3 已拍板待派工 devforge，其余待逐条讨论。
>
> **分工纪律（2026-10-24 用户明示）**：devforge 已能开发之后，操作员不再亲自编码——
> AF 系列的实现全部派给 devforge 自己（自举）；操作员只负责 devforge 做不了的或它出 bug 时的修复。

## AF-1 ·（已拍板 2026-10-24，形态 v2：指针 + 作业本）kickoff 瘦身成指针，引擎事实落盘成单元作业本

- **现状**：`doc_kickoff`（src/sop/doc_from_zero.rs:172）开场只给指针——「清单：{META_DIR}/inventory.json（先读它）」。
  devforge 自己的 inventory 是 883 条/几百 KB，agent 每个单元开场都全量吞一遍，再靠脑子过滤出
  本单元的几十条——34 个单元交 34 遍上下文税；漏读条目只能靠审查 FAIL 打回返工。
- **倒置本质**：「本单元要写什么」的完整答案（unit_id 索引）在引擎发 kickoff 那一刻就在引擎手里
  （`coverage_for_unit` 就是一行 `filter(|e| e.unit_id == unit)`），查询却让 agent 来做。
- **定稿形态（用户拍板 2026-10-24 13:12，v2 修正了 v1「直接注入上下文」的想法）**：
  上下文 = 寄存器（易失、会忘），文件 = 可信内存（持久、可重读）。**引擎事实落盘，kickoff 只留指针**：
  1. 引擎在 kickoff 前生成 `{META_DIR}/units/{unit}.md`（「单元作业本」）：inventory 按 unit_id 过滤的
     源文件/公开符号/入口点明细 + 按档位的覆盖清单（即 AF-2 的审计内容，同源同文件）+ inventory 版戳；
     幂等可重生成，随 run 生命周期管理。
  2. kickoff 内联瘦到约 10 行恒定核：单元号、产出目录、**作业本确切路径**、完成协议提醒；
     纪律常量按 AF-4 上移 AGENTS.md。
  3. 写手开场一次 read 作业本即动笔；上下文中后期遗忘可随时重读（对冲 compaction 丢弃，
     即 DR-11 `om.observations.dropped` 的教训）。
- **同源红利**：写手的作业本与审查官的确定性审计出自同一份数据——作业本 = 评分标准，无版本漂移
  （AF-2 被本形态吸收，不再单独注入）。
- **已知风险与兑底**：模型可能跳读作业本凭记忆写 → ① kickoff 一句话点名先读；② 文件小、单次 read；
  ③ 跳过也会被审查时同源审计打回，失败被抓不静默漏。
- **落点**：`doc_kickoff` 加 inventory 参数改指针化；新增单元作业本渲染（复用 docgen/audit 过滤口径）；
  review 侧审计注入改读同一文件。
- **验收**：① acc2 转录中 doc-agent 开场不再出现 inventory.json 全量读；② kickoff 消息长度恒定小；
  ③ 单元作业本文件存在且与审查审计同源。

## AF-2 ·（已吸收进 AF-1 v2，2026-10-24）确定性审计前置：真相只给法官不给嫌疑人

- 原问题：coverage_for_unit/lint 审计只注入审查官 kickoff（DR-12），写手盲写→被确定性审计打回→fix 返工。
- 解法：不再单独注入——覆盖清单就是单元作业本（AF-1 v2）的组成部分，写手与审查官同源同文件。
  本条作为独立案关闭，实现归 AF-1。

## AF-3 ·（已拍板 2026-10-24，待派工 devforge）工具权限靠剥夺不靠劝阻

- 实证：2026-10-24 acc2 实测 doc-review 用 bash 干了 78 次调用（cat/grep/ls 照样探索源码）。
  DR-12 摘了 codegraph 但 bash 还在手里。
- 原则：让它做错不了 > 告诉它别做错。定位修正（用户拍板）：审查官 = 读者 + 判官，不是探索者；
  实际 boot 验证不在其职责内（以引擎审计为准）。
- **定稿方案**：审查官工具面只剩 read + write。pi 原生支持 settings.json `defaultTools` 白名单
  （`--tools`/`-xt` 旗子亦可），无需改 spawn。
- 落点：① `AgentManifest` 加 `tools: Option<Vec<String>>`；② render 写 settings `defaultTools`；
  ③ builtin doc-review 声明 `[read, write]` + host manifest 同步（API PUT）；
  ④ 两处审查提示词摘「照 Runbook boot 一遍」措辞（定位修正）。
- **执行方 = devforge（change SOP 派工），操作员不再亲自编码**（分工纪律 2026-10-24 用户明示：
  有了开发系统后，操作员只负责 devforge 做不了的或出 bug 的修复）。
- 状态：用户拍板后操作员曾误上手编码，已全量回滚（HEAD cd0e036，599 测试绿），待派工。

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
