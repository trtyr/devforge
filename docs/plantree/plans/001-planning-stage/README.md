# P001 · 规划阶段：Plan Agent + Review 循环

## Scope

实现流水线第一个阶段（规划阶段）的编排：两个 Agent（Plan Agent / Plan Review Agent）基于 pi 底座协作，从用户意图出发产出语义自洽的 plan tree，存到系统级目录。

**In scope：**
- 编排器骨架（spawn pi 子进程、独立配置注入、生命周期管理）
- 进程身份与 spawn 台账（decisions/007：只管自家进程，禁止扫全系统杀 pi）
- 基础架构：完整错误处理 + 结构化日志（baseline/architecture-principles.md，验收标准的一部分）
- Plan Agent：与用户沟通，产出 plan tree 到 `~/.devforge/plans/`
- Plan Review Agent：检查 plan tree 语义一致性（仅语义通顺，不管潜在坑）
- Review 循环：不过 → 扔回 Plan Agent 修 → 新 Review 重查，直至通过
- 保活机制：干活 Agent（Plan Agent）在阶段完成前不死、保留完整上下文

**Out of scope：**
- 开发/测试/验收/部署四个阶段（复用本阶段结论，后续立项）
- API 接口体系与前端——归 P003（本 plan 只保证骨架按服务器形态写，接口设计不在此展开）
- 项目/任务/SOP 三层对象管理——归 P004（本 plan 的编排引擎是单 run 引擎；多任务并发调度、项目目录绑定等归 P004）
- 多项目并发编排（MVP 单任务串行；run-id 身份体系不禁止未来并发）
- 任何形式的硬限制（token/step/轮次熔断）——见 decisions/003

## Affected Modules

（代码未建，本 plan 就是第一个模块的出生地——落地后回填 baseline module map）

- `orchestrator`（未建）— spawn / 生命周期 / 消息传递 / 阶段状态机
- `agent-profiles`（未建）— plan-agent 与 plan-review 的独立 pi 配置目录

## 文件地图

- `roadmap.md` — 任务分解与状态
- `topics/orchestrator-design.md` — 编排器核心设计（状态机、保活、消息传递）
- `topics/agent-profiles.md` — 两类 Agent 的 pi 配置隔离方案
- `topics/review-loop.md` — Review 循环语义与迭代流程
- `decisions/` — 已拍板决策
- `open-questions.md` — 未决问题
