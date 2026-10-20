# P008 · doc-from-zero（从零写文档）

> 状态：**框架定稿**（2026-10-19，Q001–Q013 全关、D001–D020 落定；任务清单见 `roadmap.md`，待执行）
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

**共用资产**：`topics/doc-contract.md`（条目模板 + doc-lint 规格）不只用于本 SOP——coding 写文档、coding-review 审文档、doc-fix 补文档都用同一份，避免四份措辞漂移。

## Agent 编制（两类，沿用开发链骨架）

> **一个干活的 + 一个审查的**，循环推进，PASS 才落地。下面五个阶段是**干活 agent 的内部步骤**，不是五个 agent。

| profile | 干什么 | 供给 |
|---|---|---|
| **doc-agent**（干活） | 清单 → 勘察 → 解释 → 写作 → 汇编 | `codegraph` + `context7` MCP（与 coding-agent 同款） |
| **doc-review**（审查） | 五项审计 + 跑 doc-lint → verdict + route | `codegraph` MCP（查实体存在性） |

**codegraph 是我们给 agent 的 MCP 配置**（`codegraph_mcp()`），不是外部工具；沿用 decisions/013：引擎侧预检 `init`，agent 不自行 init，调用显式 `projectPath=<目标项目>`。

## 怎么调起来（2026-10-19 实测口径）

```bash
# 1) 登记目标仓（无文档、无 git 也行）
curl -sX POST localhost:7888/projects \
  -H 'content-type: application/json' \
  -d '{"name":"book-app","local_path":"/abs/path/book-app"}'

# 2) 建任务并触发（复用串行 guard / 看门狗 / 黑板 / 事件流）
curl -sX POST localhost:7888/projects/book-app/tasks \
  -H 'content-type: application/json' -d '{"id":"d1","sop":"doc-from-zero"}'
curl -sX POST localhost:7888/projects/book-app/tasks/d1/start   # → 202

# 3) 事后可复算（不信 run 的自述）
devforge doc-audit <repo>/docs/contexts --repo <repo>
```

产物树（D016）：`docs/contexts/` 正文 + `_meta/{inventory.json,glossary.md,gate0-closures.md,state.json,gaps.md,facts/,interpretation/,reviews/,runs/<run-id>/report.md}`。
`report.md` 带墙钟 / 模块数 / 单元数 / 审查轮次 / **token 用量（从 pi 会话账汇总，非 Agent 自述）**。

## 节奏：四道门（D010）

```
门 0  清单 + 模块树 + 档位（底座，最严）
  ↓ PASS 才开工
单元滚动（一批 = 审查官一轮读得完的量，一单元一提交）
  ↓ PASS 才 commit
汇编单元（总览/数据流/Mermaid，只合成不新增事实）
  ↓
终审（全局覆盖对账 + 跨模块一致性 + 全量 doc-lint）
```

三条硬约束：批大小按**审查阅读预算**定（不按模块数拍死）；检查点只认 **outcome 证据**（git diff / doc-lint 退码 / 实体回查），不认自述；**终审不可省**（跨模块问题只在最后暴露）。

依据：agentpatterns.ai 增量验证、SmartBear/Cisco 2500 次审查实证、GitHub stacked PR 指南、RepoDoc（arXiv 2604.26523）。详见 `topics/design-framework.md` §三·B 与 §五·E。

## 五阶段（doc-agent 内部步骤，草案，未定稿）

> 事实与推断分离，每条断言带置信度。详见 `topics/design-framework.md` §四。

| 阶段 | 输入 | 产出 | 关键点 |
|------|------|------|--------|
| 0 · 清单 | 代码库 | `inventory`（功能点条目 + 证据指针）+ `doc_level` 三档（默认 complete） | 机械抽取（**codegraph**，20+ 语言）；**是"写全"的分母** |
| 1 · 勘察 | inventory + 模块树 | 每模块事实记录（有什么，**不解释**） | **一模块一会话**；同层可并行 |
| 2 · 解释 | 勘察结果 | 业务规则/状态机/权限矩阵（**🟢/🟡/🔴，旁挂证据表**） | 无 git 时禁止声称“已知设计意图”，一律 🟡 |
| 3 · 生成/汇编 | 事实 + 推断 | **`<项目>/docs/contexts/`** 下的条目文档 + 总览 + **Mermaid 图源码** | 只合成不新增事实；上层可追溯叶子 |
| 4 · 审计 | 全部产物 | 通过/打回 + `gaps.md` | **doc-review** 在两个时机跑：单元审（这笔活写没写对写全）+ **终审**（全局覆盖 + 跨模块一致性）；工具 = devforge 侧 doc-lint |

## 研究依据（2026-10-19 检索）

