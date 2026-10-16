# Topic · Agent 配置隔离（agent-profiles）

「每类 Agent 一套自己的 pi 配置，绝对不跟全局配置走」的落地机制。

## 机制验证（已确认）

pi 原生支持配置目录覆写：环境变量 **`PI_CODING_AGENT_DIR`**（默认 `~/.pi/agent`）。spawn 时注入不同值 = 不同 Agent 拿到完全独立的配置世界，零魔改。

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

安装机制待办（归 T005）：`pi install` 把包持久化在配置目录的 settings 里——`PI_CODING_AGENT_DIR` 隔离下每个 profile 各自装各自的包集；base 四件套写进公共安装脚本/文档，差异包写进各 profile。

## 待办锚点

- T005：settings.json 最小集确认（模型、provider 怎么给子 Agent 供 key——继承环境还是要显式注入）；必装插件的 profile 内安装机制（公共四件套 + 差异包）
- skills 的分发方式：复制 vs 符号链接 vs 包管理（pi packages.md 有现成机制，待比对）
