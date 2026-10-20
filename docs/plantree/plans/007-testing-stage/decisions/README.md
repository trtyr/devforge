# P007 Decisions

> 2026-10-18 设计讨论三轮定稿（Q001-Q009 全关账 + 三项执行决策）。详细论证见 topics/testing-stage-design.md。

## D001 黑盒纪律与归因权

测试 Agent 不看实现代码：提示词**硬禁令 + 白名单**（文档全量/运行面配置/目录路径/自写测试/系统输出可读；实现文件内容禁读；路径可看内容禁读），test-review 审计兜底（断言文档未承诺行为 → 疑似偷看打回）。**归因权归 coding agent**（唯一看得见代码与文档两侧的角色）：测试侧只交中性发现记录（文档出处/文档说/实测/复现步骤/严重度），不猜不判。

## D002 红灯三分类与 flaky 去抖

测试 FAIL 三分类：A 测试自身错（修自己重跑）/ B 系统行为与文档不符（=发现，本阶段核心产出）/ C 环境抖动。去抖：pytest-rerunfailures 自动重跑 2 次（3 次机会），重跑后绿=PASS+flaky 嫌疑标记，三连红=坐实 B 类；重跑在单次运行内部，SOP 层只看最终结果。

## D003 review FAIL 双根因路由（工具侧实现）

test-review 输出增 `<reroute>test-side|doc-side</reroute>` 协议标签（沿用 coding 已实证的结构化标签模式）：test-side → 状态机自动退回测试 Agent（附 issues）；doc-side → 自动触发 doc-fix 子 SOP。保守解析：标签缺失/非法 → 按 test-side 退回，绝不静默吞。黑板 markdown 报告仅留档，路由只认标签。

## D004 doc-fix 子 SOP（写文档模式）

独立 SOP 注册（`doc-fix`），跨 SOP 触发 `start {"sop":"doc-fix"}`；操作对象 = 黑板 coding/docs/ 伴写文档（D004 产物）与 plan tree 相关节；**内置 doc-review 审查轮**（见 D008）审「新文档 ↔ plan tree 一致性 + 内部自洽 + Runbook 契约」，通过后流回测试环节——「未审文档入测试」场景结构性消除。

## D005 完成定义

review PASS + 自制计划（stages/todo）全部完成 + 全绿（发现全部处置：修复/改档复测/用户明示接受）。不允许带红收官。

## D006 执行方式（与 coding 同模）

kickoff（黑板+workspace+文档路径+plan tree 路径）→ 测试 Agent 通读文档 → 自判体量 → 自划分 N 阶段（按文档领域）→ stages.md + todo.md（rpiv-todo）→ 每阶段：写测 → 自起被测系统自跑自收 → 落报告 → `<stage-complete>N</stage-complete>` → fresh test-review → FAIL 退回 / PASS 推进 / 末阶段 Finish。环境默认：从仓库全新 checkout 照 Runbook 跑起被测系统；用例自造数据自清理互不依赖；性能/安全/混沌出界。

## D007 供给即纪律

**给**：base 四件套照常 + test-agent extras [rpiv-todo, rpiv-ask-user-question] + MCP context7（外部库文档）+ skill 仅 python-best-practices。**扣**：codegraph、pi-lens（代码事实层工具=黑盒纪律下的诱惑通道）、plan-tree skill（读不写）、非 Python 语言 skills。test-review 极简（无插件，核心工具自带）。doc-fix 沿用 coding-agent 家族供给（它需要对着代码 triage）；doc-review 极简（读 plan tree + 文档）。

## D008 审查角色阵容

test-review：无状态一次性，对文档三判据（业务关系/用户场景/极端用户面）+ 审计追溯矩阵 + reroute 标注。doc-review（**新 profile**）：doc-fix 内置审查轮，审新文档 ↔ plan tree 一致性。coding review 增门禁：**照文档从零 boot**（干净环境按 Runbook 装依赖→配置→启动→健康检查→种子数据，跑不通=文档 FAIL）——「牛逼文档」的结构性保证，Runbook 七件套契约（环境/安装/配置/启停+健康检查/种子数据/测试运行方式/公开接口清单）。

## D009 findings 台账

黑板 `testing/findings/` 一条发现一个文件（blockers 同款形制），字段：id / 文档出处 / 文档说 / 实测 / 复现步骤 / 严重度 / 状态（open→triaged→fixed→verified）/ 裁决结果。完成定义要求无 open 态遗留。

## D010 测试代码落地

Python 编写；仓库 `tests/integration/` + `tests/e2e/` 分层（pytest marker），coding 写 `tests/unit/` 同树不打架；`tests/AGENTS.md` 约定文档惯例；commit-on-pass（D009 同款）——黑盒回归套件成为仓库常驻资产。

## D011 时长哲学

不设墙钟超时，只看活性（006 D002 直接继承）：会话在长即正常，平线超阈才收割（DEVFORGE_RUN_IDLE_MS 可选，默认永不杀）。测试阶段按全流水线最慢阶段预算 watchdog 监视目录。

## D012 启动方式

沿用现状：coding Completed 后 API `start {"sop":"testing"}` 手动/编排触发（与 planning→coding 跨 SOP 触发同款）；本轮不做自动链。回归门禁（未来 coding 完成定义加「不破坏既有 e2e」）为预留接口，本轮不实现。

## D013 无标记重推有界（与 005/015 同步，2026-10-20）

**决策**：连续 3 次未发 `<stage-complete>N` → 第 4 次判基础设施故障，停止 run；测试阶段的打回轮数仍无上限。
**为什么**：见 005 decisions/015——进度循环要设界，质量循环不设。
**后果**：`testing.rs` 同步计数与常量；反向测试覆盖。
