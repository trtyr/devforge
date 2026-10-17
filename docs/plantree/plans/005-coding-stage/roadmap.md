# Roadmap — P005 Coding 阶段

## Phase A · 设计定稿

- [x] (T001) 需求消化 + 外部侦察（CodeGraph/context7/todo 插件市场）+ 四口子拍板（CG 生命周期/代码工作区/e2e 输入/测试边界）→ P005 plan tree 落档 + 根登记更新（2026-10-18）

## Phase B · 实现

- [ ] (T002) 共享黑板产物约定与结构化协议扩展：`coding/` 子树布局、阶段完成标记块（含无标记重推语义）、Review Report schema（verdict 家族扩展，PASS 可附非阻塞建议）；单测钉协议解析
- [ ] (T003) 配置供给：渲染器 mcp.json 模板（codegraph stdio + context7 npx）、插件清单扩展（rpiv-todo）、四语言 skills vendor 入库与分发
- [ ] (T004) Coding Agent profile：提示词模板（三层流程/todo 纪律/伴写纪律/单测纪律/偏差协议/刚需架构条款/codegraph-first 纪律含场景矩阵与 projectPath 传参/勘察义务）+ settings/auth 沿用 planning 渲染管线
- [ ] (T005) Coding Review Agent profile：审查提示词（三面+刚需+文档可测性+进度审计+树完成判定/codegraph-first 审查纪律）+ Report 结构化输出协议
- [ ] (T006) 编排层：CodingSopLoop 状态机（干活→标记→review→fix 重审|advance→树完成）+ SOP 注册 + 同任务跨 SOP start 触发 + 工作目录模型（黑板 cwd + 代码=项目 local_path）+ **CodeGraph 索引引擎预检**（spawn 前无 .codegraph 则 init，decisions/013）

## Phase C · 测试与验收

- [ ] (T007) 单元测试：状态机轮转/协议解析（含缺块保守路径）/渲染器扩展/跨 SOP 触发——cargo test 0 失败 + clippy 0 警告
- [ ] (T008) e2e 全链：demo-app plan tree 输入 → coding agent 分阶段开发 → review 循环（观察到真实打回则验证修复循环；直 PASS 亦有效）→ 树完成 → run Settled + 任务 Completed；观察 codegraph 多实例 FileLock 表现。注：coding 链时长预期显著高于 planning 链（真写代码+跑单测），中转长 turn 风险敞口更大，超时/取消面沿用 P001 机制

## Deferred

- 测试阶段 SOP（吃 coding/ 文档写测试）——独立立项
- Review 审查清单运行时热更新——流水线运营期人工迭代
- 系统级 CodeGraph 单例桥——仅当 per-agent 多实例在实测中出现索引争用问题才评估
