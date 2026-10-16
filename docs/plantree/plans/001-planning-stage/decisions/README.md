# Decisions — P001

稳定决策记录。只记「选了什么 + 为什么 + 后果」，过程讨论留在 topics。

---

## 001 · 底座复用 PI Agent

- **决策**：不自写 Agent runtime。devforge = 编排器，底座用现成 pi：spawn pi 子进程 + 注入独立配置 + 管生命周期
- **为什么**：pi 已有会话循环/工具系统/技能系统/配置体系，自写轻量 Agent 要重造这些，MVP 周期白拉长
- **后果**：devforge 的技术重心是「编排」（进程管理/消息/状态机），Agent 能力全部寄生在 pi 的配置体系上

## 002 · Plan tree 存 `~/.devforge/plans/`

- **决策**：plan tree 跟系统走，不跟目标项目走。所有被规划的项目的 plan tree 统一放 `~/.devforge/plans/<plan-name>/`
- **为什么**：这是 devforge 系统的工作产物，不是目标项目的资产；系统级统一便于跨项目检索与后续阶段的衔接
- **后果**：与 pi plan-tree 技能的默认约定（目标项目 `docs/plantree/`）不同——devforge 场景下编排器显式指定输出路径，目标项目仓库不被污染

## 003 · 零硬限制哲学

- **决策**：全系统禁做硬限制——无 token 上限、无 step 上限、无 review 轮次熔断
- **为什么**（他原话）：对现在的模型做限制治理完全是错误做法；限制是傻逼行为，信任模型自己收敛
- **后果**：Review 循环理论上可无限轮；成本与时长不设防。若未来出现失控场景，解法方向是改进 prompt/剧本，不是加轮次上限

## 004 · Review Agent 无状态、用完即弃

- **决策**：每轮 review 都是全新 pi 实例，从头读 plan tree；发现问题时输出清单后即死，不存活到修复完成
- **为什么**（用户对初稿的修正）：review 是检查不是干活，完事了就死；干活者（Plan Agent）才需要阶段内存活。无状态重查避免上轮判断污染新轮审查
- **后果**：修复后的重查是全局重新审查，不是增量复查——更强保证，代价是每轮重复读上下文（接受，零限制哲学下不算成本）

## 005 · Agent 通道用 pi RPC 模式（Q001 定案）

- **决策**：所有子 Agent = `pi --mode rpc` 长驻子进程，devforge（Rust）经 stdin/stdout JSONL 协议控制；保活/唤醒/终止分别映射 `agent_settled` 事件 / 同进程再 prompt / 关 stdin
- **为什么**：SDK 需 Node/Bun 宿主，Rust 不可用；print/JSON 是一次性进程无保活；RPC 官方定位就是「其他语言编排」，且 pi-agents-team（2026-10-11）已用同构架构验证可行，pi 官方仓库另有 experimental orchestrator 背书
- **后果**：devforge 是 RPC 协议客户端（协议参考 pi docs rpc.md / rpc-commands.md）；子 Agent 无 TUI，用户交互面归 devforge 中继（Q003）；Review 结果走 `get_last_assistant_text` + 结构化 verdict 块（Q002/Q004）。备选方案「pi extension 型编排器（TypeScript，同 pi-agents-team）」被否：devforge 定位是独立编排平台而非 pi 插件

## 006 · Agent 配置归属系统

- **决策**：所有 Agent 的 pi 配置（profile）统一放 `~/.devforge/profiles/<agent-name>/`，跟系统走、不跟项目走；每个 Agent 一个专属文件夹，未来新增 Agent（开发/测试/部署阶段）都在此注册
- **为什么**（用户拍板）：系统需要有个统一的地方为所有 Agent 配置；这是 Agent 的身份，不是某个项目的资产——与 decisions/002（plans 归系统）同一逻辑
- **后果**：编排器 spawn 时按 agent 名字找 profile 目录；profile 目录结构（AGENTS.md/skills/settings）是 devforge 的公共约定

## 007 · 进程身份红线：只管自家进程

- **决策**：devforge 对 pi 进程的管理只能基于自家身份体系：spawn 时注入 `DEVFORGE_MANAGED=<run-id>` + `DEVFORGE_ROLE=<role>` 环境变量，PID 记入 spawn 台账，清理只按台账精确执行（PID + 进程组），重启先对账双验证（PID 活着 + 环境变量有自家标记）
- **为什么**（用户拍板）：devforge 和用户共用机器、共用 pi——编排器绝不能「扫进程看到 piagent 就全清」，那会误杀用户自己的 pi。它必须始终清楚哪些是它启动的、哪些是用户的
- **后果**：全系统扫描式进程清理被禁止（架构级红线，见 baseline README）；孤儿处理已拍板（2026-10-16）：识别后直接杀，预防靠 ledger 实时落盘 + 退出时主动清理（Q005）
