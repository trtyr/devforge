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

## Q004 · CodeGraph 索引路径与 agent cwd 的错位？

agent 的 spawn cwd 是黑板（任务目录），`.codegraph/` 索引建在代码工作区（项目 local_path）——`serve --mcp` 无 path 时按客户端 rootUri 找索引，会扑空。设计倾向：提示词教 agent 调用 `codegraph_explore` 时显式传 `projectPath=<代码工作区>`（工具原生支持跨项目查询）；profile 模板不写死路径（profile 全局、项目路径项目级，渲染时不可知）。执行期验证展开方式，若 pi 客户端有更好的 rootUri 机制则简化。
