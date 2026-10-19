# P008 · doc-from-zero（从零写文档）

> 状态：**框架草案**（2026-10-19 立项讨论，仅记录方向，未进设计定稿）
> 一句话：给定一个**完全陌生、零文档、无 git 历史**的代码库，产出一套「写全、可验证、不编造」的文档体系。

## 为什么需要（用户提出）

现有 `doc-init` skill 体感不错（内容确实全），但**慢**、且「全不全」没有可数判据。
用户要求把它升级为自动化开发流水线的**第二条 SOP**（与「从 0 出 MVP」平级），而不只是一个 skill。

## 与现有流水线的关系

```
SOP-1  from-zero-to-mvp   规划 → 编码 → 测试        （P001/P005/P007）
SOP-2  doc-from-zero      陌生库 → 文档体系         （本 plan）
```

两条 SOP 共用：work agent + review agent 骨架、黑板、watchdog、codegraph、渲染配置（八 profile 里已有四类角色可复用）。
**SOP-2 不需要新的编排机制**，需要的是**新的判定方式**（见下）。

## 核心命题（本 plan 的立论）

> 「写全」不能靠 agent 自觉，必须靠**机械清单可数**。

- 分母 = 从代码事实里机械抽取的**功能点清单**（入口点/公开 API/CLI/路由/配置键/数据模型/测试用例）
- 覆盖 = 清单每一条 ↔ 文档某节的逐条对账（缺 = 零覆盖）
- 真实性 = 文档提到的每个代码实体回查代码，查不到 = 幻觉（防编造）
- 可跑 = Runbook 从零 boot 一遍

三条判定都是**确定性/可复核**的，不依赖模型「感觉写全了」。

## 四阶段框架（草案，未定稿）

| 阶段 | 输入 | 产出 | 关键点 |
|------|------|------|--------|
| 0 · 清单 | 代码库 | `inventory`（功能点条目 + 证据指针） | 机械抽取，不花模型时间；**是"写全"的分母** |
| 1 · 分层写作 | inventory + 模块树 | 每模块文档（叶子优先） | **多 agent 并行**；跨模块只建引用不重写 |
| 2 · 汇编 | 叶子文档 | 架构总览 / 数据流 / 使用指南 / 架构图 | 自底向上合成 |
| 3 · 审计 | 全部产物 | 通过/打回 | 覆盖 + 真实性 + 可跑，三查机械判定为主 |

## 研究依据（2026-10-19 检索）

- **CodeWiki**（[arXiv 2510.24428](https://arxiv.org/html/2510.24428v6)）：依赖图 → 拓扑找入口（入度 0）→ 模块树 → 叶子 agent → 自底向上汇编；复杂度超限动态下派；跨模块引用登记表防重复；评估=分层 rubric + 判官，覆盖率 60-82%。
- **DocAgent**（[ACL 2025 demo](https://aclanthology.org/2025.acl-demo.44.pdf)）：拓扑序写文档（消融：乱序写事实准确性 94.6%→86.8%）；Reader/Searcher/Writer/Verifier 分工；三维评估 = 完整性（AST 机械检查）+ 有用性（LLM 判官）+ **真实性（实体存在率，防幻觉）**。
- 补充：[从边缘进入陌生代码库](https://understandlegacycode.com/blog/dive-into-an-unfamiliar-codebase-from-its-edges/)（入口点优先）；[scope-discoverer 类 agent](https://github.com/shinpr/claude-code-workflows/blob/main/agents/scope-discoverer.md)（多来源功能范围发现）。

## 文件地图

- `topics/design-framework.md` — 框架全文（四阶段、判定口径、与 doc-init skill 的差异）
- `open-questions.md` — 待拍问题（Q001+）
- `roadmap.md` — 任务分解（草案，未定稿）

## Scope

**In（未来）：** 第二条 SOP 的完整设计——inventory 抽取器、并行写作编排、三查审计、Runbook 约定、产物布局。

**Out：**
- 改写现有 `doc-init` skill（它是本 SOP 的参考实现，不是被替换对象）
- 文档的长期维护/过期检测（未来另议）
- 非代码资产（设计稿/数据库 dump）的文档化
