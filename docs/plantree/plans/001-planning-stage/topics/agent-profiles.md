# Topic · Agent 配置隔离（agent-profiles）

「每类 Agent 一套自己的 pi 配置，绝对不跟全局配置走」的落地机制。

## 机制验证（已确认）

pi 原生支持配置目录覆写：环境变量 **`PI_CODING_AGENT_DIR`**（默认 `~/.pi/agent`）。spawn 时注入不同值 = 不同 Agent 拿到完全独立的配置世界，零魔改。

## 目录规划（草案，T005 细化）

```
~/.devforge/
├── plans/                      # 所有 plan tree，系统级统一（decisions/002）
│   └── <plan-name>/
└── profiles/
    ├── plan-agent/             # PI_CODING_AGENT_DIR 指向这里
    │   ├── AGENTS.md           # 「你是规划 Agent：读 plan-tree skill，跟用户沟通流程，产出 plan tree 保存到本地」
    │   ├── skills/
    │   │   └── plan-tree/      # 从 pi 技能库复制/同步 plan-tree 技能
    │   └── settings.json       # 最小化：模型等基础项
    └── plan-review/
        ├── AGENTS.md           # 「你是规划检查 Agent：读 plan tree，检查语义通顺性、前后矛盾」
        ├── skills/
        │   └── plan-tree/      # 同上（Review 需要懂 plan tree 的结构约定才能查矛盾）
        └── settings.json
```

要点：
- 两个 profile 的差异主要在 **AGENTS.md 提示词**，skills 目前相同（都要懂 plan-tree）；配置里包含 skills 的说法即此
- profiles 目录与 plans 目录都在 `~/.devforge/` 下，但职责不同：profiles 是 Agent 的「出厂配置」，plans 是工作产物
- Review 的提示词刻意窄：只查语义通顺、前后矛盾，其余不管——边界写死在 AGENTS.md 里

## 待办锚点

- T005：settings.json 最小集确认（模型、provider 怎么给子 Agent 供 key——继承环境还是要显式注入）
- skills 的分发方式：复制 vs 符号链接 vs 包管理（pi packages.md 有现成机制，待比对）
