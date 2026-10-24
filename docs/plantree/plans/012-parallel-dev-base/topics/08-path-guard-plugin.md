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
