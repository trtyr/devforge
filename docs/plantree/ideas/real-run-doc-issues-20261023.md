# 真实项目文档首跑问题清单（2026-10-23）

> 场次：cloudwalker-agent · d1 · doc-from-zero（run 31eee16d，14:01 开跑）
> 背景：e2e 玩具仓全绿后首次真实仓（941 条目 / 34 单元 / 11 顶层区）实战。开跑 50 分钟后操作员巡检发现问题。
> 处置：本轮只立案不修，下次更新统一修（用户拍板 15:53）。
> **修复状态（10-23 晚全批落地，目标 mvkoojcq）**：
>
> | 案 | 状态 | 修复提交 | 证据 |
> |---|---|---|---|
> | DR-1 | ✅ | 468174e + 053dac2 | 探针矩阵（无 key 挂死→有 key started+会话落盘）+ 守卫单测 |
> | DR-2 | ✅ | 275fa55 | agent.turn_start / doc.unit_start/done 事件实测入库 |
> | DR-3 | ✅ | 275fa55 | stall_check 单测 + watchdog agent.stall 告警接线 |
> | DR-4 | ✅ | 37344f3 | first_unit_overdue 单测（恰告警一次） |
> | DR-5 | ✅ | 275fa55 + 053dac2 | units_total 入断点 + progress 端点 x/y |
> | DR-6 | ✅ | 9ecb83b | probe-agent.sh + 三级冒烟制 + 执行记录表 |
> | DR-7 | ✅ | 275fa55 | events/详情端点；本次 relaunch 全程纯 API 观测自证 |

## DR-1 ·（致命）doc-agent 首个 prompt 静默挂死：host profile 无凭据 + 引擎对失败响应无限等待

- **真根因（10-23 晚探针矩阵定位，推翻早前「裸 pi REPL」误诊）**：
  1. host manifest 无 `api_key` 字段 → 渲染出的 9 个 profile 均无 auth.json → 模型调用必败（`No API key found`）；
  2. pi 的错误路径只回 `success:false` 的 response、**永不发 agent_settled** → 引擎 wait_settled 无限等待 → run 无声挂死；
  3. e2e 一直绿是因为 e2e 家 manifest 带 key——host 侧从未被真实调用验证过（DR-6 的直接代价）。
- **勘误**：早前 ps 观测到的「裸 pi 无参数」是 node `process.title` 覆写 argv 区的显示假象——
  探针对照实验证明带全参数的 pi 在 ps 里同样只显示 `pi`。spawn 参数路径无罪。
- **证据链**：探针1（doc profile 无 key）→ 90s 零响应零会话零网络；探针2（coding profile 无 key）→
  立回 `No API key found for the selected model`；探针3（doc profile 补 auth.json）→ `success:true, disposition:started` + 会话文件落盘。
- **修复（四层）**：
  1. ops：9 个 host profile 补 auth.json（600 权限，e2e 验证过的 key）；
  2. data：host manifest 注入 api_key（未来重渲染不再丢凭据）；
  3. code（src/config/render.rs）：渲染时 api_key 缺失当场 WARN（profile.render_without_api_key）；
  4. code（src/engine/client.rs）：wait_settled/wait_settled_with_ui 遇 `success:false` 快速失败
     （agent.command_failed WARN + 结构化错误带角色名与详情），不再傻等 settled；+1 守卫单测
     `failed_response_fails_fast_without_waiting_for_settled`（常驻假 pi 下 <4s 返回）。
- **验收**：探针3 已过；冷启动冒烟见 task-5 重跑；错误路径单测绿。

## DR-2 ·（高）引擎日志黑盒：看不到目标 agent 在干什么

- **现象**：用户原话「日志系统有大问题，不能看到目标 agent 在干啥吗？」
- **现状**：引擎只记里程碑事件（run 开始 / gate.* / agent.turn_end / phase.change）。agent 内部（正在跑哪个单元、第几轮、模型有没有回话）完全不可见，只能靠会话文件旁证——而 DR-1 场景下会话文件都不存在，等于黑上加盖。
- **修法方向**：
  - agent 生命周期事件补齐：rpc 连接建立 / 每轮请求发出（带单元 id + 轮次号）/ 每轮响应回来 / rpc 断开；
  - 或者至少把 stderr 伴读线程的产出常态化进结构化日志（P8-ISSUE-08 已铺管道，但本轮 agent 静默时日志里一个字都没有——判断是 pi 在 REPL 模式不吐 stderr）。
- **验收**：DR-1 复现场景下，日志能直接看出「agent 没被正确拉起」而不是「在慢慢写」。

