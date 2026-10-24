# 07 · 工具级路径权限（Tool-Path Permissions，办事工具的目录边界）

> 定位修正（用户拍板 2026-10-24）：read/write 碰哪些目录是**工具自身的权限配置**问题，
> 不是沙箱问题。沙箱（容器）管执行面（bash 不可枚举，只能隔离）；文件面可枚举，
> 走工具路径白名单。两层各司其职。

## 业界对标（2026-10-24 调研）

### Claude Code permissions（业界最成熟）

`settings.json` 的 `permissions.allow/ask/deny` 规则，工具级路径限定：

```jsonc
{
  "permissions": {
    "deny": [
      "Read(./.env)",            // 禁读密钥
      "Read(~/notes/private/**)",
      "Edit(//etc/**)"           // // = 文件系统绝对路径
    ],
    "allow": [ "Edit(/src/**)", "Read(docs/**)" ]  // / = 设置源相对
  }
}
```

- **gitignore 语法**：`**` 跨目录段、`*` 单段内、`!` 否定 carve-out、`~` home、`//` 绝对
- **求值顺序 deny → ask → allow**，deny 永远赢（specificity 不改变顺序）
- **symlink 双路径检查**：allow 要求请求路径与 realpath 都匹配；deny 任一匹配即拒
  （防符号链接穿越）
- **Read deny 同时封 Edit/Write 同路径**（写前提是读得到）
- **重定向也查**：bash 里 `> file` 的目标按 Edit 规则查
- **Claude Code 文档原话划界**：bash 文本规则「isn't a security boundary around the program」
  （`/usr/bin/curl`、`sh -c 'curl …'` 都绕）——「For filesystem and network enforcement that
  doesn't depend on command text, **use sandboxing**」。即：工具规则管正规调用，OS 沙箱管任意进程。
- bypassPermissions 警告原文：「Only use this mode in isolated environments like containers or VMs」

### Codex CLI（同构分层）

`sandbox_mode: read-only | workspace-write`——读全盘、写限 `cwd + writable_roots`，
OS 级实现（macOS Seatbelt / Linux landlock）。概念同为「写面目录白名单」，
外加 approval policy 管执行面。

### pi 的现实（2026-10-24 二轮调研：插件生态）

- 原生 settings **无路径权限**（defaultTools 只是工具选择，非路径围栏）
- **官方扩展点完备**：`tool_call` 事件的官方定位就是「Authorize or modify a tool call」
  （可 block 带 reason 给模型，可 mutate input）；handler 报错 = fail-safe 阻断工具（安全默认）；
  嵌套调用（ctx.executeTool）同样过 tool_call 拦截，无旁路
- **官方参考实现现成**：`examples/extensions/protected-paths.ts`（30 行）——拦 write/edit，
  查 `event.input.path`，命中保护路径返回 `{block: true, reason}`；permission-gate.ts 示范
  bash 拦截。骨架齐全，差的是配置外置/glob/realpath/read 面
- **社区包三个，全不贴合 devforge**：
  - `pi-guard-sandbox`（功能最全：bwrap OS 级 + workspace-write + 敏感路径禁读 + bash 沙箱，
    vendor fork 自 srt）——**致命：TUI-only，README 明说 RPC/print/JSON 模式不初始化沙箱、
    不拦截工具**；devforge agent 全是无头 RPC spawn，直接不可用
  - `pi-cwd-guard`、`@yandy0725/pi-sandbox`：同类 TUI 导向
- **结论：自研 path-guard 扩展**（骨架抄官方 protected-paths.ts + permission-gate.ts，
  配置从 devforge profile 渲染注入，估 ~100-150 行）——完全贴合 RPC 模式，
  fail-safe 语义白得，无第三方供应链风险

## devforge 设计：path-guard 扩展

**manifest 声明 → 渲染进 profile → pi 扩展执行**：

```jsonc
// AgentManifest 扩展字段
"pathPermissions": {
  "read":  ["docs/**", "assets/**"],         // gitignore 语法，相对 agent cwd（worktree）
  "write": ["docs/**"],                       // doc-agent：只写 docs/
  "denyRead":  [".env", "**/*.pem"],         // 可选黑名单，优先于白名单
  "denyWrite": [".pi/**", ".git/**"]
}
```

- 渲染器把配置写进 profile（如 `.pi/pathguard.json`），扩展随 profile 分发
- 守卫用 `registerTool` 替换 read/write/edit（edit = read+write 复合，两边都查）
- 拒绝时返回**可行动错误**：「此 agent 仅可写 docs/**」——模型可自纠，符合窄验证回路
- 审计：每次拦截发 `tool.denied` 事件入 run 事件流（DR-7 纯 API 可观测）

### 实现要点（从 Claude Code 借鉴的防御细节）

1. **realpath 后再比对**（symlink 穿越：请求路径在内、解析路径在外 → 拒）
2. **deny 优先**，白名单求值前先过黑名单
3. **profile 目录与守卫自身配置进 denyWrite**（拆提权链：改不回自己的工具配置）
4. 无 bash agent 的旁路面已被 devforge 渲染面封死：MCP 受控（mcp.json 渲染，DR-12 已示范）、
   无第三方扩展、skill 面受控——应用层守卫在此前提下边界完整
5. **持 bash 的 agent 不依赖 path-guard**（bash 绕一切应用层守卫）——执行面归容器

## 修正后的三层防线（最终版）

| 面 | 能力特征 | 机制 | 归属 |
| --- | --- | --- | --- |
| 执行面（bash） | 不可枚举（任意代码） | 容器（OS 级沙箱） | P012 M2 |
| 文件面（read/write/edit） | 可枚举（路径白名单） | path-guard 扩展（工具自身权限） | 本 topic |
| MCP 面 | 服务级管控 | mcp.json 渲染管控 | 已有（DR-12 先例） |

容器范围随之收窄：**只罩持 bash 的 agent**；无 bash agent（doc-agent/doc-review 等）
用 path-guard + 受控 MCP 即达边界完整，容器降为深度防御可选项。

## 验收

- 越权 read/write 被拒且事件可查（`tool.denied` + 路径 + 规则）
- symlink 穿越用例被拒；denyWrite 覆盖 profile 目录（提权链用例被拒）
- 每类 agent 的白名单按 SOP 职责声明（doc-agent 只写 docs/、doc-review 只写 review 报告目录）
