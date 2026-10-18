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
| P005 | [coding-stage](plans/005-coding-stage/README.md) | sop/config/server | Done | Coding SOP 全链 e2e 实证（v3 183P/0F，Settled+Completed 见证） | db262bf | —— |
| P006 | [coding-hardening](plans/006-coding-hardening/README.md) | sop/server/config | In Progress | 四项修复落地（F1/F2/P3/超时重设计），v4 e2e campaign 收官中 | 04b5d5b | TEST_REPORT v4 |
| P007 | [testing-stage](plans/007-testing-stage/README.md) | sop/config/server | Planning | 设计讨论落档，Q001-Q008 待拍板 | 2026-10-18 设计记录 | coding 收官后定稿开工 |

## Baseline

- [baseline/README.md](baseline/README.md) — 项目现状（空 Rust 仓）、五阶段构想、已拍板的全局决策

## Ideas

- [ideas/inbox.md](ideas/inbox.md) — 低承诺灵感池
