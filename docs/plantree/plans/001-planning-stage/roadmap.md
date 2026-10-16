# Roadmap — P001 规划阶段

## Done

- [x] (T001) 需求澄清：底座选型（复用 pi）、存储位置（~/.devforge/plans/）、无硬限制哲学（2026-10-16）
- [x] (T002) 验证配置隔离机制：`PI_CODING_AGENT_DIR` 可覆写 pi 配置目录（2026-10-16）
- [x] (T004) 保活/消息机制调研定案：RPC 模式（`pi --mode rpc`），先例 pi-agents-team（2026-10-16，见 open-questions Q001）

## In Progress

- [ ] (T003) 编排器设计定稿：状态机 + 保活 + 消息传递方案（机制已定 RPC，剩 Q002/Q004 协议细节）

## Next

- [ ] (T005) 设计两类 Agent 的独立配置目录结构（agent-profiles）
- [ ] (T010) 基础架构选型：错误处理方案（thiserror/anyhow 类）+ 结构化日志方案（tracing 类），回填 architecture-principles.md
- [ ] (T006) 落地 orchestrator 骨架：spawn Plan Agent（含身份标记 + spawn 台账）→ 用户交互产出 plan tree → 保活暂停；错误处理与日志随骨架落地，不许后补
- [ ] (T007) 落地 Review Agent：读取 plan tree + skills，输出通过/问题清单
- [ ] (T008) 落地 Review 循环：问题清单回注 Plan Agent → 修复 → 新 Review 重查
- [ ] (T009) 端到端跑通：从一句需求到一份通过 review 的 plan tree

## Deferred

- （空——四阶段扩展不进本 plan）
