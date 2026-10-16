# P002 · Agent 配置管理体系

## Scope

devforge 自带的 Agent 配置维护系统。统一管理五类配置，让「为 Agent 配置」变成系统内的一等操作，而不是手工摆弄 pi 配置文件。

**管理的五类配置：**

| # | 配置类 | pi 侧落点（已实研，见 P001 agent-profiles） |
|---|--------|---------------------------------------------|
| 1 | API 插件 | `pi install` → settings.json `packages` + npm/package.json 双记录 |
| 2 | Provider 模型 | settings.json `defaultProvider`/`defaultModel` + models.json |
| 3 | API Key | auth.json（敏感，落盘需控权限） |
| 4 | Skill | `skills/` 目录 |
| 5 | MCP | mcp.json |

**三个核心能力（用户拍板 2026-10-16）：**

1. **自动配置**：系统按清单自动生成/更新 profile（清单为真源，profile 是渲染产物，可随时重建）
2. **内部管理**：在系统内直接为 Agent 配置这五类信息，支持保存与编辑（CRUD），不要求用户手工编辑 pi 文件
3. **自动更新**：配置变更后自动同步到 profile 产物

**Out of scope：**
- 运行中 Agent 的热更新（默认：配置在 spawn 时注入，变更对下次 spawn 生效；热更新为后续候选，见 open questions）
- pi 全局配置（~/.pi/agent）的管理——devforge 只管自家 profile 世界

## 核心设计取向

**配置即代码**：devforge 维护一份 Agent 配置清单（真源），profile 目录 = 渲染产物。改清单 → 重新渲染 → profile 更新。产物可删可重建，真源唯一。

## 与 P001 的关系

- P001（编排器）spawn 时消费 P002 的产物（`~/.devforge/profiles/<agent>/`）
- P001 MVP 期间允许手工最小 profile 先行；P002 落地后接管生成与维护

## 文件地图

- `roadmap.md` — 任务分解
- `open-questions.md` — 未决问题
