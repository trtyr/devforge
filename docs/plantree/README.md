# DevForge Plantree

DevForge：AI 自动化开发流水线——**服务器形态的编排平台**（非 CLI），核心思路是把「从 0 到 MVP」写成剧本——五个阶段（规划 → 开发 → 测试 → 验收 → 部署），每个阶段是对一组 Agent 的编排；所有能力经 API 接口体系对外（P003）。

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
| P001 | [planning-stage](plans/001-planning-stage/README.md) | sop/server/config/engine | Done | 已闭环（2026-10-18，第 22 试全链通关+收官清单零待定） | bdca305 收官闭环 | 测试阶段立项时复用其结论 |
| P002 | [config-management](plans/002-config-management/README.md) | config | Done | 随 P001 落地（src/config 渲染器+清单） | 991acaf | —— |
| P003 | [api-server](plans/003-api-server/README.md) | server | Done | 随 P001 落地（src/server 五能力域） | 991acaf | —— |
| P004 | [project-task-sop](plans/004-project-task-sop/README.md) | 三层对象管理 | Planning | CRUD+单 run 引擎已随 P001 落地；并发调度未动 | —— | 多任务并发调度设计 |
| P005 | [coding-stage](plans/005-coding-stage/README.md) | sop/config/server | In Progress | 设计定稿（四口子已拍板），待用户过目开工 | 2026-10-18 plan tree 落档 | T002 黑板约定与协议扩展 |

## Baseline

- [baseline/README.md](baseline/README.md) — 项目现状（空 Rust 仓）、五阶段构想、已拍板的全局决策

## Ideas

- [ideas/inbox.md](ideas/inbox.md) — 低承诺灵感池
