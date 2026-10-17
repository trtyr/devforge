# Open Questions — P005

## Q001 · rpiv-todo 在 pi `--mode rpc` 下的真实可用性？

todo 插件是 TUI overlay 形态，而 devforge 的 agent 全部跑 `--mode rpc`（无 TUI）。overlay 大概率不渲染，但 todo 的**数据面**（模型可调用的 todo 工具、状态落盘文件）是否在 RPC 模式下照常工作，未验证。

- 若可用：直接采用（方向感 + 状态文件可被 review 审计）
- 若不可用：退「todo.md 落黑板 + 提示词纪律」纯文件方案（零插件依赖）

验证时机：T003/T008 实现与 e2e 期。不阻塞设计。

## Q002 · codegraph 多实例 FileLock 争用实测表现？

设计上并发 agent 各自拉起 codegraph 实例共享同一 `.codegraph/` SQLite 索引，FileLock 仲裁。真实并发（coding agent 干活 + review agent 同时查询）下的锁等待/冲突表现未知。e2e（T008）观察；若出现实质争用，再评估单例桥（Deferred，不预设）。

## Q003 · pi 内置 web tools 在渲染 profile 的 RPC agent 上是否可用？

偏差协议的「搜索求证」环节依赖 web 搜索/抓取。pi 内置 web tools 在 TUI 会话可用，但在 devforge 渲染的 profile + `--mode rpc` 组合下是否照常暴露，未验证（同类：Q001 todo 插件）。不可用则评估补 pi-web-access 等市场插件。验证时机：T003/T008。

## Q004 · CodeGraph 索引路径与 agent cwd 的错位？【已解决 2026-10-18】

agent 的 spawn cwd 是黑板（任务目录），`.codegraph/` 索引建在代码工作区（项目 local_path）——server 根目录无索引时工具是否可用？

**结论：官方原生支持，设计成立。**codegraph 官方有无根索引变体（SERVER_INSTRUCTIONS_NO_ROOT_INDEX）：server 起在无索引目录时工具照常暴露，靠 `projectPath` 逐项目查询（解析最近 `.codegraph/`，一会话多项目），新索引被运行中 server 实时拾取。落法：提示词教 agent 显式传 `projectPath=<代码工作区>`（T004/T005 提示词落地）；引擎 spawn 前预检 init（decisions/013）保证索引存在；不可用时 agent 优雅降级原生工具。详见 topics/codegraph-strategy.md。

## Q005 · pi-lens（LSP/诊断层）是否入选？【已解决 2026-10-18】

生态侦察发现代码上下文有三大家族（结构图/LSP 诊断/语义检索），已覆盖结构与文档两类，缺**诊断层**。候选 pi-lens。

**结论：入选（用户拍板）。**supply 五件套定型（decisions/014）。待验：`--mode rpc` 下工具可用性（T003，同类 Q001）；与 codegraph 工具面重叠度（预期互补：图=结构真相，lens=编译器级反馈）。不入选的代价已消除：诊断不再依赖手动跑构建。

## 执行期验证记录（2026-10-18 夜，T007 回写）

- **Q001/Q003/Q005 部分验证**：RPC 单点冒烟（evidence/rpc-smoke.md）——手工 profile（双插件+双 MCP）下 `pi --mode rpc` 零 stderr、RPC 应答正常；**工具级可见性（todo/lens 工具注册、MCP 连接、web tools 暴露）未验，归 T008**。
- **planning review cwd 观察项（新）**：planning 的 review 实例沿用旧构造 cwd=服务器 cwd（历史行为，e2e 已验证能找到树）；coding 的审查实例改用 per_round_in 显式 cwd=黑板。planning 是否迁移到 plan root 留观（不动已验证路径）。
- **执行期新发现（记录不阻塞）**：①edit 工具批量操作存在部分落盘非原子现象（task.rs 两轮修复实证）——同一文件多处修改改用单次小编辑；②coding_review_instruction 轮次号从 1 起（MockReviewFactory 以 opened.len() 计数，push 后即当轮号）。