## DR-3 ·（高）心跳与 run 状态只证明服务端线程活着，不证明 agent 活着

- **现象**：heartbeat 每 300ms 刷新、run status=Running，但实际工作进程是闲置 REPL。心跳的语义是「watchdog 线程活着」，不是「活儿在推进」。
- **修法方向**：心跳 payload 增加 agent 活性字段（session 文件 mtime / 最近 rpc 往返时延 / 已完成轮次计数），或在 API run 详情里透出「最后一个 agent 事件距今 N 分钟」；超阈值（如 15 分钟无任何 agent 事件）打 WARN 日志。
- **验收**：DR-1 复现场景下，心跳/日志在 ~15 分钟内主动报警，而不是靠人巡检 ps。

## DR-4 ·（中）零产出无早期预警

- **现象**：docs/contexts/ 在 run 期间为空是正常的（单元写完才落盘），但配合 DR-1/DR-2/DR-3，没有任何机制在「应该出现第一个单元产出的时间内没出现」时报警。
- **修法方向**：doc SOP 加最小产出节奏守卫（首单元超时告警 / 单元间隔超时告警），复用 change 侧已有的有界追补思路。
- **验收**：人为模拟 agent 假活，能在首单元预期窗口 ×2 内看到告警事件。

## DR-5 ·（记录）真实仓规模感与进度可见性

- 941 条目 → 34 单元（11 顶层区），flash 模型预估 4-6 小时全程。
- run 进行中没有任何「x/34 单元完成」的总进度指标（API/日志均无）——DR-2 修完顺手带上：单元完成事件里带序号，汇总进 run 详情。
- **验收**：API 或日志可一眼看出当前进度 x/34。

  doc_level 轻量档。均非性能阻塞，随真实仓使用观察再定。

## 修复批次（更新后终版顺序）

1. DR-1 根因 + 修复（前提）；2. DR-2 + DR-3 + DR-7（可观测一次动刀：事件加密 → 查询端点 → 聚合端点）；
3. DR-4 + DR-5（守卫与进度）；4. DR-6（冒烟回归机制化）；5. 重启重跑 cloudwalker d1 盯首单元闭环。

## DR-6 ·（流程债）引擎层动刀后，非 change 侧 SOP 零回归

- **现象**：10-22/23 的 c7b 场对引擎做了大手术（渲染闸门 / 心跳拆分 / 原子写 / 步提交门 / 解析器容差），
  全部验收只跑 change 侧；doc-from-zero 的 e2e 停留在 008 时代（10-20，玩具仓）——之后引擎每个补丁都没碰过 doc 侧的冷启动路径。今天真实仓首跑直接踩中 DR-1。
- **教训**：引擎是所有 SOP 的公共底盘，底盘动刀后每个 SOP 都要跑一次最小冒烟（拉起→首单元→落盘）。
- **修法方向**：把「SOP 冒烟」做成发版前固定动作（每个 sop 一条最小路径用例），或至少在每次引擎层提交后手动过一遍 doc 冷启动。
- **验收**：下次引擎动刀后，doc/asking/change 三侧各有一次冒烟记录在案。

### DR-6 修复（10-23 晚落地，37344f3 + 本节）：冒烟三级制

**L1 · spawn/凭据探针（秒级，引擎无关，可重复）**：`scripts/probe-agent.sh <profile目录>`
——发真 prompt，60s 内见 `success=true` = 模型调用已发起；`success=false`（如 No API key）或超时 = DR-1 型故障。
引擎层动刀 / 换 profile / 换凭据后：**9 个 profile 全跑一遍**。

**L2 · 单元回归（分钟级，可重复）**：`env -u DEVFORGE_MANAGED cargo test --offline`
（587 lib + 三守卫套件 + f6 44 + doctest）。

**L3 · 冷启动管线冒烟（真跑）**：被改的 SOP 各一次最小真实跑，盯三件事：
`agent.turn_start` 事件出现 / 心跳推进 / 首个产物落盘。云端仓即 cloudwalker relaunch（task-5）。

**执行记录**：

