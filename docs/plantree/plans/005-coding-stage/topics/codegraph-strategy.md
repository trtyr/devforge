# Topic · CodeGraph 战略用法

## 定位：代码事实层

CodeGraph 不是供给清单里的一个工具，是流水线的**代码事实层**——plan tree 是语义意图，CodeGraph 是代码活的结构真相，两者之间的桥。且索引是**项目的持久资产**：住在代码工作区、auto-sync 随写随新、跨 run 跨 SOP 越用越新鲜（decisions/002 的「黑板」管过程资产，索引管结构资产）。

三层战略作用：

1. **省轮次**：一次 explore 替代 grep+read 爬取循环（官方实测工具调用 -88%、文件读→0）——轮次少=链路快=**中转长 turn 截断（P8-ISSUE-06）敞口缩小**
2. **审得动**：decisions/011「对照实物逐项核验」的可行性前提——无状态 Review 靠影响面/调用链从零上下文快速建立全局，否则大树逐项核验不可执行
3. **接得住**：写前 explore 集成点、改前 impact 看波及——AI 产出贴着现有架构长；错误责任分布制（decisions/008）的落地手段就是 impact 分析

## 角色 × 场景使用矩阵

| 场景 | 动作 | 价值 |
|------|------|------|
| Coding·开工勘察（decisions/010） | CLI `codegraph files` + explore 关键区域 | 半成品/已有项目快速成图，支撑续作/重做判断 |
| Coding·写前理解 | explore「X 怎么工作/X 到 Y 怎么走」 | 一次调用拿到集成点，零文件爬取 |
| Coding·改动前 | CLI `codegraph impact <symbol>` | 波及面先行；错误责任分布的定位依据 |
| Coding·阶段自检 | CLI `codegraph affected <改动文件>` | 交 review 前自查测试缺口（哪些测试需补/更新） |
| Coding·偏差取证（decisions/005） | explore 验证「plan 假设 vs 代码现实」 | 问用户前自查门槛的证据链 |
| Review·从零建全局 | explore 审查区域 | 无状态实例快速建立结构认知 |
| Review·改动审计 | impact / callers 逐改动核波及 | 「改签名漏调用方」类经典漏网的机器辅助 |
| Review·对接审计 | 调用链边即对接证据 | 前后端/模块连接审查从猜测变实证 |
| Review·文档核对 | 文档声明 vs 图上真实结构 | 文档审查落到结构层，不止语义层 |
| （未来）Test·影响定测 | `codegraph affected` | 变更→该测什么，测试 SOP 的现成地基 |

## 运行策略

- **索引预检归引擎**：spawn 前引擎预检代码工作区——无 `.codegraph/` 则执行 `codegraph init`（确定性基建，不烧 agent 上下文与轮次；大项目首建耗时与 run 启动重叠可接受）。agent 侧发现 codegraph inactive 时自行 init 兜底（提示词写明）
- **路径策略**（Q004）：agent cwd=黑板、索引在代码工作区——提示词教 MCP 调用显式传 `projectPath=<代码工作区>`；CLI 相应传路径参数。profile 模板不写死路径（profile 全局、项目路径项目级）
- **工具面**：MCP 维持默认单工具 `codegraph_explore`（实测智能体行为最优，输出已含影响面/关系图/符号体）；`impact`/`affected`/`callers`/`files` 走 CLI（agent 有 Bash）——不扩 MCP 工具面，省 context
- **提示词纪律条款（codegraph-first）**：结构问题先问图，禁止 grep-read 爬取循环；explore 返回的源码视为已读，不重复 Read；改前必看 impact
- **卫生**：mcp env 置 `CODEGRAPH_TELEMETRY=0`（跑在用户项目上，匿名遥测也关掉）
