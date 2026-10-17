# P005 · Coding 阶段：Coding Agent + Coding Review

## Scope

流水线第二个阶段 SOP：Plan 阶段产出 plan tree 后，Coding Agent 在共享黑板（任务目录）上读树、自判体量、自定 N 阶段、逐阶段开发（代码+文档伴写+单测），Coding Review Agent 逐轮审查并产出 Review Report，驱动「修复重审 / 进入下一阶段 / 全树完成收工」。

**In scope：**

- 共享黑板产物约定（任务目录 `coding/` 子树：阶段/todo/文档/report/偏差留痕）
- Coding Agent：体量自判 → N 阶段 → todo list → 按 todo 开发；上下文全程保持；文档伴写不分离；只写单测禁 e2e；plan 偏差协议（自查→搜索→问用户→留痕）
- Coding Review Agent：无状态用完即弃；后端/前端/前后端连接三面审查 + 通用刚需（AI-friendly/错误处理/审计）+ 文档可测性审查 + todo 进度审计 + 树完成判定；产出 Review Report + 结构化 verdict
- 配置供给：todo 插件（rpiv-todo）、CodeGraph MCP + context7 MCP（per-agent stdio）、四语言 skills（Rust/Go/Python/TS-JS，外部采集 vendor）
- 编排层：CodingSopLoop 状态机（阶段循环/打回/推进/树完成判定）+ SOP 注册 + 同任务跨 SOP 触发
- 单元测试 + e2e 全链验证（输入复用 demo-app 树）

**Out of scope：**

- 测试阶段 SOP——本阶段只定「文档可测性」规范（decisions/004），测试 Agent 立项归后续
- 验收/部署阶段
- Review 审查规则的运行时热更新——审查清单在使用期由人工反馈迭代，属流水线运营，非本系统当前能力
- 多任务并发调度——归 P004
- 前端——归 P003

## Affected Modules

- `sop/` — 新增 coding SOP 循环（阶段状态机）与 RPC 桥接复用
- `config/` — 渲染器扩展：mcp.json 模板、插件清单、语言 skills 分发、两份新提示词模板
- `server/` — SOP 注册表 +1（coding）；start 接口 sop 覆盖
- `engine/` — 复用（spawn/协议/台账不变）

## 文件地图

- `roadmap.md` — 任务分解与状态
- `topics/shared-blackboard.md` — 黑板布局、读写矩阵、代码工作区模型
- `topics/coding-agent.md` — Coding Agent 行为剧本（三层流程/伴写/偏差协议）
- `topics/coding-review-agent.md` — Review Agent 审查面与 Report 协议
- `topics/agent-supply.md` — todo 插件 / MCP / 语言 skills 供给与渲染器扩展点
- `topics/orchestration.md` — CodingSopLoop 状态机、与 planning 的衔接、风险
- `decisions/` — 已拍板决策
- `open-questions.md` — 未决问题
