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

## 修复批次建议（下次更新）

1. **DR-1 根因 + 修复**（一切的前提；修完本地冷跑一次单单元冒烟）；
2. **DR-2 + DR-3**（可观测补齐，一次动刀：agent 生命周期事件 + 心跳活性字段 + 停滞告警）；
3. **DR-4 + DR-5**（产出节奏守卫 + 进度指标）；
4. 全部修完 → 重启服务 → relaunch cloudwalker d1（当前 run 是僵局，可先停掉）→ 全程盯首单元产出确认闭环。
