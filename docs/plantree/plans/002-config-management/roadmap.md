# Roadmap — P002 配置管理体系

## Done

- [x] (T001) 需求澄清：五类配置 + 三能力（自动配置/内部管理/自动更新），配置即代码取向（2026-10-16）

## Next

- [ ] (T002) 模板与清单 schema 设计：模板层（提示词/必装插件/skills/MCP/Key，写死）+ 模型层（每 Agent 可变），两层分离（decisions/002/003）
- [ ] (T003) 渲染器：清单 → profile 目录（生成 AGENTS.md 骨架、调 pi install、写 mcp.json/models/auth、同步 skills/）
- [ ] (T004) 管理面：模板编辑 + 每 Agent 模型选择（不做通用 CRUD，只护住这两个开放面）
- [ ] (T005) 自动更新：清单变更检测 → 增量重渲染 profile（开发调试期工具，定型后自然闲置）
- [ ] (T006) 与 P001 spawn 链路联调：spawn 按 agent 名取 profile，全链路无手工配置

## Deferred

- 热更新（运行中 Agent 的配置生效）——视 open questions 演进
