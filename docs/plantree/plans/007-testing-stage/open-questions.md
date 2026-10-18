# P007 Open Questions

> 全部待用户拍板；拍板后移入 decisions/ 并回填 roadmap。

## Q001 红灯归因路由

测试跑挂 = 测试误读文档（修测试）或代码不符合文档（实现 bug）。测试 Agent 无归因能力（不看代码），自查文档两轮为限仍对不上时，「文档-代码分歧」的处置：回流 coding SOP 修码（跨 SOP 触发回环）？还是移交 calling agent 裁决？

*助手倾向：回流 coding（分歧的修复方是代码），calling agent 只在「文档本身不足」时介入——但与 Q003 定位联动。*

## Q002 review FAIL 双根因路由

review FAIL 拆两路：「测试写错/场景不合理 → 退回测试 Agent 修」vs「覆盖不够且根因是文档太薄 → 移交 calling agent（写文档模式）」。是否采纳？

## Q003 calling 阶段定位

calling agent / calling 阶段在流水线的位置：planning → coding → **calling** → testing 串行第五环？还是横切旁路（随时可被 testing/coding 移交的文档服务）？「写文档模式」是独立 SOP 还是 calling agent 的模式位？

## Q004 测试代码落地

测试套件是否进仓库（commit-on-pass，D009 同款）成为常驻回归资产（以后 coding 每次改动可回归）？助手强烈倾向进仓库。

## Q005 「不看代码」的工程边界

集成/e2e 天然走公开面（HTTP/CLI）。建议边界：**允许读运行面**（如何起被测系统、测试命令、配置），**禁读实现**。是否采纳？

## Q006 未审文档直接入测试（确认项）

极端路径：calling agent 写完文档不经 review 直接交测试 Agent。用户已口头接受（极端+留痕）；落档确认。

## Q007 覆盖判定追溯矩阵

review 产出「文档章节 ↔ 测试用例」追溯矩阵（哪些文档有测试覆盖、哪些零覆盖），作为覆盖面判定的操作化锚点。是否采纳？

## Q008 测试 Agent 供给取舍

不看代码 → codegraph（代码事实层）对 test-agent 无意义甚至违纪，默认**不给**；context7/web tools（运行面对接与库文档查询）是否保留？test-review 同议。
