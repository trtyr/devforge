# P014 Roadmap · 提示词体系全面改造

> 方法论与骨架见 00-overview.md。批次按 SOP 家族划分，每批独立 commit、
> 全目标测试绿才进下一批。验收：每条过 overview §四检查表 + manifest pinning 同步。

## M0 · 骨架模板与基线 ☐

- ☐ T0.1 六块骨架模板 + 四型（分析/生成/验证/编排）适配版定稿（落 assets/prompts/_template.md，不进 manifest）
- ☐ T0.2 14 条现状基线留档（改造前快照 + token 量，供前后对比）

## M1 · change 规划 + 编码家族 ☐

- ☐ T1.1 change-plan-agent（60 行：规划官——生成型模板；重入纪律/续编号已在此批改过，保持）
- ☐ T1.2 change-plan-review（38 行：计划审查官——验证型模板）
- ☐ T1.3 change-coding-agent（56 行：编码官——生成型；白名单/申报边界/步提交纪律带原因化）
- ☐ T1.4 change-coding-review（27 行：编码审查官——验证型）
- 验收：四条过检查表 + manifest pinning 同步 + 全目标绿

## M2 · testing + 审计家族 ☐

- ☐ T2.1 test-agent / test-review（60/44 行：黑盒测试官 + 测试审查官）
- ☐ T2.2 code-review-agent（39 行：审计批次官——六视角检查清单保留，补边界情况段与「不知道」示例）
- 验收：同 M1

## M3 · doc 家族 ☐

- ☐ T3.1 doc-agent（57 行：文档写作官——生成型；DR-12 不读代码/事实注入纪律保持）
- ☐ T3.2 doc-fix / doc-review（21/29 行）
- 验收：同 M1

## M4 · change 测试 + coding 家族 ☐

- ☐ T4.1 change-test-agent / change-test-review（35/23 行）
- ☐ T4.2 coding-agent / coding-review（66/42 行：MVP 编码官 + 编码审查官）
- 验收：同 M1

## M5 · pi 侧系统提示词（pi-config 仓） ☐

- ☐ T5.1 personality（江晚棠）+ APPEND 工作栈按同标准审改：砍冗余（堆积审计）、
  硬约束前置带原因、禁止带正向替代、动态内容与稳定内容分离
- ☐ T5.2 pi-config 独立提交（sync-guard 域，commit 即推）

## M6 · 终验与回写 ☐

- ☐ T6.1 全目标 cargo test 0 failed + clippy 无新增（双 EXIT 原始日志）
- ☐ T6.2 roadmap 全勾 + overview 补前后对比（token 量/结构检查表结果）+ commit

## 依赖与顺序

M0 → M1 → M2 → M3 → M4 → M6 串行（devforge 侧）；M5 独立可并行。
每批验收不过不进下一批；manifest pinning 断言随批同步，不留到终验。
