# Roadmap — P005 Coding 阶段

## Phase A · 设计定稿

- [x] (T001) 需求消化 + 外部侦察（CodeGraph/context7/todo 插件市场）+ 四口子拍板（CG 生命周期/代码工作区/e2e 输入/测试边界）→ P005 plan tree 落档 + 根登记更新（2026-10-18）

## Phase B · 实现

- [x] (T002) 共享黑板产物约定与结构化协议扩展：`coding/` 子树布局、阶段完成标记块（含无标记重推语义）、Review Report schema（verdict 家族扩展，PASS 可附非阻塞建议）；单测钉协议解析（13 条含缺块保守/语义矛盾）——commit b61e07a
- [x] (T003) 配置供给：渲染器 mcp.json 模板（codegraph stdio + context7 npx、遥测关闭、空则不生成）、插件清单（rpiv-todo + **pi-lens 诊断**）、四语言 skills vendor 入库与分发（0xMassi rust/go/ts/js + Bible python 全目录，来源登记 evidence/skills-sources.md，编译期嵌入全量分发）；RPC 单点冒烟入档（evidence/rpc-smoke.md，pi 零 stderr 启动）——commit 2737fe6
- [x] (T004) Coding Agent profile：提示词正稿（三层流程/todo 纪律/伴写纪律/单测禁 e2e/git 阶段 PASS 即 commit/勘察义务/偏差协议/刚需四条款/codegraph-first 含场景矩阵与 projectPath/双路径 kickoff 告知/打回重审/notes 消化）——commit aaac3d2；渲染单测协议与纪律锚点覆盖
- [x] (T005) Coding Review Agent profile：七审查面提示词正稿（刚需/后端/前端/连接/文档可测性/todo 实物审计/树完成锚点对照实物）+ Report 结构化输出协议（与 T002 schema 一致）——commit aaac3d2；渲染单测断言
- [x] (T006) 编排层：CodingSopLoop 状态机（干活→标记→review→fix 重审|advance→finish）+ SOP 注册 + 同任务跨 SOP start 触发（可选 sop 覆盖）+ 工作目录模型（黑板 cwd + 代码=项目 local_path）+ CodeGraph 引擎预检 init——commit 22c96e0；状态机/注册/跨 SOP/预检/分发拒绝单测齐

## Phase C · 测试与验收

- [x] (T007) 全量回归：cargo test **120 passed / 0 failed**（+1 doctest）+ clippy --all-targets **0 警告**（bg_run 原始日志验真，2026-10-18 晨）；执行期发现回写 open-questions（Q001/Q003/Q005 冒烟态 + planning review cwd 观察项）；commit 见 git log
- [x] (T008) e2e 全链：demo-app plan tree 输入 → coding agent 分阶段开发 → review 循环（观察到真实打回则验证修复循环；直 PASS 亦有效）→ 树完成 → run Settled + 任务 Completed；观察 codegraph 多实例 FileLock 表现 + Q001/Q003/Q005 的 RPC 工具级可见性。（**证据**：`evidence/t008-coding-full-chain.md`——2026-10-18 全供给真跑 25 PASS/0 FAIL，七轮 review 真实打回循环，四阶段 commit，Q001/Q003/Q005 工具调用实证；Q002 FileLock 零冲突事件与局限已诚实登记）

## Deferred

- 测试阶段 SOP（吃 coding/ 文档写测试）——独立立项
- Review 审查清单运行时热更新——流水线运营期人工迭代
- 系统级 CodeGraph 单例桥——仅当 per-agent 多实例在实测中出现索引争用问题才评估
