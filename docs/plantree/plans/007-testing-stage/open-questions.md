# P007 Open Questions

> 全部待用户拍板；拍板后移入 decisions/ 并回填 roadmap。
> 2026-10-18 17:56 第二轮讨论后更新：calling→coding 修正（原 Q003 消解），红灯分类学与 review 双根因具体化（见 topics/testing-stage-design.md §四）。

## Q001 归因权与移交协议 —— ✅ 已拍板（2026-10-18 18:23）

用户确认：测试 Agent 看不了代码（黑盒纪律），所以「改码还是改档」的归因权必须归 **coding agent**（唯一看得见两侧的角色）。测试侧只交中性发现记录+复现证据，不猜不判。发现记录字段口径（文档出处/预期/实测/复现步骤/严重度）随 P007 定稿细化。

## Q002 review FAIL 双根因路由 —— ✅ 已拍板（2026-10-18 18:25）

用户拍板方向 + 实现位：**本质是路由问题，从工具侧实现，不经人工读报告**。

- 根因①test-side（测试的错：漏场景/断言与文档矛盾/场景不合理）→ 状态机自动退回测试 Agent（附 issues 重推）
- 根因②doc-side（文档太薄撑不起覆盖）→ 状态机自动触发 doc-fix 子 SOP（Q003 机制），新文档回测后重审

**实现形状**：沿用 coding 阶段已实证的结构化协议标签模式——test-review 输出增 `<reroute>test-side|doc-side</reroute>` 字段，TestingSopLoop 解析标签路由（保守解析：标签缺失/非法 → 按 test-side 退回，绝不静默吞）；黑板 markdown 报告仅作留档审计，路由只认标签。定稿时落 decisions/。

## Q003 「写文档模式」实现位 —— ✅ 已拍板（2026-10-18 18:21）

**独立子 SOP**（用户拍板）。形状：注册进 SopRegistry（如 `doc-fix`），跨 SOP 触发 `start {"sop":"doc-fix"}`，复用 watchdog/黑板/审查全套。定稿时落 decisions/。

## Q004 测试代码落地 —— ✅ 已拍板（2026-10-18 18:21，用户授权助手定）

**已定**：Python 编写；进仓库专门位置；`tests/integration/` + `tests/e2e/` 分层（pytest marker），coding 写 `tests/unit/`（助手决定，业界三层惯例+与 devforge 自身 tests/e2e 互镜像）；commit-on-pass 沿用 D009；抄 tests/AGENTS.md 约定文档惯例。

## Q005 「不看代码」的工程边界 —— ✅ 已拍板（2026-10-18 18:45）

**硬禁令 + 白名单 + review 审计兑底**（用户拍板）：

- 白名单：文档全量（含 Runbook）/ 运行面配置（依赖段/pytest 配置/compose 编排段）/ 目录结构（路径）/ 自写测试文件 / 被测系统公开输出（HTTP 响应、日志、含 stack trace）
- 一句绝对禁令收口：除白名单外禁止读取任何实现文件内容；判定线=路径可看、内容不能读
- 边角：stack trace 属系统输出按黑盒证据记录，不许据此翻源码；缺资料（如 schema 未披露）= doc-side 发现走 doc-fix，不是偷看理由
- 兜底：test-review 审查面加一条——测试断言了文档未承诺的行为 → 视为疑似偷看打回（提示词纪律非沙箱，诚实声明）

## Q006 未审文档直接入测试 —— ✅ 已拍板（2026-10-18 18:47）

**选 A：doc-fix 子 SOP 内置 review 轮**（用户拍板推荐案）。fresh 审查官审「新文档 ↔ plan tree」一致性 + 文档内部自洽 + Runbook 契约，过了才流回测试环节。机制成本≈零（review factory/per-round/watchdog 全现成）；「未审文档」场景从结构上消除，残留风险（错文档被自洽测试认证）同步消除。

## Q007 覆盖判定追溯矩阵 —— ✅ 已拍板（2026-10-18 18:49）

采纳（用户拍板）。黑板活表格 `testing/coverage-matrix.md`：文档章节↔需求点↔测试用例↔状态四列；测试 Agent 写测时维护，test-review 审计（抓漏行+抓撒谎行）；保守规则：文档有而矩阵无 → 默认零覆盖。收益：覆盖判定查表化、FAIL 证据具体化、根因按行标（衔接 Q002）、流水线管理面可查产物（RTM 教科书实践）。

## Q008 测试 Agent 供给取舍 —— ✅ 已拍板（2026-10-18 18:52）

原则：**context7/web 是外部世界的知识，给；codegraph/pi-lens 是产品内部的捷径，扣**（工具箱形状=角色边界，供给即纪律）。

- test-agent：base 四件套照常 + extras [rpiv-todo, rpiv-ask-user-question] + MCP context7 + skill 仅 python-best-practices（rust/go/ts 不给）
- test-review：无插件、极简（核心工具自带，可配 context7）
- **不给**：codegraph、pi-lens（代码事实层工具，黑盒纪律下纯诱惑通道）、plan-tree skill（读不写，黑板自有协议）

## Q009 flaky 去抖策略

Doc2Test 实证 flaky 是文档驱动测试的真实痛点（其 flaky 率 11.4%）。同一用例失败先自动重跑 N 次（N=2~3？）再判红，防环境抖动污染 B 类归因。

**待确认**：N 值与是否进第一版。
