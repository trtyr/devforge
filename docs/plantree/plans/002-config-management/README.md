# P002 · Agent 配置管理体系

## Scope

devforge 自带的 Agent 配置维护系统。统一管理五类配置，让「为 Agent 配置」变成系统内的一等操作，而不是手工摆弄 pi 配置文件。

**管理的五类配置：**

| # | 配置类 | pi 侧落点（已实研，见 P001 agent-profiles） |
|---|--------|---------------------------------------------|
| 1 | API 插件 | `pi install` → settings.json `packages` + npm/package.json 双记录 |
| 2 | Provider 模型 | settings.json `defaultProvider`/`defaultModel` + models.json |
| 3 | API Key | auth.json（明文，不设防，decisions/001） |
| 4 | Skill | `skills/` 目录 |
| 5 | MCP | mcp.json |

**三个核心能力（用户拍板 2026-10-16）：**

1. **自动配置**：系统按清单自动生成/更新 profile（清单为真源，profile 是渲染产物，可随时重建）
2. **内部管理**：在系统内为 Agent 配置这五类信息——但按 decisions/002 模板化后大幅瘦身：不做通用 CRUD，开放面只有模板编辑 + 每 Agent 模型选择
3. **自动更新**：配置变更后自动同步到 profile 产物

**Out of scope：**
- 运行中 Agent 的热更新（已定案：配置 spawn 时注入，变更对下次 spawn 生效，Q002）
- pi 全局配置（~/.pi/agent）的管理——devforge 只管自家 profile 世界
- 通用配置 CRUD 系统（decisions/002：模板化，只护模板编辑 + 模型选择两个开放面）

## 核心设计取向

**配置即代码 + 模板化**（decisions/002/003）：devforge 维护一份 Agent 配置清单（真源），profile 目录 = 渲染产物。清单分两层：

- **模板层（写死）**：提示词、必装插件四件套、skills、MCP、Key——定稿即定死，MCP 无特殊待遇就是普通一员
- **模型层（可变）**：每个 Agent 用什么模型——系统定型后唯一长期开放的自定义维度（维护收敛律）

维护节奏：现在全量可编辑（开发调试期需要）；未来定型后日常维护只剩换模型。插件版本是唯一例外（安装模式，版本随装随流）。

## 与 P001/P002/P003 的关系

- P001（编排器）spawn 时消费 P002 的产物（`~/.devforge/profiles/<agent>/`）
- P001 MVP 期间允许手工最小 profile 先行；P002 落地后接管生成与维护
- P002 的管理面（模板编辑 + 模型选择）经 P003 API 暴露，不自建交互通道（服务器形态，2026-10-16）

## 文件地图

- `roadmap.md` — 任务分解
- `decisions/README.md` — 已拍板决策（001-003）
- `open-questions.md` — 全部已结案（2026-10-16）
