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
| P001 | [planning-stage](plans/001-planning-stage/README.md) | 编排器（未建） | In Progress | 设计已定稿，待开工 | 2026-10-16 设计定稿（T001-T004, T003） | T005 profile 目录设计 |
| P002 | [config-management](plans/002-config-management/README.md) | 配置管理（未建） | Planning | 立项建档 | 2026-10-16 立项建档 | 清单 schema 设计 |
| P003 | [api-server](plans/003-api-server/README.md) | API 接口体系（未建） | Planning | 立项建档 | 2026-10-16 形态拍板（服务器非 CLI） | 协议与风格选型 |
| P004 | [project-task-sop](plans/004-project-task-sop/README.md) | 三层对象管理（未建） | Planning | 立项建档 | 2026-10-16 三层模型拍板 | 数据模型设计 |

## Baseline

- [baseline/README.md](baseline/README.md) — 项目现状（空 Rust 仓）、五阶段构想、已拍板的全局决策

## Ideas

- [ideas/inbox.md](ideas/inbox.md) — 低承诺灵感池