| 日期 | 侧 | 层 | 结果 |
|---|---|---|---|
| 10-23 | doc（doc-agent profile） | L1 | PASS（探针3 + probe-agent.sh 双验） |
| 10-23 | doc（对照：无 key 场景） | L1 | FAIL→定位→修复（probe1/2 矩阵，即 DR-1 破案过程） |
| 10-23 | change | L2 | 587 全绿（含三守卫套件 + f6 44） |
| 10-23 | change | L3 | c7b 十二续跑 Settled（本日实战即记录） |
| 10-23 | asking/planning | L3 | c7b resume6e/6f 计划轮问询中继全程活跃（SSE ui.request → 应答回填） |
| 10-23 | doc | L3 | ✅ relaunch run 2d9fb677：首单元 unit-app-01 全环真实走通——**3 轮审查 / 2 次打回 / ~87 分钟**（写 16min → 审FAIL → 修复 → 审FAIL → 修复 → 审PASS → unit_done 1/25），58+ 真文档落盘 docs/contexts/，全程纯 API 观测（events/详情端点自证）；期间 agent.stall 两次假阳性（长流式回合）暴露 DR-3 事件粒度缺陷 → 4501f99 修（流式 delta 即活性）+ 二次重建重部署闭环 |
| 10-23 | doc | L3（二次部署） | ✅ 审计驳回后重建重部署（新二进制 18:28，含 053dac2+4501f99）：resumed=true 认账；**DR-3 活体签名实测**——长写作回合中 agent_age 31.7s→3s→11s 持续被流式 delta 压低（旧二进制下单调爬升假告警），修复在车上验证 |

### DR-8 候选（10-23 晚新立，待排）：空模型响应被归为「agent 不配合」而非基础设施故障

- 二次部署后首跑（run a6a35d00）死于连续 4 次空模型响应：newapi/MiniMax-M3.1-Flash 连吐
  `output: 0 tokens, content: []`，引擎按「连续 4 次未输出 <doc-complete>」质量失败（retryable:false）收场。
- 双重问题：①空响应（0 token）是 provider 侧故障信号，应分类 infra 可重试，而非 agent 质量失败；
  ②失败后无任何守夜机制，run 停在 Failed 无人管。
- 临时处置（10-23 晚二段）：①doc-agent/doc-review/doc-fix 换模 —— MiniMax/deepseek-flash 先后空响应，
  逐模型捅测后锁定存活渠道 **deepseek-ai/DeepSeek-V4-Flash**（入池 + 三角色切装 + 重渲染）；
  ②操作员侧夜间哨兵（Failed 自动 resume 重启，上限 3 次）。
- 换模后验证：run e0f33987 Running、agent_age 3.3s（流式活性实时刷新）、会话 218KB 真流式。
- 正式修法（下批）：空响应/零 token → infra 分类 + 有界重试；连续多 provider 空响应才升级人工。

### DR-9 ·（高，用户拍板 10-23 21:01）日志系统终态：所有内容 + 所有行为，纯 API、可远程

- **用户原话**：「我能够通过 Win10 系统查看它的所有内容和所有行为，现在做不到是吧」「日志系统每次弄都有大坑，怎么总是缺少东西」。
- **痛的根源（自我诊断）**：历次都拿「当时的坑」当需求修一层（DR-2/3/7 修到回合级），下一次诊断掉进下一层又回头翻文件——
  因为验收标准从来没按用户的完整诉求一次性立全。本次按终态立，分层列缺口：

| 层级 | 内容 | 现状 |
|---|---|---|
| L1 run/单元/回合 | 状态/进度/心跳/活性/turn 事件 | ✅ DR-7 已建 |
| L2 步进级 | 每次工具调用：工具名/耗时/参数摘要/结果摘要，入事件流 | ❌ 今晚「为什么慢」翻文件才知道 209 次×4s |
| L3 回合间隙 | 审查判决→下一单元之间的引擎行为（那 10 分钟空转目前完全盲区） | ❌ |
| L4 会话转录 | pi 侧完整对话+工具往返经 API 可查（分页/按角色/按时段），不登机 | ❌ 审查官 61 次调用 16 万字全靠文件挖掘 |
| L5 远程可视 | 服务绑 tailnet/局域网 + token 鉴权，用户从 Win10 浏览器/客户端直接看全部上述内容 | ❌ 现绑 127.0.0.1，物理不可能 |

- **修法方向**：
  1. L2：RpcClient 收到 toolCall/toolResult 时发 `agent.step` 事件（工具名、耗时 ms、参数摘要 ≤256B），会话文件仍是真源，事件是投影；
  2. L3：引擎在判决处理/单元切换的各节点发 `engine.glue` 计时事件（阶段名+起止）；
  3. L4：新端点 `GET runs/{id}/transcript?session=&from=&limit=`——服务端代读会话 JSONL 并结构化返回（文件仍是存储，API 是唯一入口）；
  4. L5：`DEVFORGE_ADDR` 绑 tailnet IP + 现有 token 机制强制启用；文档化 Win10 访问路径（`http://<tailnet-ip>:7888` + SSE）。
