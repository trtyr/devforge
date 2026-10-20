# T008 / M1 / M3 · change SOP 真链端到端实证（验收阶段，2026-10-20）

- 脚本：`tests/e2e/test_26_change_e2e.py`（可复跑；`python3 tests/e2e/test_26_change_e2e.py`）
- **终版见证：v5，2026-10-20 20:xx，25m46s，31 PASS / 0 FAIL / 0 issues**（run Settled）
- fixture：`textkit` 小 Python 包（stdlib unittest + git 基线）；需求：新增 `count_words`（实现 + 单测 + README 同步）
- 供给：宿主 manifest 9 agents + change 六件套（夹具 `tests/e2e/fixtures/change_agents.json`，防漂移守卫 `change_agents_fixture_matches_builtin` 锁同步）；模型统一映射 `minimax-m3.1-flash`（deepseek/grok 渠道当日不可用——机制见证用可用渠道）

## 一、终版链路（v5）

| 阶段 | 实测 |
| --- | --- |
| 门 0 | change-id `change-20261020-textkit-…`（幂等）、codegraph 预检、baseline 记 HEAD（`baseline.txt` 落计划目录） |
| plan | 六块计划落**目标仓** `docs/plantree/changes/<id>/change-plan.md`（需求复述/影响面/白名单/验收标准/风险逐块断言 PASS）→ 计划审查 PASS |
| coding | 基线取证轮（unittest 全绿）→ 4 步按步提交：`89e4319 …(change-… step 1)` 清单实现 → `1941ecf step 2` 包导出 → `a0b66f6 step 3` 单测 → `2328bfb step 4` README 同步 |
| testing | 回归 + 新测试 + 验收对账（引擎机械解析 `testing/acceptance.md`，逐条 `- 条款 → path:line`） |
| 收尾 | 引擎写 `report.md`（含验收对账 + 墙钟）；黑板 `change/state.json` 检查点在；**run Settled（25min）** |
| 兑现 | `count_words` 真实存在、`python3 -m unittest discover` 全绿、README 接口清单同步 |

## 二、五轮实测记录（每次都修出真问题——这正是 e2e 的价值）

| 轮 | 结果 | 发现 → 修复 |
| --- | --- | --- |
| v1 | Failed @3min | **审查官正确打回**计划（15 处 path:line 简写），但引擎 plan 段**没有打回-修复环**，一枪 Quality → 产品修复 `0cdc83b`：`fix_plan` 回注重审，上限 `MAX_PLAN_FIXES=3`（对齐 MVP 规划循环），单测两条 |
| v2 | Failed @31min | plan 修好、coding 走完，testing 对账拒收「命令行 → exit 0」混入对照表——**引擎按设计拒收是对的**（非 path:line 引用=假绿风险）→ 提示词收紧 `6b9a9e5`：对照表只准有验收项 |
| v3 | Failed @12min | plan 把验收标准写成 **markdown 表格**，`plan_facts` 连表头抽出 23 条鬼条款 → 双修 `c868e70`：抽取只认列表行（单测 `plan_facts_ignores_table_rows_and_keeps_bullets`）+ plan 提示词格式硬约束（禁表格） |
| v4 | Settled @24min | 产品全过；唯一 FAIL 是脚本按 `change:` 字面前缀 grep，实际提交是 `feat(change-<id>): step N` 风格（引擎门认可）→ 断言对齐 `8bdf203` |
| **v5** | **Settled @25min46s，31 PASS / 0 FAIL** | 干净见证（上表） |

## 三、验收对照（T008 原文 + M1/M3）

| 项 | 实测 |
| --- | --- |
| **T008** plan 阶段端到端，六块齐全且审查可判 | ✅ v5：六块断言全 PASS；v1 还实证了审查**会**判 FAIL 且（修后）打回可恢复 |
| **M1** plan 阶段端到端跑通 | ✅ |
| **M3** 一次真实变更端到端（plan→coding→testing→报告），证据齐全 | ✅ 计划/基线/按步提交/对账/报告/断点全部落盘可回查 |

## 四、诚实边界

1. **边界申报（`<route>plan</route>` + `<impact-gap>`）未在真链触发**——fixture 需求小，coding 没越界；该路径由 9 条边界单测覆盖（含 3 轮往返上限、history.md 留痕）；
2. **审查打回后修复**在 v1→v2 之间经产品修复后由单测覆盖（`plan_fix_loop_recovers_after_one_rejection`）；v5 真链里 plan 一次过审，未观测到真实打回-修复；
3. 模型统一 MiniMax（渠道现实）；六 profile 的模型差异不改变链路机制；
4. change-id 尾部截断（`…-0-textk`）：slugify 40 字符上限把长需求截了尾——幂等性不受影响，观感问题，登记不修。

## 五、配套修复清单（本轮 e2e 逼出的产品改动）

| commit | 内容 |
| --- | --- |
| `0cdc83b` | plan 阶段打回-修复环（`fix_plan` + `MAX_PLAN_FIXES`，2 单测） |
| `6b9a9e5` | change-test-agent 提示词：对照表只准有验收项 |
| `c868e70` | `plan_facts` 只认列表行 + plan 提示词验收标准禁表格（1 单测） |
| `8bdf203` | e2e 按步提交断言对齐实际门禁格式 |
