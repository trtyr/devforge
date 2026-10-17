# Topic · CodeGraph 战略用法

## 定位：代码事实层（cached intelligence）

CodeGraph 不是供给清单里的一个工具，是流水线的**代码事实层**——plan tree 是语义意图，CodeGraph 是代码活的结构真相，两者之间的桥。官方定位一句话：**缓存智能**——数千次解析/追踪决策预先算好，agent 不必每次重新推导。索引是**项目的持久资产**：住在代码工作区、auto-sync 随写随新（写后 ~1s）、跨 run 跨 SOP 越用越新鲜——黑板管过程资产，索引管结构资产。

三层战略作用：

1. **省轮次**：一次 explore 替代 grep+read 爬取循环（官方实测工具调用 -88%、文件读→0）——轮次少=链路快=**中转长 turn 截断（P8-ISSUE-06）敞口缩小**
2. **审得动**：decisions/011「对照实物逐项核验」的可行性前提——无状态 Review 靠影响面/调用链从零上下文快速建立全局
3. **接得住**：**边写边看图**（官方教义：reach for it BEFORE *and* while writing）——产出贴着现有架构长；错误责任分布制（decisions/008）的落地手段

## 三大基本功（用户拍板的场景，官方教义逐一对应）

1. **了解功能**：explore 传自然语言问题（「导出功能是怎么工作的」）——一次调用返回该功能全流程的逐行源码+调用路径。Coding Agent 接到阶段 todo 先 explore 功能区；Review 审查前先弄清「这功能是什么」
2. **修改函数**：explore 直接点名符号——返回可安全 Edit 的行号源码 + **谁调它 + 它影响谁**（blast radius）。「动这个函数要改什么」的完整清单一次给全；重载名一次返回所有重载体。改前必看，改时对照
3. **快速上手新项目**：CLI `codegraph files` 看结构 + 几次 explore 关键区域 = 分钟级架构地图。Coding Agent 首触已有项目（decisions/010 勘察义务）的标配动作

## 角色 × 场景使用矩阵

| 场景 | 动作 | 价值 |
|------|------|------|
| Coding·开工勘察（decisions/010） | `files` + explore 关键区域 | 半成品/已有项目快速成图，支撑续作/重做判断 |
| Coding·功能理解 | explore 自然语言问功能 | 一次拿到功能全流程源码+路径，零爬取 |
| Coding·写前/写中 | explore 点名符号（源码+调用者+波及一次给全） | 带着影响面编辑，改动清单完整 |
| Coding·阶段自检 | CLI `codegraph affected <改动文件>` | 交 review 前自查测试缺口 |
| Coding·偏差取证（decisions/005） | explore 验证「plan 假设 vs 代码现实」 | 问用户前自查门槛的证据链 |
| Review·从零建全局 | explore 审查区域 | 无状态实例快速建立结构认知 |
| Review·改动审计 | impact / callers 逐改动核波及 | 「改签名漏调用方」类漏网的机器辅助 |
| Review·对接审计 | 调用链边即对接证据 | 前后端/模块连接审查从猜测变实证 |
| Review·文档核对 | 文档声明 vs 图上真实结构 | 文档审查落到结构层 |
| Review·新鲜度协议 | 识别 ⚠️ staleness banner | 被 flag 的文件改用 Read 确认，其余照信（下详） |
| （未来）Test·影响定测 | `codegraph affected` | 变更→该测什么，测试 SOP 的现成地基 |

## 纪律条款（写进双 agent 提示词，源自官方 anti-patterns）

- **codegraph-first**：结构问题先问图，一次 explore 起手；grep-read 爬取循环 = 重复图已做过的工作
- **结果视为已读**：explore 返回的行号源码安全可 Edit，不重复 Read；要更多细节→带更具体的名字再 explore 一次
- **不复核**：图来自全量 AST 解析——用 grep 复核它更慢更不准；只有图没覆盖的细节（配置/文档）才用原生工具
- **不手工拼流程**：把流程两端符号名都给 explore，让它吐路径（含 grep 跟不了的动态派发跳转）
- **精确点名**：流程端点用精确符号名（部分/错名不会静默替换）；explore 按词法匹配不按语义，空结果按 did-you-mean 重试
- **新鲜度协议**：响应带 ⚠️ banner 时——点名文件 pending→那几个文件改用 Read；auto-sync disabled/recovering→catch-up 完成前直接 Read；projectPath 查询的漂移文件→Read 该文件，其余照信
- **正确性边界**：图只管结构，不管对错——编译器/测试/lint 仍是真值来源，review 跑测试不可省

## 运行策略

- **索引 init 归引擎独占**：spawn 前引擎预检代码工作区——无 `.codegraph/` 则执行 `codegraph init`。**agent 不自行 init**（官方教义：indexing is the user's decision，agent 只提不动手；引擎=系统=用户的执行者，预检属确定性基建）。codegraph 不可用/未索引时 agent 优雅降级用原生工具，不阻塞。新索引被运行中的 server 实时拾取，无需重启
- **路径策略**（Q004 已解决）：官方有无根索引变体——server 起在无索引目录时工具照常可用，靠 `projectPath` 逐项目查询，解析最近 `.codegraph/`。提示词教 agent 显式传 `projectPath=<代码工作区>`
- **工具面**：MCP 维持默认单工具 `codegraph_explore`（官方实测最优）；`impact`/`affected`/`callers`/`files` 走 CLI——不扩 MCP 面
- **dedup 不开**：`CODEGRAPH_EXPLORE_DEDUP=1` 仅适用于「一连接一持久上下文」的宿主——coding agent 长会话会触发 pi compaction，「已在对话中」指针会断，保持默认重发源码
- **卫生**：mcp env 置 `CODEGRAPH_TELEMETRY=0`（跑在用户项目上，匿名遥测也关掉）
