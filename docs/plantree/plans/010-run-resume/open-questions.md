# 未决问题（`010-run-resume`）

## Q001 · 与 `009` D022 的位置冲突：`changes/<id>/state.json` 要不要也搬去黑板？

P009 D022 定的是「引擎写 `docs/plantree/changes/<change-id>/state.json`，存 phase / baseline_sha / roundtrips / impl_steps_done / acceptance_done / review_rounds」。

按本计划 D001（运行状态住黑板、产物目录只放产物），这条口径该改：那份 state 是**运行进度**，不是交付物；它跟着目标仓走反而会在「同一个 change 被另一台机器/另一个会话接着做」时失真（黑板是 run 的，产物是仓的，两者生命周期不同）。

- **待定**：改 P009 D022 为黑板（`plans/<p>/<t>/state.json` 内含 change 扩展段），`changes/<id>/` 只留 `change-plan.md` / `history.md` / `report.md`。
- **影响**：P009 roadmap T011b（checkpoint + resume）的落点跟着变；D022 需要写取代条目。

## Q002 · 黑板被清空时，能不能从产物重建断点？

产物目录里有 `_meta/inventory.json`、`glossary.md`、`gates` 证据与正文文档——理论上可以推断「哪些单元写过」。但推断出来的 `done` 没有审查记录背书（PASS 结论只在黑板里）。

- **倾向**：不重建。黑板丢了就冷启动（`fresh` 语义），并让 `doc-audit` 暴露覆盖缺口。假绿比白跑更贵。
- **待定**：确认「宁可重跑，不猜」。

## Q003 · 自动重启与串行 guard、run 记录的交互

一个 run 失败后自动重启，是**同一个 run id 继续**（`attempt` 递增、run 记录不新增）还是**新起一个 run**？

- **倾向**：同一 run id、`attempt` 递增，UI 上表现为「该 run 第 N 次尝试」；串行 guard 全程持有，中途不让别的 run 插进来。
- **待定**：重启期间 `GET /runs` 的状态字段要不要新增 `Retrying`，还是留在 `Running` 用 `attempt` 表达。

## Q005 · 同一个 task 上真的会并发跑不同 SOP 吗？

现状是**串行 guard（同一 task 同时只能一个 run）**，所以「doc 与 change 同时落在同一 state 文件」在同一个 task 上不会真的同时发生；但**先后、交替**跑会（今天跑文档、明天加功能、下周重新跑文档），而一份装所有 SOP 的状态就会相互覆盖。本计划按「project/task/sop 三级分文件」解决这个问题，**不放开并发**。

- **待定**：以后若要真并发（同一 task 同时两个 SOP），需要另外改串行 guard 与黑板锁；不属于本计划。

## Q004 · 续跑的边界条件：产物与代码不同步怎么办

续跑时目标仓的代码可能已经变了（人改了代码），而 `inventory.json` 是上一轮的。

- **倾向**：门 0 照旧重算清单；若清单指纹与 state 记录的不一致，则把受影响单元的 `done` 全部作废（保守：全量重做 vs 增量重做，增量归 P008 T023–T025，本计划不碰）。
- **待定**：指纹取什么（文件集 + mtime？还是 `git` HEAD？无 git 的仓怎么办）。
