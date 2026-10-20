# doc-from-zero · 路线图（实施中）

> 2026-10-19 框架期结束（Q001–Q013 全关，D001–D020 落定）；当夜进入实施，M0–M3 主体已落地。
> 顺序按依赖排：Phase 0 → A → B → C → D → E → F → G，前一阶段 M 达成才进下一阶段。

## Phase 0 · 共用资产（先于一切；同时是 P005/P007 的增量）

- [x] **(T000a) `doc-contract` 落成 skill** — `assets/skills/doc-contract/SKILL.md`；coding-agent / coding-review / doc-fix / doc-review 四处共用（commit `080c1ff`）。
  - 验收：skill 可被渲染进 profile；同一份文本四处引用无副本。
- [x] **(T000b) `doc-lint` 最小版（引擎侧）** — `src/docgen/lint.rs` + CLI `devforge doc-lint`；必填节/链接/Runbook/禁写/实体/置信度/Mermaid（commit `b6c3330`、`a5f80e5`、`72d1145`、`b15660a`）。
- [x] **(T000c) 接入既有链路** — 预审加第四条（doc-lint 退出码 0 + 证据落 stages.md），coding-review 增 doc-lint 复核（commit `f96c77a`）。

## Phase A · 清单（分母）

- [x] **(T001) inventory 抽取器（引擎侧）** — `src/docgen/inventory.rs` + CLI `devforge doc-inventory`；真实仓实测 531 条目（commit `51abc8d`）。
  - **v1 限制（诚实登记）**：靠行首启发式抽公开符号，无 tree-sitter；codegraph 只记 `meta.codegraph_indexed`，本版未消费其图。
- [x] **(T002) 约定面抽取** — 配置键（源码 env 读取 + `.env.example`）、Data（SQL `CREATE TABLE/VIEW`）、构建启动命令（Makefile 目标 + `package.json` scripts）。v1 不解析 pyproject/TOML 与 migration 目录，已在证据文件登记。
- [x] **(T003) 测试即可执行规格** — 从测试反抽行为条目（`spec:` 前缀的 Behavior）：Python `def test_*` / TS `it()|test()` / Go `func Test*` / Rust `#[test] fn`（含源文件内联测试）；每条带 path:line。v1 只取名字与位置，不读断言、不识别 skip/xfail。
- [x] **(T003b) `doc_level` 三档生效** — `audit::DocLevel`（essential 仅 entry / complete 含 module+symbol）。
- [x] **(T003c) 三道闭包校验** — 入口可达 / 文件覆盖 / 符号覆盖，反向验证有单测（删模块/删符号必报错）。
- [x] **(T003d) 编号预分配** — `assign_units` 幂等，单测断言两次调用结果一致。

## Phase B · 编排与供给（骨架）

- [x] **(T004) 新 profile `doc-agent`** — deepseek-flash / high / codegraph+context7 / doc-contract+四语言 skills / coding 同款包（commit `4d2a634`）。
- [x] **(T005) 改 `doc-review` 供给** — 加 codegraph，thinking=medium，不给 context7（同上 commit）。
- [x] **(T006) 注册 SOP `doc-from-zero`** — SopRegistry 5 个；`start` 返回 202 有单测（commit `04af7f4`）。
- [x] **(T007) 协议与路由** — `<doc-complete>` / `<doc-verdict>` / `<route>`；缺/非法 route 保守 phase3；违例上限 3（同上）。
- [x] **(T008) 检查点与幂等续跑** — 落点已于 2026-10-20 改为黑板 `plans/<project>/<task>/doc-from-zero/state.json`（plan 010 D001）；`DEVFORGE_DOC_ONLY` 单点重派保留。

## Phase C · 勘察与解释（事实 / 推断分离）

- [x] **(T009) 勘察步骤** — 提示词写死「只记事实、禁写为什么」，产物 `_meta/facts/<unit>.md`（commit `72d1145`）。
- [x] **(T010) 解释步骤** — 每条断言 🟢/🟡/🔴；🟢 同行必须给 `path:line` 或可重放命令（lint 强制检查）。
- [x] **(T011) 🔴 缺口转问题** — 写 `_meta/gaps.md`；文档有 🔴 却无 gaps.md → lint FAIL。

