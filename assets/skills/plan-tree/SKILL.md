---
name: plan-tree
description: Use when project work involves planning, roadmap discussion, requirement clarification, scope negotiation, or maintaining a Markdown planning tree for a software project.
---

# plan-tree

用一棵纯 Markdown 的「规划树」承载项目的规划资产：目标、路线图、主题深潜、决策、开放问题、证据。规划树是 AI 与人共同维护的单一天源——写下来的才算数。

## 目录约定（plan root 即当前任务目录）

```
<plan root>/
├── README.md          # 入口：项目一句话定位、范围、权威顺序、文件地图
├── roadmap.md         # 路线图：任务清单（[ ]/[x]）+ 当前焦点 + 阶段记录
├── topics/            # 主题深潜：一个复杂主题一个文件（设计/架构/风险）
├── decisions/         # 决策记录：一个决策一个文件（选了什么+为什么+后果）
├── ideas/             # 收纳箱：未定型的想法，定期评审晋升或归档
└── open-questions.md  # 开放问题：待澄清事项，解决后移入 decisions 或删除
```

## 权威顺序

信息冲突时：decisions > roadmap > topics > README。改低权威文件前先确认不与高权威矛盾。

## 工作模式

- **clarify**：需求模糊时先问清（用 ask-user-question），把答案记入 README 的目标与范围。
- **shape-plan**：把共识沉淀为 roadmap 任务清单（每项有验收口径）+ 需要深潜的主题。
- **ready-check**：执行前自检——每个任务有验收标准？决策都落盘？开放问题阻塞执行吗？
- **execute-ready**：通过自检后宣告就绪；执行中新决策随手记入 decisions/。

## 硬规则

1. 每个文件开头一行说明「这是什么、谁在什么时候维护它」。
2. 决策必须带「为什么」——没有理由的决策等于没做。
3. 不写废话：规划树是给下一个读者（人或 Agent）的工作指令，不是日记。
4. 全程小步快跑：一次写一个文件的一小段，写完简短总结再继续。
