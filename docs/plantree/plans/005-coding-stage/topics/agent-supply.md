# Topic · Agent 供给（插件 / MCP / Skills）

Coding Agent 与 Review Agent 共用同一配置基面（Review = Coding 换提示词），由渲染器分发。

## Todo 插件

- 选型：`@juicesharp/rpiv-todo`（pi 插件市场，43K/mo；todo 列表 live overlay，/reload 与上下文压缩后存活）
- ⚠ 开放验证点见 open-questions Q001：插件 overlay 面向 TUI，devforge agent 跑 `--mode rpc`——RPC 下数据面（todo 工具与状态文件）是否可用需执行期验证；不可用则退「todo.md 落黑板 + 提示词纪律」方案

## MCP（配置统一入 profile 渲染，per-agent stdio，decisions/007）

| MCP | 拉起方式 | 用途 |
|-----|---------|------|
| CodeGraph | `codegraph serve --mcp`（stdio；本机 CLI 已装 1.6.2） | **代码事实层**——战略地位与分场景用法见 [codegraph-strategy.md](../topics/codegraph-strategy.md)：引擎预检 init、codegraph-first 纪律、MCP 单工具 + CLI 补充 |
| context7 | `npx -y @upstash/context7-mcp` | 最新库文档（resolve-library-id / get-library-docs）——偏差协议里「技术求证」的弹药 |
| web tools | pi 内置 | 通用网络搜索/抓取 |

生命周期：随各自 pi 进程生灭；系统对 pi 的台账管辖（decisions/007）传递覆盖。并发任务 = 多实例共享同一 `.codegraph/` 索引，FileLock 仲裁 + auto-sync 保新鲜。

## 语言 Skills（严禁自写，decisions/006）

- 范围：Rust / Go / Python / TS-JS 四类
- 来源纪律：外部搜索现成 skill → 下载 → **原样 vendor 入库**（不经过手重写——自写必带个人偏好与过时压缩；以搜索到的现状为准）
- 分发：随 coding/review profile 渲染落到 skills 目录

## 渲染器扩展点（config/）

- mcp.json 模板渲染（新面，planning 阶段未覆盖）
- 插件清单：planning 现有 ask-user-question 基础上 + rpiv-todo
- skills 分发：plan-tree skill（已有）+ 四语言 skills