## Phase D · 生成与汇编

- [x] **(T012) 条目文档生成** — 按契约与必填节表（D014）写；不适用必写 `N/A`；自底向上、跨模块只建引用。（证据：`evidence/t012-generate-assemble-glossary.md`）
- [x] **(T013) 汇编** — 总览 / 架构 / 数据流 / Runbook；**只合成不新增事实**，上层断言可溯源叶子；图为 **Mermaid 源码**（D005）。提示词已补硬约束与锚点测试（证据同上）。
- [x] **(T014) 词表导出与同名异写校验** — `glossary.md` 自动生成；终审机械查"同名异写 / 幽灵实体"（D011）。证据同上。

## Phase E · 审计（doc-review + 引擎）

- [x] **(T015) 引擎侧审计夹具** — 引擎每轮注入覆盖+lint+Runbook 命令块，冠以「确定性、勿质疑」（commit `ad0ecb6`）。
- [x] **(T016) 覆盖审计** — `docgen/audit.rs` 按档位取集合逐条对账；单元级与全局两种。
- [x] **(T017) 真实性 + 置信度诚实性** — lint 实体检查 + 🟢 必须同行给证据（t4/t8）。
- [x] **(T018) 可跑审计** — 引擎抽 Runbook 命令块注入审查，审查官逐条从零 boot（诚实边界：执行是审查侧判定）。
- [x] **(T019) 终审（全局）** — 循环后同 profile 新实例；跨模块重复/全量 lint/覆盖；FAIL 即 run 失败（commit `0cc9106`）。
- [x] **(T020) 单元审（局部）** — `coverage_for_unit` + 单元范围指令；FAIL→route→修→PASS 有实测回路。

## Phase F · 接入与实证

- [x] **(T021) 端到端实证** — fixture `test_22_doc_from_zero.py`：三轮跑完，最后一轮 20m56s / 两单元 / 28 PASS / doc-audit 退出码 0（见 `evidence/t021-m3-e2e.md`，含两次失败归因）。
  - 验收：五类审计可复算；`gaps.md` 存在且每条有原因。
- [x] **(T022) 速度校准** — 四轮实测已定型（`evidence/t022-calibration.md`）：门 0 亚秒级不是瓶颈；成本在单元会话的上下文重发（input:output≈76:1）；明确**不加产品侧硬超时**（挂死是子进程死亡，靠状态判死修）。待做的三项优化已列入，尚未实现。

## Phase G · 增量模式（D015）

- [ ] **(T023) 变更检测 + 语义影响传播** — diff + AST 定位实体 → codegraph 双向遍历得受影响单元。
  - 验收：对一次真实提交，受影响集合与人工判断一致（允许少、不允许漏）。
- [ ] **(T024) 选择性重生成 + 一致性校验** — 只重写受影响单元；链接/图/父文档复核。
- [ ] **(T025) 增量实证** — 同一仓两次提交，报告时间/token 节省与更新召回。

## Milestones

- **M0**（Phase 0）：契约 + doc-lint 可用，coding 链路已接入 ✅
- **M1**（Phase A）：清单可机械产出且三道闭包校验为真 ✅（T002/T003 待补）
- **M2**（Phase B+C）：SOP 可被 API 触发，单元能滚动并断点续跑 ✅
- **M3**（Phase D+E）：产出的文档能过五项审计 ✅（fixture 实证 20m56s / 两单元 / doc-audit rc=0）
- **M4**（Phase F）：真实陌生库端到端有实证数据。
- **M5**（Phase G）：增量模式跑通。

## Deferred（明确不做，避免以后误以为遗漏）

- 截图/浏览器 UI 代理（D019）——只登记为 gaps。
- 多单元并行编排（D017）——串行滚动。
- 文档过期检测 / 定时重跑——归调用方调度。
- 非代码资产（设计稿、数据库 dump）。
- codegraph 不支持语言的补位解析器（D004：登记缺口，不自建）。

## 备注

2026-10-19：框架期结束（Q001–Q013 全关、D001–D020 落定）。本清单进入"待执行"状态——开工按 Phase 顺序，每完成一项把验收证据写进该项下方。
