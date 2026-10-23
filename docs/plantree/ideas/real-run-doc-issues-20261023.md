# 真实项目文档首跑问题清单（2026-10-23）

> 场次：cloudwalker-agent · d1 · doc-from-zero（run 31eee16d，14:01 开跑）
> 背景：e2e 玩具仓全绿后首次真实仓（941 条目 / 34 单元 / 11 顶层区）实战。开跑 50 分钟后操作员巡检发现问题。
> 处置：本轮只立案不修，下次更新统一修（用户拍板 15:53）。

## DR-1 ·（致命）doc-agent 被拉起为裸 pi REPL，单元写作从未开始

- **现象**：run 显示 Running、心跳鲜活，但 50 分钟零产出、零事件。
- **证据**：
  - `ps`：pid 2619560（devforge 2619452 的子进程，14:01 起）命令行**只有 `pi` 一个词**——没有 `--mode rpc`、没有 `--session-dir`、没有 prompt；
  - stdin/stdout/stderr 全是指向 devforge 的管道（fd 0/1/3 同一管道对），agent 在等 stdin 输入；
  - 50 分钟累计 CPU 仅 0:18、**零网络连接**（没碰任何模型 API）、doc-agent profile 下**零会话文件**；
  - 服务端日志只有 `phase.change doc 单元推进`（14:01:45）一条，之后 50 分钟静默。
- **对照**：`build_spawn_command`（src/engine/process.rs:53）本应产出 `pi --mode rpc --session-dir <dir>` + `PI_CODING_AGENT_DIR=<profile>` + `DEVFORGE_MANAGED=<run>`；server/mod.rs:480 的 doc 路径也是走 `RpcClient::spawn(SpawnSpec::new(doc-agent…))`。
- **根因候选（修时必答）**：
  1. release 安装二进制（11:07 cargo install）里的 doc spawn 路径与工作树不一致（陈旧分支？）；
  2. doc SOP 存在第二 spawn 路径绕过了 build_spawn_command；
  3. RpcClient::spawn 之前有某个 doc 专属构造点参数为空。
- **验收**：重跑后 doc-agent 进程命令行含 `--mode rpc --session-dir`；会话文件出现；首单元 ≤15 分钟落盘。

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
