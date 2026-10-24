# 08 · path-guard 插件（pi 扩展）研究与服务端设计

> P012 topic 07 的实现篇。M2 沙箱作废后，工具级路径权限的本体 = 这个 pi 扩展。
> 定位：**devforge 系统的内置插件**——源码住 devforge 仓（assets 分发模式，与 skills 同构），
> 渲染 profile 时分发进每个 agent 的 `extensions/`，配置由 devforge 渲染（`.pi/pathguard.json`）。
> 2026-10-24 研究定稿；实现分两步：devforge 分发管线（本 plan）+ 插件本体（assets 内 TS）。

## 一、pi 扩展机制研究结论（2026-10-24，读 pi 官方文档与源码示例）

### 1. 形态与分发

- 扩展 = TypeScript 模块，`export default function (pi: ExtensionAPI)`；**jiti 免编译**。
- 加载位置三处：agent 目录（`<agent-dir>/extensions/`，可用 `PI_CODING_AGENT_DIR` 环境变量
  重定向）、项目 `<cwd>/.pi/extensions/`、CLI `--extension <file>`。
- **关键发现：profile 即 agent-dir**。devforge spawn pi 时已注入 `PI_CODING_AGENT_DIR=<profile>`
  （profile 内的 settings.json / models.json / mcp.json 正是 agent-dir 结构）→
  **`<profile>/extensions/path-guard/` 会被 pi 自动加载，无需额外注册**。分发零协议。
- 扩展在 **interactive / RPC / JSON / print 四种模式全部加载**（extensions.md §UI and modes）
  ——devforge 的 RPC headless agent 照常生效。pi-guard-sandbox 等社区方案败在 TUI-only，
  官方扩展机制没有这个限制。

### 2. 拦截面：`tool_call` 事件是官方授权拦截点

- 文档原话：`tool_call` = "Authorize or modify a tool call"；handler 可原位改 `event.input`、
  可返回 `{ block: true, reason }` 阻断（reason 展示给模型）。
- **fail-closed 是文档口径**："gate handlers should fail closed when their safety decision
  cannot be made"；且 **handler 抛错 = 阻断该工具**（fail-safe）。
- **嵌套调用无旁路**：`ctx.executeTool()` 发起的嵌套调用与模型发起的调用走同一
  `tool_call`/`tool_result` 拦截，codemode 脚本同样不豁免。
- 官方最小参考：`examples/extensions/protected-paths.ts`（31 行，write/edit + 路径子串匹配 +
  `{block, reason}`）与 `permission-gate.ts`（mode-aware + fail-closed 范式）。

### 3. 与 devforge 的结合面（深度结合的三个点）

1. **配置分发**：devforge 渲染 `AgentManifest.pathPermissions` → `<profile>/.pi/pathguard.json`
   （已落地，64f3bcf）。插件按发现顺序读取，**两端 glob 语义必须同构**
   （devforge `path_covered_by`：精确 / 目录前缀（尾 `/`） / `**` 递归）。
2. **事件回流**：被阻断的调用经 RPC 事件流 `tool_execution_end`（`toolName`/`isError`/`result`）
   流回 devforge。约定 **reason 前缀 `[path-guard] `**——devforge 客户端（engine/client.rs）
   识别 `isError && result 含前缀` → 转 `boundary.violation` 事件入 run 事件流。
   零新通道，复用现有 RPC 解析。
3. **双保险分层**：插件 = 事前拦截（模型根本拿不到越界结果）；引擎终校验（M0）=
  事后审计（`git diff base..HEAD ⊆ owned_paths`）。插件管 agent 级静态边界（manifest 声明），
  引擎管任务级契约边界（forbidden_paths 等）。

### 4. 边界（明确不拦）

- **bash 不拦**：bash 不可枚举（任意 shell 写文件绕过路径检查），持 bash 的 agent 的
  边界由 worktree（M1 物理隔离）+ 契约终校验（M0）承担；path-guard 只服务
  **无 bash 的 agent**（read/write/edit 型），其 profile 不渲染 bash 工具。
- **MCP 工具不在面内**：devforge 渲染 mcp.json 完全受控（manifest 声明什么才有什么），
  无第三方 MCP 注入路径。

## 二、插件设计

### 形态与分发

