# P007 · Testing 阶段：Testing Agent + Testing Review

> 状态：**定稿（2026-10-18）**——Q001-Q009 全部关账，决策 D001-D012 落档 decisions/，roadmap 已定稿；待用户发令开工。

## Scope

流水线第三个阶段 SOP：Coding 阶段完成后，基于三产物（已完成的 plan tree、通过 review 的代码、coding 落地的文档），**Testing Agent 不看代码、只看文档**（以及可选的 plan tree）编写集成测试与端到端测试，自跑自收；Testing Review Agent 依据文档判定测试是否真实反映业务关系、覆盖用户场景与极端用户面。架构与前两阶段完全同构（work Agent + per-round fresh review + 黑板 + 状态机编排）。

**In scope（草案，随 open-questions 拍板收敛）：**

- 测试阶段 SOP：`TESTING_SOP` 注册 + 跨 SOP 触发（planning → coding → testing 链）
- 黑板 `testing/` 布局与协议（套件清单/运行报告/追溯矩阵/偏差留痕）
- Testing Agent 行为剧本：文档驱动写测试（不看代码）、自起被测系统、自跑自收、可写度分级与「写不了」留痕移交
- Testing Review Agent：对文档三判据（业务关系/用户场景/极端用户面）+ 覆盖判定操作化（追溯矩阵候选）
- 红灯归因路由与 review FAIL 双根因路由（D001/D003：归因权归 coding agent，`<reroute>` 协议标签工具侧路由）
- doc-fix 子 SOP（D004：写文档模式 + 内置 doc-review 审查轮）与 doc-review 新 profile（D008）
- findings 台账（D009：testing/findings/ 一条一文件+状态机）
- 文档质量门（D008）：coding review 增「照文档从零 boot」门禁 + Runbook 七件套契约
- 供给：test-agent + test-review 双 profile（提示词纪律与 coding 反转；MCP/插件取舍 pending Q008）
- 单测 + e2e 验证（test_17+ 系列脚本）

**Out of scope：**

- 单元测试——归 coding 阶段（两阶段测试组合互补不重叠）
- 验收/部署阶段
- 多任务并发调度——归 P004

## Affected Modules（预估）

- `sop/` — 新增 testing SOP 循环 + 协议解析（复用 coding_protocol 形制）
- `config/` — 两份新提示词模板 + 双 profile 供给（复用渲染器）
- `server/` — SOP 注册表 +1（testing）；start 跨 SOP 触发复用
- `engine/` — 复用（spawn/协议/台账/watchdog 不变）

## 文件地图

- `roadmap.md` — 任务分解（定稿 T001-T008，M1-M3 里程碑）
- `topics/testing-stage-design.md` — 设计讨论全记录（2026-10-18 用户口径 + 架构映射 + 差距分析 + 三轮裁定）
- `open-questions.md` — Q001-Q009 全关账存档
- `decisions/README.md` — D001-D012 决策全文
