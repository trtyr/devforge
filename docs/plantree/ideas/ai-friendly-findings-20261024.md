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

## AF-4 ·（已拍板 2026-10-24，与 AF-1/AF-3 同批派工）常量纪律上移 AGENTS.md，kickoff 只留变量

- **现状实铸（双份纪律对照）**：同一套纪律存在两份——AGENTS.md（role_prompt，380 行角色卡）
  已有完整版：「只写 docs/」（332）、「codegraph-first」（335 含新鲜度细节）、「事实/推断分离+🟢🟡🔴」（336）、
  「doc-contract + N/A」（337）、「<doc-complete> 逐字遵守」（363-369）；而 doc_kickoff 又注入了
  这五条的摘要副本。唯一反向错位：穿插节奏/批量写/30K 预算（DR-11/10④）只在 kickoff，AGENTS.md 没有。
- **两宗罪**：① 副本漂移——同一规则两个生效层，改一忘一即双权威打架（违反「规则只允许一个生效层」，
  codegraph-first 已轻度漂移：AGENTS.md 完整版 vs kickoff 四字摘要）；
  ② 权重稀释——纪律藏在工单消息里当「任务描述」被读，遵守率低于系统提示词层。
- **定稿形态**：
  1. AGENTS.md（DOC_AGENT_PROMPT）补一节「写作节奏」：DR-11/10④ 三条款迁入，成为唯一权威；
  2. doc_kickoff 瘦到 ~150 字节纯变量工单：单元号 + 档位 + 作业本路径（AF-1 v2）+ 「按你的纪律干，
     完成交 <doc-complete>」；零常量。
- **分层原则**：改得少的进规范层（AGENTS.md），次次变的进工单层（kickoff）。
- **生效机制真话**：AGENTS.md 会话启动加载，doc-agent 单会话连跑多单元——改纪律后现跑会话用旧纪律
  到死，新会话才吃新纪律。可接受：纪律低频变。
- **派工提示**：AF-1（作业本）+ AF-3（摘 bash）+ AF-4（纪律上移）动同一条 prompt/渲染链，
  一次派工 devforge 改完一次验收，避免三次返工同一函数。

## AF-5 ·（已拍板 2026-10-24，机制定稿，排在 AF-1/3/4 批次后派工）协议结构化：文本咒语 → 交卷工具

- **研究结论（2026-10-24 查证 pi 文档）**：机制全在，零 pi 改动——
  ① pi 扩展 `pi.registerTool()` 支持 TypeBox 参数 schema 的自定义工具（机器可校验）；
  ② RPC 事件流本就暴露 tool_execution（toolName + args，DR-9 L2 已在消费）；
  ③ profile 的 `extensions/` 目录分发，渲染体系现成的路。
- **方案**：doc profile 带专属扩展注册交卷工具——doc-agent 得 `submit_unit({unit})`，
  审查官得 `submit_verdict({verdict, route, issues})`；引擎判据从「文本里找魔法块」
  改为「工具事件 + 结构化 args 字段比对」。
- **退役清单（好方案的标志 = 让补丁失业）**：
  ① parse_doc_complete 的 rfind/trim/大小写容忍全容错；
  ② 编号输错/漏输出 → nudge 烧回合（换成工具参数校验失败的一条重试提示）；
  ③ DR-13 裁决截断 = 本轮作废（裁决是工具参数不是叙述后文本，结构性免疫截断）；
  ④ prompt 里整段「输出协议（强制，逐字遵守）」咒语（模型调工具不用背咒语）。
- **哲学收束（AF 系列统一）**：五条 AF 是同一原则的五次应用——能靠机制的不靠提示词，
  能靠引擎的不靠模型自觉，能剥夺的 不劝阻。协议的失败模式在引擎侧不可达。
- **成本（诚实账）**：扩展 ~40 行 ×2；render 分发 extensions 目录 ~10 行；引擎主改动 =
  交卷信号从最终文本搬到事件流（client.rs wait 结果加 submitted 载荷 → SOP 循环改判据，~80 行）；
  prompt 净变简单；测试 mock 需可模拟工具提交。
- **风险与兑底**：模型不调工具 → nudge 换「调用 submit_unit」（工具调用比逐字复刻文本标签简单）；
  参数质量 → TypeBox schema 引擎侧校验，坏即重试不静默错。
- **顺序**：AF-1/3/4（prompt/渲染链）先行 → AF-5（动引擎事件流）独立批次，均派工 devforge。

## 附：验收与量纲

- AF-1/2 的验收数据源 = acc2 事件流 + L4 transcript（DR-9 已建）；开场 inventory 全量读的出现次数是硬指标。
- 本文件由 2026-10-24 架构讨论沉淀；AF-1 用户拍板「可以，优化一下吧」。
