# Open Questions — P005

## Q001 · rpiv-todo 在 pi `--mode rpc` 下的真实可用性？

todo 插件是 TUI overlay 形态，而 devforge 的 agent 全部跑 `--mode rpc`（无 TUI）。overlay 大概率不渲染，但 todo 的**数据面**（模型可调用的 todo 工具、状态落盘文件）是否在 RPC 模式下照常工作，未验证。

- 若可用：直接采用（方向感 + 状态文件可被 review 审计）
- 若不可用：退「todo.md 落黑板 + 提示词纪律」纯文件方案（零插件依赖）

验证时机：T003/T008 实现与 e2e 期。不阻塞设计。

## Q002 · codegraph 多实例 FileLock 争用实测表现？

设计上并发 agent 各自拉起 codegraph 实例共享同一 `.codegraph/` SQLite 索引，FileLock 仲裁。真实并发（coding agent 干活 + review agent 同时查询）下的锁等待/冲突表现未知。e2e（T008）观察；若出现实质争用，再评估单例桥（Deferred，不预设）。
