# devforge agent 提示词骨架模板（P014 · 不进 manifest）

> 改造依据：四篇 2026 指南（ingest 见 engram wiki）。本模板是 14 条提示词重写的
> 唯一结构依据；每条重写后逐项过 §五检查表。

## 一、六块结构（固定顺序，不可换）

1. **身份**（≤2 句）：角色 + 产出读者 + 消费场景（「谁读、什么状态下读」）。
2. **硬约束**（紧跟身份，吃注意力峰值）：每条带原因；禁止必带正向替代
   （❌「不要读代码」✅「代码事实一律取自引擎注入的审计段——自己读会引入未经核实的断言」）。
3. **流程**：编号步骤，每步带判定标准。按 agent 类型取 §二 对应模板的流程骨架。
4. **边界情况**：空输入 / 歧义 / 超界 / 前置缺失——每项给明确处置动作，不许「酌情」。
5. **输出契约**：协议块形态 + 各字段含义 + **错误/无产出时返回什么**（不许自由文本混入协议外）。
6. **示例**（仅格式硬需求时）：XML `<example>` 包裹，≥1 条「不知道/失败」路径示例，
   贴在提示词尾部（离用户消息最近，影响力最强）。

## 二、四型流程骨架（按 agent 职责取用）

- **分析型**（读产物找问题）：gather context → initial scan → deep analysis（分维度）→
  synthesize（同类合并）→ prioritize（按严重度）→ report。
- **生成型**（写代码/文档/计划）：understand requirements → gather context → design →
  generate（按约定）→ self-validate → document。
- **验证型**（判 PASS/FAIL）：load criteria → scan target → check rules（逐条）→
  collect violations（带位置）→ assess severity → verdict（只有达标才 PASS）。
- **编排型**（多阶段协调）：plan → prepare → execute phases → monitor → verify → report。

## 三、写法纪律

- 第二人称（「你负责…」不是「该 agent 负责…」）。
- 每条规则回答「这行修的是哪个真实见过的失败」——答不出就删。
- 禁止式约束必须带正向替代与原因（三件套）。
- 具体化：❌「关注安全问题」✅「逐个检查数据库查询是否参数化，非参数化记 critical」。
- 能被引擎机械校验的（协议块形态）不靠提示词重复祈祷——引擎 verify 层负责。
- 动态值（change-id/基线/batch 清单）不写死，标注注入通道。
- ≤1200 token；新增内容从删除冗余中换。

## 四、XML 约定

- 块边界用 XML：`<role>` `<rules>` `<process>` `<edge-cases>` `<output-contract>` `<example>`。
- 指令内部用 markdown 列表；数据与示例必须包在 XML 内（划清「文档止、输入始」的边界）。

## 五、检查表（每条重写后逐项勾）

- [ ] 六块齐备且顺序正确
- [ ] 每条硬约束带原因 + 禁止带正向替代
- [ ] 有边界情况段（空输入与歧义有处置动作）
- [ ] 输出契约含错误/无产出时行为
- [ ] ≥1 条「不知道」路径示例（或明确的拒绝/上报动作）
- [ ] ≤1200 token
- [ ] manifest pinning 断言同步（涉及内容子串的）
- [ ] 全目标 cargo test 0 failed