```
devforge 仓
  assets/extensions/path-guard/
    index.ts        # 工厂：读配置 → 注册 tool_call handler
    match.ts        # glob 匹配（与 devforge path_covered_by 同构，含单测）
    config.ts       # 发现 + 解析 + fail-closed 判定

渲染管线（config/render.rs render_agent）
  1. skills 分发既有逻辑旁，复制 assets/extensions/path-guard/ → <profile>/extensions/path-guard/
  2. .pi/pathguard.json 已有（64f3bcf）
  → pi 启动（PI_CODING_AGENT_DIR=<profile>）自动加载
```

渲染开关：`AgentManifest.path_permissions` 为 None 的 profile **不分发插件**（与 pathguard.json
同口径——不启用的 agent 面板上零痕迹）。

### 配置发现（config.ts）

按序取第一个命中：

1. 环境变量 `DEVFORGE_PATHGUARD`（绝对路径——devforge spawn 显式注入，最强约定）；
2. `<cwd>/.pi/pathguard.json`；
3. `<agent-dir>/.pi/pathguard.json`。

判定口径：

- 三处都无 → **未启用**，handler 直接放行（老 profile 零影响，与渲染端 None 不生成对齐）；
- 文件存在但 JSON 损坏 / schema 不合法 → **fail-closed**：所有 write/edit 一律 block
  （read 放行——损坏不能成为拒绝服务的读放大），`[path-guard] 配置损坏` 通知一次。

### 拦截判定（index.ts）

```
tool_call(toolName, input):
  toolName ∉ {read, write, edit} → 放行
  path = input.path（normalize 后 realpath——防 symlink 指到界外）
  配置 = load()（损坏 → block 所有 write/edit）

  # deny-first：两个 deny 列表永远赢
  if deny_read 覆盖(path) or deny_write 覆盖(path):
      op = (deny_read 命中) ? "read+write/edit（deny_read 连带堵写）" : "write/edit"
      → block（仅当 toolName 会写，或 deny_read 命中且 toolName 是 read）

  # write 白名单语义：write 非空 = 只允许列表内
  if toolName ∈ {write, edit} and write 非空 and ¬write 覆盖(path):
      → block（reason 带 [path-guard] 前缀 + 命中的具体规则）

  # read 白名单语义：read 非空 = 只允许列表内
  if toolName == read and read 非空 and ¬read 覆盖(path):
      → block

  放行
```

- **deny_read 连带堵写**：读不进的路径写也没意义（edit 前置 read 会被堵，直接 write
  也被 deny_write 覆盖——两列内容通常成对，但语义上 deny_read 单独命中就堵写，防漏配）。
- **提权双保险**：渲染端已把 `<profile>/**` 追加进 deny_write（64f3bcf）；插件启动时
  自检配置：若 `DEVFORGE_PATHGUARD` 所在目录不在 deny_write 覆盖内 → 运行时动态追加
  （防渲染端未来回退）。
- **realpath**：`fs.realpathSync` 解析后再匹配（symlink 指到界外 = 界外）；解析失败
  （目标不存在——write 新文件）→ 用 parent 目录 realpath + 文件名拼接。

### 遥测（与 devforge 事件流闭环）

- block 时：`ctx.hasUI` 则 `ctx.ui.notify`（TUI/RPC 可见的交互面）；reason 恒带
  `[path-guard] ` 前缀。
- devforge 侧（engine/client.rs，实现归 devforge 任务）：解析 RPC 流 `tool_execution_end`，
  `isError && toolName ∈ {read,write,edit} && result 文本含 "[path-guard] "` →
  记 `boundary.violation` 事件（复用 M0 事件通道，验收可在 run 事件流查询拦截记录）。

### pathguard.json schema（与 devforge `PathPermissions` struct 逐字段对齐）

```jsonc
{
  "read":       ["**"],                    // 允许读 glob；空 = 不限（仅受 deny_read）
  "write":      ["src/**", "docs/**"],     // 允许写 glob；空 = 不限（仅受 deny_write）
  "deny_read":  [".env", "**/secrets/**"], // 永远赢；命中连带堵写
  "deny_write": ["assets/prompts/**", "<profile>/**"]  // 永远赢；profile 目录渲染端恒追加
}
```

## 三、实现计划（两仓分工）

