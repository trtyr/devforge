# DevForge Plantree

DevForge：AI 自动化开发流水线（编排平台）。核心思路是把「从 0 到 MVP」写成剧本——五个阶段（规划 → 开发 → 测试 → 验收 → 部署），每个阶段是对一组 Agent 的编排。

## 阅读顺序

1. `baseline/README.md` — 项目现状与全局背景（先读这个）
2. `plans/<NNN>-<plan>/README.md` — 具体计划的 scope 与文件地图
3. 计划内的 roadmap / topics / decisions / open-questions

## Authority Order

- 全局背景与现状 → `baseline/`
- Plan 身份与生命周期 → 本文件注册表
- Plan scope → 各 Plan 的 `README.md`
- 任务状态 → 各 Plan 的 `roadmap.md`
- 稳定决策 → 各 Plan 的 `decisions/`

## Active Plans

| ID | Plan | Affected Modules | Status | Current Phase | Last Landed | Next Target |
|----|------|------------------|--------|---------------|-------------|-------------|
| P001 | [planning-stage](plans/001-planning-stage/README.md) | 编排器（未建） | In Progress | 编排设计定稿 | 2026-10-16 立项建档 | 设计过目确认 |
| P002 | [config-management](plans/002-config-management/README.md) | 配置管理（未建） | Planning | 立项建档 | 2026-10-16 立项建档 | 清单 schema 设计 |

## Baseline

- [baseline/README.md](baseline/README.md) — 项目现状（空 Rust 仓）、五阶段构想、已拍板的全局决策

## Ideas

- [ideas/inbox.md](ideas/inbox.md) — 低承诺灵感池
