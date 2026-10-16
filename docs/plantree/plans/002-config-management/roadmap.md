# Roadmap — P002 配置管理体系

## Done

- [x] (T001) 需求澄清：五类配置 + 三能力（自动配置/内部管理/自动更新），配置即代码取向（2026-10-16）
- [x] (T002) 清单 schema 定版 + 内置 planning 清单：ConfigManifest（模板层/模型池/agents/api_key），四件套原文照录 + ask-user-question 差异化 + DeepSeek flash 模型池（src/config/manifest.rs，2026-10-16）
- [x] (T003) 渲染器落地：Renderer 清单→profile 目录（AGENTS.md/settings 骨架/skills 复制或占位/auth.json 明文），安装桥接（pi install 命令构造 + FakeInstaller 模拟双记录）——单测不依赖真实网络（src/config/render.rs + installer.rs，2026-10-16）

- [x] (T004) 管理面：数据层+API 端点（GET/PUT /api/v1/config/manifest 整份替换=模板编辑+模型选择最简形态，PUT 即重渲染）——已完成 2026-10-16
- [x] (T005) 自动更新：PUT manifest → 全量重渲染 profile（src/server/handlers.rs put_manifest，最简形态）
- [x] (T006) 与 P001 spawn 链路接缝：PlanningRpcEngine 按 agent 名取 profile（src/server/mod.rs）；端到端验证归测试阶段

## Next

- （开发阶段完成——细粒度模板编辑端点、钥匙串等远期项见 decisions）

## Deferred

- 热更新（运行中 Agent 的配置生效）——视 open questions 演进