- **验收（一次立全，逐层验收）**：用户不登机器、从 Win10，能回答：它在干嘛（L1）/为什么慢（L2/L3）/它刚才读了什么写了什么说了什么（L4）——全程浏览器。
- **教训入档**：可观测性需求必须按「用户的完整问题空间」立验收，不按「本次踩的坑」立范围；每层缺失都会在下一次诊断时退化成翻文件。

### DR-10 ·（中）性能四条：单元 55 分钟的成本结构

- 实测（unit 2，纯文件诊断——正是 DR-9 缺口的代价）：写 30min（209 步×4s+大文件生成）/ 审 14.5min（61 次工具全量重读）/
  引擎空转 ~10min（盲区）/ 一次性卡顿 5min。
- 治法按收益排：①审查与下一单元写作**流水线并行**（藏掉 40% 墙钟）；②审查瘦身——只喂当前单元目录树
  （008/t022 待优化项兑现）；③L3 事件落地后定位 10 分钟空转；④批量写文件压步数。
- 预期：两轮改造后单单元 55→20 分钟量级。

### 观察（非 DR，另案候选）：resume 证据口径与 mark_done 口径不一致

- 二次部署后 resume 把 unit-app-01 判「证据失效」重跑：mark_done 的门槛是审查 PASS，
  resume 的证据复核是「文件在 + lint 绿」——审查过了但 lint 不绿的单元每次续跑都会被重做。
- 本次代价：unit 1 的 ~87 分钟修复环成果被重跑覆盖（done 标记被摘）。
- 待观察：重跑后的 unit 1 若 lint 仍不绿，则说明审查官与 lint 的判据存在系统性分歧，应立案
  （审查提示词带 lint 结果注入 D012 是否足够？还是 lint 应作为单元完成的硬门？）。

**规则入档**：引擎层动刀后，L1 全 profile + 被改 SOP 的 L3，缺一不发版。

## DR-7 ·（高，用户拍板）可观测主入口必须是 API，不是登机器翻文件

- **用户原话（10-23 15:58）**：希望通过 API 接口看到 agent 到底在干什么，而不是本地查看文档；
  能看到一整条超级详细的日志记录；通过 API + 日志系统掌握所有进度。
- **现状盘点**：
  - 已有底子：`GET /api/v1/events` SSE 是全事件流（EventBus 1024 条环形缓冲 + Last-Event-ID 回放，
    gate.*/phase.*/agent.turn_end/ui.request 全在上面）；结构化 JSONL 日志已在盘上。
  - 差距一（内容粗）：agent 粒度事件只有 turn_end——没有「正在跑哪个单元/第几轮/请求发出/模型回话/
    token 用量/agent 角色」，agent 未开轮时零事件（DR-1 场景下用户看到的就是 Running + 什么都不发生）。
  - 差距二（无历史查询）：SSE 只在线 + 1024 条回放；完整历史只能读盘上 JSONL 文件——违背 API-first。
  - 差距三（无进度聚合）：list_runs 只给状态机状态；心跳数据在文件里没透出 API；
    没有 {phase, 单元 x/y, 最近 agent 事件距今, agent 活性, 轮次/累计 tokens} 的单端点概览。
- **修法方向**（DR-2/DR-3 的事件源工作之上加 API 面）：
  1. 事件加密：agent 生命周期 + 单元进度 + 轮次元数据（承接 DR-2/DR-3 的事件定义）；
  2. 新端点 `GET /projects/{p}/tasks/{t}/runs/{run}/events?from=&type=&limit=`（分页查历史，
     数据源即结构化 JSONL，绕过 1024 缓冲限制）；
  3. 新端点 run 详情聚合（进度 + 活性 + 用量，心跳数据 API 化）。
- **验收**：不登机器、只凭 API/SSE，能回答三个问题：它现在在干嘛？干到哪了？卡没卡？

- 台账待排（selfhost-findings-20261021.md）：F7 续跑边界穷尽(c8 靶) / F8 plan 延迟优化 /
  F9 宽靶约束 / F10+F13 守卫常量物化 / F11 补边界引用卫生 / F12 夹具冲突盘点节 / F14 历史裁决附录 /
  F19 文书数字随补录漂移（10-23 新立）。
- 结构缺陷未修：change SOP 的 testing→plan 路由立即 Quality、无修复环（resume6d 注册，10-23）。
- 观察项：模型单一（doc 全链只在 flash 上验证过，换模型未测）/ 用量累计口径 / 40 字 slug 截断 /
  F6 静默死亡杀手未捉到（tracepoint 天网在岗值班中）。

## 附：修复批次之外的已知遗留（立案在旧账，勿重记录）
