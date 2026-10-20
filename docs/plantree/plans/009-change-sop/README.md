# 变更 SOP（`change`）· 框架

> 2026-10-20 立项。目标：在**既有项目**上加功能 / 修 bug。
> 与 MVP（从零做需求）共用 plan / coding / testing 三段骨架，真正重写的是 **plan 阶段**：
> 把「凭需求想象功能」换成「用 codegraph 把需求落到具体文件与符号上」，产出一份可机械校验的**改动边界**。

## 一句话边界

- **做什么**：给一个已有代码仓 + 一句需求 → 影响面 → 改动计划 → 改动 → 回归证明 → 文档同步。
- **不做什么**：从零建项目（那是 MVP 链）；补全文档（那是 `doc-from-zero`，本 SOP 只在 `doc_level=essential` 时调用它）。

## 三档场景（门 0 判定，不是猜）

| 档 | 特征 | 门 0 动作 |
| --- | --- | --- |
| A | 有 `.git` + 有 `docs/contexts/` | 记 HEAD 当基线；plan 直接读文档 |
| B | 有 `.git`、无文档 | 记 HEAD；文档**默认当没有**（D002），要用就调用方指定 `doc_level=essential` 先跑轻量 |
| C | 无 git、无文档 | `git init` + **基线提交**（D001）；之后按 B 走 |

> 为什么不在引擎里自动探测“有没有文档然后决定跑不跑 doc-from-zero”：文档完整度是人的决定，不是程序的猜测。
> 引擎只认调用方给的 `doc_level`（`none` / `essential` / `complete`），默认 `none`。

## 阶段差异（与 MVP 对照）

| 段 | MVP | change |
| --- | --- | --- |
| plan | 功能设计 + 任务拆分 | **影响面 + 改动边界 + 验收命令**（六块，见下） |
| plan 审查 | 需求覆盖 / 计划可执行 | 追加：影响面必须有 codegraph 证据、边界外零改动、验收命令审查官亲自跑 |
| coding | 从零实现 | 同一阶段提示词 + **越界二分**（范围膨胀 FAIL / 计划缺口打回 plan）+ 先跑既有测试 |
| testing | 给新系统写测试 | **先回归后新增**，回归失败直接阻塞 |
| 收尾 | 无 | 文档同步（`docs-update` / `docs-sync`）+ `doc-lint` 0 违规 + 变更报告 |

## 编排：带回路的状态机（与 MVP 最大差别）

MVP 是单向流水（plan→coding→testing）；change 是 **plan ⇄ coding ⇄ testing**：

- coding 发现计划漏了必需文件 → 输出 `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界
- 边界增量过 change-review（只审增量，防边界蠕变）→ 带新边界重进 coding
- 越界但**没申报** → FAIL（范围膨胀），一票否决
- 往返上限 3 次，每次写 `changes/<change-id>/history.md`；第 4 次停下问人

细节见 `topics/boundary-protocol.md`。这也是本 SOP 不能照搬 `run_coding` 的原因。

## plan 阶段六块（`change-plan.md`）

1. **需求复述 + 模糊点**——先问不猜（沿用 plan 纪律）
2. **影响面**——codegraph `explore` 出的相关文件、调用链、被引用点，每条 `path:line`
3. **三张表**——要改的 / 受影响但不改的 / 必须回归验证的
4. **改动边界**——文件级白名单 + 明确不碰项（coding 审查的一票否决依据）
5. **验收命令**——既有测试怎么跑、变更测试写在哪、通过标准
6. **风险**——无测试覆盖的盲区、兼容性/并发风险

## 为什么“白名单”要引擎侧机械校验

写进提示词的边界只是请求；写成 `git diff --name-only` 对白名单的机械比对才是约束。
这是 P008 学到的同一条道理（“prompt 是请求，退出码是事实”），这里直接复用。

## 落盘

- 计划/影响面/边界 → 目标仓 `docs/plantree/<change-id>/`（D003，可随基线提交、可审查）
- 运行证据（diff 统计、回归输出、审查结论）→ 黑板 `plans/<project>/<task>/change/`
- 基线 commit → `docs/plantree/<change-id>/baseline.txt`

## 相关

- `roadmap.md`：任务拆解与里程碑
- `open-questions.md`：待定问题
- `decisions/README.md`：已拍板决策
- `topics/impact-analysis.md`：影响面与 codegraph 用法口径
