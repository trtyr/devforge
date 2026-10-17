# Topic · Coding Review Agent

本质 = Coding Agent 换提示词（同底座同配置面），角色不同：**无状态、用完即弃**（同 plan-review，decisions/004）——每轮全新实例，从头读黑板与代码，出 Report 即死。只管审查、只出 Report，不管进度推进。

## 审查面

1. **通用刚需**（对标 decisions/008）：架构是否 AI-friendly；错误处理/回退/审计/日志/行为审查是否齐全；错误责任分布是否可定位
2. **后端**：功能点 bug、边界与并发、API 语义一致性等（底稿参考本仓 code-review skill 的 bug 透镜与六维剧本）
3. **前端**（项目含前端时）：与后端对接正确性；布局与统一样式（列表/表格/复选框等同一样式系）；交互态完整性——审查清单是活的，使用期人工反馈迭代
4. **前后端连接**：契约一致、数据流贯通
5. **文档审查**：docs/ 全不全、漏没漏、**能否完全基于文档写测试**（不可 = 打回补文档）
6. **进度审计**：对照 todo.md 逐项验实物——没做完不许勾（防绕过）；阶段自报完成度核实
7. **树完成判定**：锚点 = 树的目标**对照实际代码与文档**逐项核验（decisions/011）——stages.md 只是过程工具，划漏了也蒙不过；当前阶段全过后查树还有哪些目标未达成 → advance（还有）或 finish（全达成）

## Review Report 协议（verdict 家族扩展）

每轮写 `coding/reports/review-N.md`（人读）+ 末尾结构化块（机读，planning verdict 同一机制家族）：

- `<stage-verdict>PASS|FAIL</stage-verdict>` — 当前阶段是否合格
- `<issues>…</issues>` — 不合格时的问题清单（回注 Coding Agent 修复）
- `<next>fix|advance|finish</next>` — fix=修完重审；advance=进 N+1；finish=树已全完成

解析容错沿用 planning 经验：缺块/未知值保守按 FAIL（P8-ISSUE-10 同款处理）。
