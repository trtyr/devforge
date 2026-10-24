# P012 · 并行开发底座（Parallel Development Base）

> 立项：2026-10-24，用户拍板「未来默认本项目存在并行开发，这是 devforge 的底座」。
> **执行人：操作员亲自实现（不派工 devforge）**——本计划改造的是 devforge 自己的执行模型
> （调度/工作区/集成），底座手术期间系统无法可靠自举。AF/ARC 系列的派工在本底座就绪后进行。

## 背景与问题

devforge 当前执行模型是「单线流水线」：一个 task 一次一个 run，change SOP 直接在当前分支工作。
要支撑「同一项目多需求并发开发」，缺五块底座能力（工作项契约 / 工作区隔离 / 所有权防冲突 /
集成事务 / 准入控制），外加一个安全前置（agent 沙箱化）。

设计依据：2026-10-24 业界实践研究（Compound Engineering 设计模式、intentic/orcyx/Augment 等
worktree 编排产品、NP-Bench 并发协调验证）——并行开发本质是「**调度 + 合并**」问题，
不是「多开几个 agent」。详细原则见 `topics/00-overview.md`。

## Scope（做什么）

- 工作项契约：change 任务携带 owned paths / base revision / 验收证据 schema
- 工作区生命周期：每任务唯一分支 + linked worktree，pin 到已知 base，崩溃清理所有权转移
- 所有权与冲突预防：路径锁（语法）+ 契约单一 owner（语义），乐观/保留两档
- 集成事务：merge queue + 推测目标组合验证 + CAS 原子更新 + 证据重建
- 沙箱化 spawn：pi 子进程按 profile 声明进 Docker 隔离（凭证最小化）
- 依赖图调度器 + 准入控制：按 DAG 与下游容量派工，量串行分数

## Non-goals（不做什么）

- 远程分布式 worker（先单机多 worktree，网络调度后议）
- 非 git 仓库
- UI 面板（纯 API 复用现有 events/详情端点）
- agent 间自由对话协调（worker 只通过 durable artifacts 与状态机通信）

## 文件地图

- `roadmap.md` — 里程碑 M0-M5 与任务状态
- `topics/00-overview.md` — 底座原则、失败模式表、五特征映射
- `topics/01-work-item-contract.md` — 工作项契约（schema 与载体）
- `topics/02-workspace-lifecycle.md` — worktree 生命周期与崩溃恢复
- `topics/03-ownership-conflicts.md` — 所有权模型与五级冲突
- `topics/04-integration-queue.md` — 集成事务：merge queue + CAS + 组合验证
- `topics/05-sandbox-spawn.md` — 沙箱化 spawn（pi containerization 集成）
- `topics/06-scheduler-admission.md` — 依赖图调度与准入控制
- `open-questions.md` — 待用户拍板的 Q1-Q5

## 与既有资产的关系

- AF/ARC 系列（ai-friendly-findings / arch-ai-friendly）：派工批次依赖本底座的隔离与调度能力；
  其中 ARC-R1（删 env 开关）是本计划的前置简化。
- plantree 依赖图：批 A/B/C/D 手工聚类是调度器的第一份输入数据。
- 现有可复用件：runstate 断点、事件流、断路器、doc-lint/coverage/测试（组合验证工具箱）、
  失活收割 watchdog（worktree 化后语义需复审）。
