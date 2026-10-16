# Topic · Agent 配置隔离（agent-profiles）

「每类 Agent 一套自己的 pi 配置，绝对不跟全局配置走」的落地机制。

## 机制（本机实研 2026-10-16）

核心开关：pi 原生支持配置目录覆写——环境变量 **`PI_CODING_AGENT_DIR`**（默认 `~/.pi/agent`）。spawn 时注入不同值 = 不同 Agent 拿到完全独立的配置世界，零魔改。

参考样本：本机 `~/.pi/agent/`（用户日常 pi 配置）。结构与机制：

- **插件安装双记录**：`pi install npm:X` 会写两处——`settings.json` 的 `packages` 数组（登记）+ `npm/package.json` 的 dependencies（记版本，实体在 `npm/` 下）。这俩都是配置目录内的，`PI_CODING_AGENT_DIR` 隔离后各 profile 自成体系
- **插件配置放目录根**：如 `sol-pi.json`（SoL-Pi 的开关与参数）；插件数据自建子目录（本机可见 `fff/`、`pi-blackhole/`、`intercom/` 等）
- **提示词分层**：`AGENTS.md`（全局规则）为角色提示词主载体；`extensions/` 是本地手写扩展（不走包管理），`skills/` 是技能库
- **其余配置面**：`mcp.json`（MCP 服务器）、`models.json`/`auth.json`（模型与凭据）、`trust.json`（项目信任）

→ devforge profile = 同构缩微：角色 AGENTS.md + settings.json（含必装包集）+ skills/ + 按需 mcp.json，`pi install` 在 profile 内执行即自动落双记录。

## 目录规划（草案，T005 细化）

```
~/.devforge/
├── plans/                      # 所有 plan tree，系统级统一（decisions/002）
│   └── <plan-name>/
├── profiles/                   # 所有 Agent 配置，系统级统一（decisions/006）
│   ├── plan-agent/             # PI_CODING_AGENT_DIR 指向这里
│   │   ├── AGENTS.md           # 「你是规划 Agent：读 plan-tree skill，跟用户沟通流程，产出 plan tree 保存到本地」
│   │   ├── skills/
│   │   │   └── plan-tree/      # 从 pi 技能库复制/同步 plan-tree 技能
│   │   └── settings.json       # 最小化：模型等基础项
│   └── plan-review/
│       ├── AGENTS.md           # 「你是规划检查 Agent：读 plan tree，检查语义通顺性、前后矛盾」
│       ├── skills/
│       │   └── plan-tree/      # 同上（Review 需要懂 plan tree 的结构约定才能查矛盾）
│       └── settings.json
└── logs/                       # 结构化日志（architecture-principles §2）
```

要点：
- **配置归属系统，不跟项目走**（decisions/006，用户拍板）：未来阶段新增更多 Agent（编码/测试/部署…），每个都在 `~/.devforge/profiles/<agent-name>/` 有专属文件夹，统一注册、统一管理——profile 目录就是 Agent 的「出厂身份」
- 两个 profile 的差异主要在 **AGENTS.md 提示词**，skills 目前相同（都要懂 plan-tree）；配置里包含 skills 的说法即此
- profiles / plans / logs 同在 `~/.devforge/` 下但职责不同：profiles 是出厂配置，plans 是工作产物，logs 是运行轨迹
- Review 的提示词刻意窄：只查语义通顺、前后矛盾，其余不管——边界写死在 AGENTS.md 里

## 必装插件（用户拍板 2026-10-16）

以下包名为原文照录，所有 Agent 的 profile 都必装——这是基础功能，不是可选项：

```
npm:@ff-labs/pi-fff
npm:@juicesharp/rpiv-web-tools
npm:pi-blackhole
git:github.com/NVlabs/SoL-Pi
```

按 Agent 差异化追加：

```
npm:@juicesharp/rpiv-ask-user-question   # 仅 Plan Agent（要跟用户沟通，需要提问能力）
```

安装机制（用户拍板 2026-10-16，与 pi 文档一致）：**`pi install` 一把梭**——装的时候自动登记（settings.json packages 数组 + npm/package.json 双记录），插件随 pi 启动自动加载、数据自建，不需要手工接线。唯一可能要手工的是插件自身的配置文件（如 `sol-pi.json` 那种开关参数），按需后置。profile 初始化 = 跑一遍安装清单，完事。

## 待办锚点

- T005：settings.json 最小集确认（模型、provider 怎么给子 Agent 供 key——继承环境还是要显式注入）；插件安装已定案（pi install 一把梭，见必装插件节）
- skills 的分发方式：复制 vs 符号链接 vs 包管理（pi packages.md 有现成机制，待比对）
