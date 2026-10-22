# 与 MVP 的差异总表（改这一条前先看这里）

> 目的：以后有人问「change 为什么跟 MVP 不一样」，只看这一页。
> 五条差异，其余一律沿用 MVP——**没有第五条之外的差别**。

## 差异一：plan 的输入与工具

| | MVP plan | change plan |
| --- | --- | --- |
| 输入 | 一句需求 + 空白仓 | 一句需求 + **既有代码仓**（+ 现有文档） |
| 工具 | 无 MCP | **codegraph**（explore / 反向引用 / 依赖） |
| 产出 | 目标、范围、任务拆分 | 六块：需求 / 影响面 / 三张表 / 白名单 / 验收标准 / 风险 |
| 明确不做 | —— | 不跑测试、不写实施顺序、不看 diff |

## 差异二：coding 多了基线、白名单与越界二分

- **基线门**：动手前自己判断怎么跑测试、跑一遍存证，红就不许动（MVP 没有“之前”）
- **白名单**：计划给的文件集合；`git diff --name-only <baseline>` ⊆ 白名单才放行
- **越界二分**：越界 + 未申报 = 范围膨胀 FAIL；越界 + `<route>plan</route>` + `<impact-gap>` = 打回 plan 补边界
- **文档同步**：改动涉及的 `docs/contexts/` 由 coding agent 同步写，`doc-lint` 必须 0 违规（MVP 也是 coding 写文档，这里只是强调“随改动同步”）

## 差异三：testing 多了回归与验收对账

- **回归先行**：先跑全套既有测试，红 → 直接 `<route>coding</route>`，回注带**失败明细逐条原文（含 `path:line`）**
- **e2e 只补变更点**，不重建整套
- **验收对账**：plan 每条验收标准必须指到测试 `path:line`；缺一条 = FAIL
- **回退方向**：doc 问题也走 `<route>coding</route>`（MVP 是 reroute=doc-side → doc-fix 子 SOP）

## 差异四：无标记重推有界（与 MVP 同步改）

- 三段都沿用 `<stage-complete>N</stage-complete>`
- 连续 3 次无标记 → 第 4 次判**质量失败**（不自动重启）——此前本节写的是「基础设施故障」，与代码不符，已改正
- 质量循环（审查打回、修复轮次）仍无上限——**只给进度循环设界**
- 审查**输出协议违例**的重审有界（≤3）：换**全新审查实例**重审（不回注被审对象），超限判质量失败；审查**实例失败**的重试也有界（≤3），超限判基础设施故障（可重启）

## 差异五：检查点落盘跨会话

- `docs/plantree/changes/<id>/state.json` 由**引擎写**：阶段、基线 sha、往返次数、已完成步骤/验收项、审查轮次
- MVP 各 SOP 的进度账本在内存里，进程重启即丢；change 要求断点续跑

## 附：命名对照

| 段 | MVP | change |
| --- | --- | --- |
| plan | `plan-agent` / `plan-review` | `change-plan-agent` / `change-plan-review` |
| coding | `coding-agent` / `coding-review` | `change-coding-agent` / `change-coding-review` |
| testing | `test-agent` / `test-review` | `change-test-agent` / `change-test-review` |

命名规则 `change-<阶段>-<角色>`：看到名字就知道这一段在干什么、审的是什么。
