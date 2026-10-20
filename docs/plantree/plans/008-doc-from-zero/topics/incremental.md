# 增量模式（D015 / D021）· 数据流与失效规则

> 目标：同一套 doc-from-zero 管线，只重算「这次提交真正影响到的单元」。
> 不做第二条链，不引入新依赖；所有判定必须机械可复算。

## 一、数据流

```mermaid
flowchart TD
    A[start doc-from-zero] --> B{状态里有 base_sha?}
    B -- 无 --> F[全量: 现有行为]
    B -- 有 --> C[git diff --name-status base..HEAD<br/>+ git status --porcelain]
    C --> D{变更文件为空?}
    D -- 是 --> E[无变更: 直接终审/收口]
    D -- 否 --> G[变更文件 → 新清单 unit_id]
    G --> H[旧清单 vs 新清单: 变更文件上消失的 canonical_name]
    H --> I[全仓源码词边界扫描这些名字]
    I --> J[命中文件的 unit_id 入集合]
    G --> K[受影响单元集合]
    J --> K
    K --> L[汇编页引用检查<br/>README/architecture/dataflow/runbook]
    L --> M[affected = 单元 ∪ (lint 红时全部 done)]
    M --> N[从 done 移除 affected]
    N --> O[只派发 affected 单元]
    O --> P[单元终审全局跑]
    P -->|PASS| Q[base_sha 推进到 HEAD]
    P -->|FAIL| R[base_sha 不动]
```

## 二、算法步骤（可执行）

1. **读基线**：`RunState.data["base_sha"]`；环境变量 `DEVFORGE_DOC_BASE` 可覆盖；`DEVFORGE_DOC_INCREMENTAL=1` 才启用增量（默认关闭，避免悄悄改变现有行为）。
2. **取变更**：`git diff --name-status <base> HEAD`，映射 A/M/D/R；再并入 `git status --porcelain` 的未提交路径。去重后按路径排序。
3. **文件 → 单元**：读 `docs/contexts/_meta/inventory.json`（不存在则重建），用 `entry.path == 变更路径` 取 `unit_id`。新增文件不在旧清单 → 先重建清单再取。
4. **名字差集**：对变更文件，比较旧清单（上次运行写入的）与新清单的 `canonical_name`，取**消失的名字**（删除/改名）。改名同时产生新名字 → 新名字所在文件也进集合。
5. **引用传播**：对名字集合做全仓词边界扫描（跳过 `docs/`、`.git`、`target`、`.codegraph`、`node_modules`、`*.lock`），命中文件 → unit_id 入集合。
   - 保守近似：宁可多算（多写一篇），不可漏（漏写 = 文档骗人）。
6. **汇编上溯**：检查 `README.md`、`01-architecture.md`、`02-dataflow.md`、`10-runbook.md` 是否提到受影响单元（文档路径或实体名）→ 记入 `assembly_review` 清单。
7. **失效**：`affected = 单元集合`；`doc-lint` 当前已经不是 0 违规 → `affected = 全部 done`（无法机械归因，宁重做）。
8. **派发**：`to_run = 受影响单元`；未受影响单元不进 to_run，也不删除其产物。
9. **校验**：重写后 `doc-audit` 机械三查 + `assembly_review` 里每页的 `path:line` 仍可解析。
10. **记账**：终审 PASS → `base_sha = HEAD`；报告里写「增量：base→HEAD，受影响 N 单元，跳过 M」。

## 三、状态字段

| 字段 | 位置 | 语义 |
| --- | --- | --- |
| `base_sha` | `RunState.data`（黑板 state.json） | 上次成功运行的代码基线；缺省 = 从未成功增量过 |
| `done` | 同上 | 已有单元完成名单（不变） |
| `files` | 同上 | 每单元产出文件（续跑证据，不变） |
| `incremental` | 同上（可选） | 本次是否增量运行 + 受影响集合，供报告与排障回溯 |

## 四、失败与降级

| 情形 | 行为 |
| --- | --- |
| 无 git / HEAD 不可得 | 全量 + warn（note 入续跑说明） |
| 状态无 `base_sha` | 全量；成功后写入 HEAD |
| 清单缺失/损坏 | 重建清单后再算；重建失败 → 全量 + warn |
| base 不在历史里（rebase/force push） | 全量 + warn，提示基线丢失 |
| 变更文件全在 `docs/` | affected 为空 → 不派单元，直接一致性校验（文档自身被手改也要复算） |

## 五、单测口径（T023/T024 验收）

| 断言 | 说明 |
| --- | --- |
| 改一个文件只命中相关单元 | 构造两个单元，只改 A 的文件 |
| 删除/改名符号命中引用方 | 删 `alpha`，B 里调用过 `alpha` → B 也进集合 |
| 未改动单元不进集合 | 断言 C 不在 affected |
| 允许多、不允许漏 | 保守扫描的边界用例（同名不同模块） |
| 汇编引用受影响单元 → 标记复核 | README 提到 A 的文档路径 |
| 缺 base_sha → 全量 | 断言 note 含降级原因 |
| lint 红 → 全部 done 重做 | 与现有 DocResume 口径一致 |
