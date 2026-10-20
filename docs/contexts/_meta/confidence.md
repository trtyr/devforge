# unit-src-01 · 置信度证据表

配套：`facts/unit-src-01.md`（只记有什么）、`gaps.md`（🔴 台账）。本文件只记「凭什么这么写」。
档位：complete。分级：🟢 能给出 `path:line` 或可重放命令 / 🟡 给出依据 / 🔴 进 `gaps.md`。

## src/config/installer.rs

| # | 断言 | 置信度 | 依据 / 证据 |
|---|---|---|---|
| I1 | 装包命令是 `pi install <pkg>`，profile 目录经 `PI_CODING_AGENT_DIR` 注入 | 🟢 | `src/config/installer.rs:12`—`src/config/installer.rs:17` |
| I2 | `PI_CODING_AGENT_DIR` 在 pi 侧确实是「覆盖配置目录」的环境变量（默认家目录下 .pi/agent） | 🟢 | pi 官方文档 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/environment-variables.md:81` |
| I3 | `pi install <source>` 是 pi 的安装命令形态 | 🟢 | pi 官方文档 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/cli.md:9` |
| I4 | 装包失败时错误里带 stderr 尾部（最多 500 字节、不切坏 UTF-8） | 🟢 | `src/config/installer.rs:23`—`src/config/installer.rs:42`；单测 `src/config/installer.rs:174`、`src/config/installer.rs:183`、`src/config/installer.rs:193` |
| I5 | 截断按字节而非字符 | 🟢 | 长度判断与切片都作用在字符串字节长度上：`src/config/installer.rs:27` |
| I6 | 假桥写两份记录（`settings.json` 的 `packages` 数组 + `npm/package.json` 的 `dependencies`） | 🟢 | `src/config/installer.rs:76`、`src/config/installer.rs:86`、`src/config/installer.rs:97`、`src/config/installer.rs:107`；单测 `src/config/installer.rs:135` |
| I7 | 假桥的 `packages` 数组不去重、`dependencies` 按包名覆盖 | 🟢 | `src/config/installer.rs:89`（push 不查重）、`src/config/installer.rs:112`（insert 覆盖） |
| I8 | 假桥遇到非法 JSON 静默退化为空对象；遇到合法但非对象的 JSON 会 panic | 🟢 | `src/config/installer.rs:78`、`src/config/installer.rs:100`（`unwrap_or` 退化）；`src/config/installer.rs:85`、`src/config/installer.rs:106`（`expect` 触发 panic） |
| I9 | `PackageInstaller` 的 `Send + Sync` 是为并发渲染共享同一装包器 | 🟢 | 注释 `src/config/installer.rs:46`；渲染层 worker 上限 4：`src/config/render.rs:27`、`src/config/render.rs:227` |
| I10 | 「pi 不在 PATH 就起不了进程」是推断（未在本机触发） | 🟡 | 依据：`Command::new("pi")` 的进程查找规则（`src/config/installer.rs:13`）；本轮未实测该失败路径 |
| I11 | 保留 stderr 尾部是刻意决策（上游 postinstall 会吞掉真实 errno） | 🟡 | 依据：模块/函数注释原文 `src/config/installer.rs:20`；收益未在本机复现 |
| I12 | 真桥与假桥写出的记录形状完全一致 | 🔴 | 见 `gaps.md` #2（本轮未实测真实 `pi install`） |
| I13 | 本单元 5 条 behavior 单测当前为真 | 🔴 | 见 `gaps.md` #1（本机无 Rust 工具链，单测未执行） |

## src/config/manifest.rs

