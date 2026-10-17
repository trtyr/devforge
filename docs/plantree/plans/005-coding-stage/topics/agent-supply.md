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

## 供给生态地图（同类工具侦察结论，2026-10-18）

代码上下文工具三大家族（社区对比文献 + 936 实测的分工结论）：

| 家族 | 回答什么 | 我们的选型 |
|------|---------|-----------|
| **结构图**（repo map/graph） | 调用谁/影响谁/流程怎么走——结构真相 | ✅ CodeGraph（事实层，见 codegraph-strategy.md） |
| **LSP/诊断** | 类型错误/lint/形式检查——编译器级即时反馈 | ✅ pi-lens（2026-10-18 拍板入选，Q005 关闭）：49 语言服务器 + ast-grep/opengrep/typos 辅助扫描，编辑后即时诊断免跑全量构建；pi 原生插件与我们的宿主同源。RPC 可用性待验（T003，同类 Q001） |
| **语义检索**（embedding RAG） | 按意思找代码（补偿图的词法盲区） | 🍉 Deferred：opencode-codebase-index / pi-smritikosh；1M 上下文+好提示词下非刚需 |

旁类已覆盖：最新文档（context7+web tools）✓｜过程管理（rpiv-todo）✓｜领域知识（四语言 skills）✓。

pi 插件市场其他扫过的相关件（评估后不入选/备用）：pi-lsp（通用 LSP runner，pi-lens 更全）、pi-simplify（变更码复审，与我们 review agent 重叠）、pi-fff（模糊搜索，pi 内建已够）、billion-context（第三方压缩，替换 pi 原生 compaction 风险大不採）、pi-web-access（web tools 不可用时的备胎，Q003）、cc-safety-net（破坏性命令拦截，agent 自治场景摩擦大不採）。

## 渲染器扩展点（config/）

- mcp.json 模板渲染（新面，planning 阶段未覆盖）
- 插件清单：planning 现有 ask-user-question 基础上 + rpiv-todo
- skills 分发：plan-tree skill（已有）+ 四语言 skills