| 步骤 | 仓 | 内容 |
| --- | --- | --- |
| 1 | devforge | assets/extensions/path-guard/ 本体三件（index/match/config，TS，含 match 单测——与 devforge `path_covered_by` 同一样例集双端对齐） |
| 2 | devforge | render_agent 分发：复制插件目录进 profile/extensions/（path_permissions None 则不分发）+ 渲染单测 |
| 3 | devforge | engine/client.rs 识别 `[path-guard] ` 前缀 → boundary.violation 事件 + 集成测试（mock RPC 流） |
| 4 | 验收 | 7889 起带契约任务：越界 write 被插件拦（run 事件流见 boundary.violation）；界内 write 正常 |

## 四、边界与非目标

- bash 不拦（见「边界」）；持 bash agent 的隔离 = worktree + 契约终校验。
- 不做 per-task 动态 pathguard（任务级 forbidden_paths 由 M0 终校验兜底；pathguard 是
  agent 级静态边界——多任务并发共享 profile，动态覆盖会串）。
- 不做 TUI 配置面板（配置唯一真源 = devforge manifest 渲染）。
- 网络面不在本期（原 srt 二期网络白名单随 M2 一并作废；模型 API 出网是刚需）。

## 五、影响面分析（审查 Q&A，2026-10-24）

| 影响对象 | 判定 |
| --- | --- |
| pi 内置工具（bash 等） | 零接触：handler 只对 read/write/edit 判定，其余返回 undefined 放行 |
| MCP 工具 | 工具名不在面内，天然不碰（devforge 渲染的 mcp.json 受控） |
| 未启用 profile | path_permissions None → 不分发插件不生成配置，零痕迹（已有测试钉住） |
| codemode/嵌套调用 | 走同一 tool_call 拦截无旁路——脚本读越界文件同样被堵（正向特性） |
| devforge 客户端 | 被拦调用是 agent 层工具失败（模型自调整），不进 SOP 基础设施故障重试分类，无重启风暴 |

两个真实风险点与缓解：

1. **handler 顺序与短路**：多扩展同挂 tool_call 时按加载序串行，block 短路后续 handler。
   安全组件应在**链尾最后裁决**（让改 input 的扩展先跑，path-guard 看最终形态防绕过）。
   当前 profile 内它是唯一扩展；分发实现固定排在 extensions 目录排序末位。
2. **自身加载失败 = fail-open**：TS 加载失败 pi 报 extension error 继续跑（主进程无恙），
   但边界失守。缓解：渲染分发时对插件源码过语法校验 + 验收启动探针（故意越界 write
   必须见到 `[path-guard]` 拦截，已列实现计划第 4 步）。


## 六、产品化：非源码部署下的可用性（2026-10-24 审查）

产品化 = 用户拿到的是编译好的单二进制，没有 devforge 源码仓。逐项盘点资产链路：

| 资产 | 非源码部署状态 | 机制 |
| --- | --- | --- |
| 提示词 13 件 | ✅ 已就绪 | R4：include_str! 编译期内联，二进制单文件 |
| skills 内容 | ✅ 已就绪 | skill_assets.rs 43 处 include_str! 内嵌（P2-ISSUE-05），生产渲染 skills_source=None 走内置资产 |
| manifest/模型池/契约 | ✅ 已就绪 | ConfigManifest::builtin() 内嵌 |
| **path-guard 插件本体** | 设计即内嵌 | 实现 Discipline：assets 里的 TS 用 include_str! 内嵌，渲染时从内存字符串写出 `.ts` 文本——pi 侧 jiti 免编译，拿到文本即可执行 |

结论：**可用**。插件分发模式从「复制仓库目录」改为「编译期内嵌 + 渲染时写出」——
这是 devforge 已验证两次的成熟模式（R4 prompts / skills P2-ISSUE-05），非新发明。
实现纪律：`assets/extensions/path-guard/*.ts` 全部经 include_str! 进二进制；
渲染单测断言「skills_source=None 时 extensions/ 照常分发」（防回退到目录复制）。

真正的非源码部署遗留（另案，不阻塞本期）：

- **pi 本体的 npm 安装**：profile 渲染时装包走网络/npm 源——产品化部署需预装镜像或
  离线 npm 源（部署工程，与插件机制无关）；
- codegraph 外部二进制（preflight 可选依赖，缺索引自动跳过）。
