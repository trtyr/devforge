# P007 Open Questions

> 全部待用户拍板；拍板后移入 decisions/ 并回填 roadmap。
> 2026-10-18 17:56 第二轮讨论后更新：calling→coding 修正（原 Q003 消解），红灯分类学与 review 双根因具体化（见 topics/testing-stage-design.md §四）。

## Q001 归因权与移交协议 —— ✅ 已拍板（2026-10-18 18:23）

用户确认：测试 Agent 看不了代码（黑盒纪律），所以「改码还是改档」的归因权必须归 **coding agent**（唯一看得见两侧的角色）。测试侧只交中性发现记录+复现证据，不猜不判。发现记录字段口径（文档出处/预期/实测/复现步骤/严重度）随 P007 定稿细化。

## Q002 review FAIL 双根因路由

review FAIL 拆两路：根因①测试的错（漏场景/断言与文档矛盾/场景不合理）→ 退回测试 Agent；根因②文档的错/薄（覆盖天花板）→ 移交 coding agent 写文档模式，新文档回测试扩测后重审。与「可写度三级」统一为同一机制的两次触发（写入时+审查时）。

**待确认**：路由拆分是否采纳；review Report 协议需承载根因标注（test-side / doc-side）。

## Q003 「写文档模式」实现位 —— ✅ 已拍板（2026-10-18 18:21）

**独立子 SOP**（用户拍板）。形状：注册进 SopRegistry（如 `doc-fix`），跨 SOP 触发 `start {"sop":"doc-fix"}`，复用 watchdog/黑板/审查全套。定稿时落 decisions/。

## Q004 测试代码落地 —— ✅ 已拍板（2026-10-18 18:21，用户授权助手定）

**已定**：Python 编写；进仓库专门位置；`tests/integration/` + `tests/e2e/` 分层（pytest marker），coding 写 `tests/unit/`（助手决定，业界三层惯例+与 devforge 自身 tests/e2e 互镜像）；commit-on-pass 沿用 D009；抄 tests/AGENTS.md 约定文档惯例。

## Q005 「不看代码」的工程边界

允许读运行面（如何起被测系统、测试命令、公开配置），禁读实现。被测系统由测试 Agent 自起自跑自收。

**待确认**：边界表述进提示词的强度（硬禁令 vs 纪律条款）。

## Q006 未审文档直接入测试（确认项）

极端路径：coding agent 写完文档不经 review 直接交测试 Agent。用户已口头接受（极端+留痕）；落档确认。

## Q007 覆盖判定追溯矩阵

review 产出「文档章节 ↔ 测试用例」追溯矩阵（哪些文档有测试覆盖、哪些零覆盖），作为覆盖面判定的操作化锚点。是否采纳？

## Q008 测试 Agent 供给取舍

不看代码 → codegraph（代码事实层）对 test-agent 无意义甚至违纪，默认**不给**；context7/web tools（运行面对接与库文档查询）是否保留？python-best-practices skill（写 Python 测试）建议保留。test-review 同议。

## Q009 flaky 去抖策略

Doc2Test 实证 flaky 是文档驱动测试的真实痛点（其 flaky 率 11.4%）。同一用例失败先自动重跑 N 次（N=2~3？）再判红，防环境抖动污染 B 类归因。

**待确认**：N 值与是否进第一版。