- **CodeWiki**（[arXiv 2510.24428](https://arxiv.org/html/2510.24428v6)）：依赖图 → Tarjan 缩点 → 拓扑找入口（入度 0）→ 模块树（**叶子 ≤32K tokens，下派 ≤3 层**）→ 叶子 agent（四件输入）→ **父文档由子文档合成**；跨模块引用登记表防重复；评估=分层 rubric + 三厂商判官投票（68.79% vs DeepWiki 64.06%；脚本语言强、C/C++ 弱）。**注意：它的 rubric 是由官方文档反推的，无文档场景不可照搬**；且全文未报耗时/成本。
- **DocAgent**（[ACL 2025 demo](https://aclanthology.org/2025.acl-demo.44.pdf)）：**逐组件条目**级（Repo-Level Doc = ✗）；拓扑序写文档（消融：乱序写真实性 94.64%→86.75%）；Reader/Searcher/Writer/Verifier/Orchestrator 分工（Reader↔Searcher 可多轮）；三维评估 = 完整性（**AST 机械** 0.934）+ 有用性（判官 3.88）+ **真实性（实体存在率 95.74%，Chat 基线仅 61.10%、FIM 45.04%）**。

**两者定位**：**CodeWiki 管结构**（模块树/分层汇编），**DocAgent 管条目**（单点质量与反幻觉）——P008 两条都吃。
- 补充：[从边缘进入陌生代码库](https://understandlegacycode.com/blog/dive-into-an-unfamiliar-codebase-from-its-edges/)（入口点优先）；[scope-discoverer 类 agent](https://github.com/shinpr/claude-code-workflows/blob/main/agents/scope-discoverer.md)（多来源功能范围发现）。
- **Reversa**（[analysis pipeline](https://sandeco.github.io/reversa/pipeline/)）：遗留系统 → 可执行规格的五阶段 agent 流水（ Scout→Archaeologist→Detective/Architect→Writer→Reviewer）；**关键贡献：事实/推断分离 + 🟢🟡🔴 置信度标记 + 一模块一会话 + doc_level 三档 + 🔴缺口转问题交人**。无 git 时它靠历史回溯的那条路不成立（我们全部计 🟡）。
- **渐进式节奏的证据链**：增量验证模式（[agentpatterns.ai](https://agentpatterns.ai/verification/incremental-verification/)，点名 write-then-review 为反模式、检查点须 outcome-based）；[SmartBear/Cisco 2500 次审查](https://mikeconley.ca/blog/2009/09/14/smart-bear-cisco-and-the-largest-study-on-code-review-ever/)（100-300 行最佳、>400 行失控、>60 分钟断崖、作者预注释则缺陷密度极低）；[GitHub stacked PR 指南](https://docs.github.com/en/copilot/tutorials/stack-ai-generated-code-in-pull-requests)（底座先审、错层不修）。
- **RepoDoc**（[arXiv 2604.26523](https://arxiv.org/html/2604.26523v1)）：知识图谱为骨架，**4096-token 功能内聚聚类 + 图查询只取相关片段**，24 仓 8 语言实测：比 CodeWiki 快 3×、token -85%、**增量更新召回 97%（时间 -73%）**、自动出 Mermaid。
- **特征定位学术骨架**（[Feature Location Survey](https://www.cs.toronto.edu/~chechik/pubs/locationsurvey12.pdf)）：静态（依赖/调用图）/ 动态（执行轨迹）/ IR（标识符与注释相似度）；我们用“静态优先 → 可跑补动态 → IR 当命名探针”。背景：专业开发者最多 **70% 时间花在程序理解**（Xia et al., TSE 2018）。

详细精读结论（可借用/不可照搬两张表）见 `topics/design-framework.md` §五·B。

## 文件地图

- `topics/design-framework.md` — 框架全文（双 Agent 编制、四道门节奏、五阶段、完备性自证、路由、检查点）
- `topics/doc-contract.md` — **跨 SOP 共用的文档契约草案**（条目模板/反模式/真实性规则/doc-lint 规格）
- `decisions/README.md` — D001–D020（权威、红线、落点、codegraph、画图、置信度、档位、doc-lint、双 Agent、四道门、词表、审查供给、完成判据、必填节、增量、目录、串行、供给、UI、协议）
- `open-questions.md` — **Q001–Q013 全关**
- `roadmap.md` — **已定稿任务清单** T000a–T025 + M0–M5（待执行）

## 关键前提

**完成判据（D013）**：两层 100%——**清单内覆盖 100%**（限定在 `doc_level` 要求的条目集合内）+ **清单外缺口全部登记**（codegraph 不支持的语言/生成代码/无行为可写，静默缺口 = FAIL），再加真实性 100% 与 Runbook 可跑。口径叫"声明性完整"：不是"全写完了"，而是"有什么我们说清了，包括没写的"。

**必填节（D014）**：按条目类型固定分级（入口/公开 API 缺节即 FAIL，内部符号只记 finding）；**不适用必须写 N/A**，省略不等于不适用。

**文档体系唯一权威 = 本 plan（D001，2026-10-19 用户拍板）**；旧 `doc-init` skill 退役，其六节格式与标记契约**不再作为约束**，但其工程教训以 D002 吸收为红线（证据下限/防伪标记/编号稳定/幂等重跑/只读/不回显任务书）。

## Scope

**In（未来）：** 第二条 SOP 的完整实现——inventory 抽取器、引擎侧 doc-lint、单元滚动编排、五项审计、Runbook 约定、产物布局、增量模式。

**Out：**
- 改写现有 `doc-init` skill——**已拍板退役（D001）**，不并行维护两套
- 文档的长期维护/过期检测（见 Q013）
- 非代码资产（设计稿/数据库 dump）的文档化