| # | 断言 | 置信度 | 依据 / 证据 |
|---|---|---|---|
| M1 | 清单分两层：模板层（写死）+ 模型层（可变） | 🟢 | 模块头注释 `src/config/manifest.rs:3`—`src/config/manifest.rs:6` |
| M2 | 内置清单 = 4 必装包 + 9 Agent + 3 模型池条目 + 1 provider + `api_key = None` | 🟢 | `src/config/manifest.rs:502`、`src/config/manifest.rs:504`、`src/config/manifest.rs:511`、`src/config/manifest.rs:534`、`src/config/manifest.rs:646`；单测 `src/config/manifest.rs:700` |
| M3 | 模型固定分配：审查类 → `grok-4.7`；编码/规划/测试/文档修补 → `deepseek-flash`；doc-agent → `minimax-m3.1-flash` | 🟢 | 各 Agent 的 `model` 字段：`src/config/manifest.rs:539`、`548`、`559`、`574`、`590`、`599`、`609`、`624`、`639`；单测 `src/config/manifest.rs:700` |
| M4 | 「模型分配由用户拍板固定死、是终态配置」 | 🟡 | 依据：行内注释 `src/config/manifest.rs:548` 与决策记录 `docs/plantree/plans/002-config-management/decisions/README.md` 决策 004 |
| M5 | 「维护收敛律：系统定型后唯一可自定义维度是模型」 | 🟡 | 依据：决策记录 `docs/plantree/plans/002-config-management/decisions/README.md` 决策 003；代码侧对应两层 schema 注释 `src/config/manifest.rs:3` |
| M6 | 「Key 明文不设防」是既定决策 | 🟡 | 依据：字段注释 `src/config/manifest.rs:459` 与决策记录 001（同上路径） |
| M7 | 提示词常量是「定稿即定死、改决策先改 plan tree」 | 🟡 | 依据：各常量上方注释（如 `src/config/manifest.rs:73`、`src/config/manifest.rs:311`） |
| M8 | 差异化包的分工意图（问用户出口 / 待办纪律 / 诊断证据 / 黑盒不给诊断） | 🟡 | 依据：常量注释 `src/config/manifest.rs:21`、`src/config/manifest.rs:24`、`src/config/manifest.rs:31`、`src/config/manifest.rs:34`——注释是意图声明，不是可执行约束 |
| M9 | `resolve_model` / `resolve_provider` 是清单替换前的 fail-fast 校验闸门 | 🟢 | 服务端在 PUT 清单时先对每个 Agent 解析模型与 provider：`src/server/handlers.rs:353`、`src/server/handlers.rs:354` |
| M10 | 清单缺四个非 `Option` 必填字段即反序列化失败；`api_key` 缺失时取 `None` | 🟡 | 依据 serde 派生语义 + 四个非 Option 字段均无 `#[serde(default)]`（`src/config/manifest.rs:452`、`454`、`456`、`458`）；`api_key` 为 `Option` 且无 serde 属性（`src/config/manifest.rs:460`）；本仓无对应单测（见 `gaps.md` #4） |
| M11 | `save` 覆盖写、不回读校验；`load` 不做字段级默认值填充 | 🟢 | `src/config/manifest.rs:681`—`src/config/manifest.rs:692` |
| M12 | `api_key` 默认 `None`，由渲染层按 provider 分键写进 auth.json | 🟢 | `src/config/manifest.rs:646`；消费方 `src/config/render.rs:138` |
| M13 | `doc-contract` skill 由 coding-agent / coding-review / doc-fix / doc-review 四个 profile 共用 | 🟢 | 单测 `src/config/manifest.rs:761`；各 Agent 的 `skills` 字段 `src/config/manifest.rs:560`、`src/config/manifest.rs:575`、`src/config/manifest.rs:610`、`src/config/manifest.rs:640` |
| M14 | 本单元 8 条 behavior 单测当前为真 | 🔴 | 见 `gaps.md` #1（本机无 Rust 工具链，单测未执行） |
| M15 | 7 段提示词各自的「设计依据」出处 | 🔴 | 见 `gaps.md` #3（正文只有决策编号，无逐条出处） |

## 外部工具契约（pi / codegraph / npx）

| # | 断言 | 置信度 | 依据 / 证据 |
|---|---|---|---|
| E1 | pi 的个人安装写进配置目录的 `settings.json` | 🟢 | pi 官方文档 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/packages.md:19` |
| E2 | pi 用 `packages` 数组声明已安装包 | 🟢 | pi 官方文档 `/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/packages.md:100` |
| E3 | `codegraph` 与 `npx @upstash/context7-mcp` 两个 MCP server 可正常连通 | 🔴 | 见 `gaps.md` #5（本单元只声明配置，未拉起） |
