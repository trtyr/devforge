# Roadmap — P002 配置管理体系

## Done

- [x] (T001) 需求澄清：五类配置 + 三能力（自动配置/内部管理/自动更新），配置即代码取向（2026-10-16）

## Next

- [ ] (T002) 清单 schema 设计：五类配置的统一表示（Agent → 插件集/模型/Key 引用/skills/MCP），含必装四件套等公共层与 Agent 差异层的继承关系
- [ ] (T003) 渲染器：清单 → profile 目录（生成 AGENTS.md 骨架、调 pi install、写 mcp.json/models/auth、同步 skills/）
- [ ] (T004) 内部管理面：devforge 命令对清单 CRUD + 保存编辑
- [ ] (T005) 自动更新：清单变更检测 → 增量重渲染 profile
- [ ] (T006) 与 P001 spawn 链路联调：spawn 按 agent 名取 profile，全链路无手工配置

## Deferred

- 热更新（运行中 Agent 的配置生效）——视 open questions 演进
