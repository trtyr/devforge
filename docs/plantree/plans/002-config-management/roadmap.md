# Roadmap — P002 配置管理体系

## Done

- [x] (T001) 需求澄清：五类配置 + 三能力（自动配置/内部管理/自动更新），配置即代码取向（2026-10-16）
- [x] (T002) 清单 schema 定版 + 内置 planning 清单：ConfigManifest（模板层/模型池/agents/api_key），四件套原文照录 + ask-user-question 差异化 + DeepSeek flash 模型池（src/config/manifest.rs，2026-10-16）
- [x] (T003) 渲染器落地：Renderer 清单→profile 目录（AGENTS.md/settings 骨架/skills 复制或占位/auth.json 明文），安装桥接（pi install 命令构造 + FakeInstaller 模拟双记录）——单测不依赖真实网络（src/config/render.rs + installer.rs，2026-10-16）

## In Progress

- [ ] (T004) 管理面：数据层已有（manifest 结构）；编辑/选择接口归 P003 API 端点（T007 挂接时实现）

## Next

- [ ] (T005) 自动更新：清单变更检测 → 增量重渲染 profile（开发调试期工具，定型后自然闲置）
- [ ] (T006) 与 P001 spawn 链路联调：spawn 按 agent 名取 profile，全链路无手工配置（端到端验证归测试阶段，代码接缝本阶段完成）

## Deferred

- 热更新（运行中 Agent 的配置生效）——视 open questions 演进
