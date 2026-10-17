# Topic · 共享黑板（Shared Blackboard）

单位 = 当前任务。物理位置 = `~/.devforge/plans/<项目>/<任务>/`（即 planning SOP 的产物目录与引擎 spawn cwd，decisions/002）。「共享」之义：当前任务的后续所有 Agent（coding/review/未来的测试…）都可见、按读写矩阵存取。

## 布局约定

```
plans/<project>/<task>/
├── （plan tree 原样）      # planning 阶段产物；coding 阶段只读（偏差协议除外，decisions/005）
└── coding/                 # coding 阶段过程资产，CodingSopLoop 创建
    ├── stages.md           # 阶段划分与状态（coding agent 写；review 审计其真实性）
    ├── todo.md             # 当前阶段 todo list（随阶段滚动更新）
    ├── docs/               # 功能文档（伴写产出；未来测试 Agent 的输入）
    ├── reports/            # review-N.md（Review Agent 每轮产出）
    └── blockers/           # plan 偏差留痕（发现依据/问题位置/用户裁决，decisions/005）
```

## 读写矩阵

| 角色 | 读 | 写 |
|------|----|----|
| Coding Agent | plan tree、coding/ 全部 | coding/（除 reports/）、偏差时按协议编辑树 |
| Coding Review Agent | 全部（树 + coding/ + 项目代码） | coding/reports/ |
| 测试 Agent（未来） | docs/、树、代码 | ——（未定，测试立项时定） |

## 代码工作区（不入黑板）

目标项目代码位置 = 项目登记的 `local_path`（创建项目时用户指定：从 0 创建 = 在该位置建项目；已有项目 = 原地登记）。代码跟着项目走，是项目层属性；黑板只放过程资产——run 归档、重跑留档策略不卷代码。Coding Agent 同时拿到两个路径：黑板（cwd）与代码工作区（提示词注入），CodeGraph 索引建在代码工作区。
