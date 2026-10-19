{
  "version": 3,
  "id": "mvfafune-7ynfrl",
  "objective": "在 devforge 内实现第二条 SOP「doc-from-zero」到 M3：可被 API 触发、按四道门滚动产出目标项目的 docs/contexts/ 文档体系，并通过五项确定性审计（覆盖/完整性/真实性/置信度诚实性/可跑）。",
  "status": "active",
  "autoContinue": true,
  "usage": {
    "tokensUsed": 254868,
    "activeSeconds": 96
  },
  "sisyphus": false,
  "revision": 43,
  "createdAt": "2026-10-19T13:26:30.890Z",
  "updatedAt": "2026-10-19T13:28:28.807Z",
  "scheduler": {
    "version": 1,
    "owner": "01a1450b-6d3e-77bc-a7c1-bbb88d566132",
    "generation": "bf72990c-2f22-46b4-9e5c-4cbf0a27a90c",
    "used": 1,
    "phase": "idle",
    "repairUsed": false
  },
  "taskList": {
    "tasks": [
      {
        "id": "t1",
        "title": "文档契约落成 skill（T000a）",
        "status": "pending",
        "verificationContract": "契约文本与 topics/doc-contract.md 一致；六类条目模板 + 六条反模式 + 真实性规则齐全；skill 能渲染进 profile，coding/doc-fix/P008 共用同一份无副本"
      },
      {
        "id": "t2",
        "title": "doc-lint 骨架：必填节 / 链接 / Runbook / 禁写扫描（T000b 结构部分）",
        "status": "pending",
        "verificationContract": "对一份故意缺节、死链、Runbook 不全的文档报出具体行号与违例类型；退出码可判（0 通过/非 0 失败）；给出一正一反两份样本的运行记录"
      },
      {
        "id": "t3",
        "title": "清单抽取器 + 三道闭包校验 + 编号预分配（T001/T002/T003/T003b/T003c/T003d）",
        "status": "pending",
        "verificationContract": "对真实仓生成 _meta/inventory.json（条目含 id/kind/path:line/canonical_name/aliases/status）；三道闭包（入口可达/文件覆盖/符号覆盖）可运行；反向验证——从清单删掉一个模块，闭包校验必须报错；单元编号只在门0分配一次"
      },
      {
        "id": "t4",
        "title": "doc-lint 实体存在性 + 同名异写检测（接清单）",
        "status": "pending",
        "verificationContract": "文档里写的幽灵实体 100% 被抓出并给行号；同一实体两种写法被抓出；对真实仓正确文档零误报（附样本）"
      },
      {
        "id": "t5",
        "title": "契约接入 coding 链路（T000c）",
        "status": "pending",
        "verificationContract": "coding 提示词含契约引用一行；预审门要求 doc-lint 退出码 0 且证据落 stages.md；coding-review 有对应复核项；单测锚点断言存在"
      },
      {
        "id": "t6",
        "title": "doc-agent profile 新建 + doc-review 供给改造（T004/T005）",
        "status": "pending",
        "verificationContract": "doc-agent = deepseek-flash/high/codegraph+context7/语言 skills/coding 同款包；doc-review 增 codegraph + thinking_level=medium 且不开 context7；单元测试断言两侧供给符合 D018"
      },
      {
        "id": "t7",
        "title": "SOP 注册 + 协议块 + 检查点续跑（T006/T007/T008）",
        "status": "pending",
        "verificationContract": "start {\"sop\":\"doc-from-zero\"} 返回 202；<doc-complete>/<doc-verdict>/<route> 三块解析有单测；缺/非法 route 保守回 phase3；_meta/state.json 可断点续跑且 --only 单点重派幂等"
      },
      {
        "id": "t8",
        "title": "勘察 / 解释 / 缺口转问题（T009/T010/T011）",
        "status": "pending",
        "verificationContract": "勘察产出只记事实（抽查不含因果解释）；解释每条断言带置信度；标 🟢 的必须能给出 path:line 或重放命令；🔴 全部进 _meta/gaps.md"
      },
      {
        "id": "t9",
        "title": "条目生成 + 汇编 + 词表导出（T012/T013/T014）",
        "status": "pending",
        "verificationContract": "生成遵守必填节表且不适用处写 N/A；汇编层只合成不新增事实（抽查可溯源到叶子）；图为 Mermaid 源码；glossary.md 为清单自动导出且与清单一致"
      },
      {
        "id": "t10",
        "title": "审计夹具 + 四项确定性审计（T015/T016/T017/T018）",
        "status": "pending",
        "verificationContract": "doc-lint 报告由引擎生成并注入审查输入；覆盖审计按档位条目集合对账（缺一条 FAIL）；真实性 100%；标 🟢 给不出证据判 FAIL；Runbook 从零 boot 可执行"
      },
      {
        "id": "t11",
        "title": "单元审 + 终审编排（T019/T020）",
        "status": "pending",
        "verificationContract": "单元审只判本单元；终审查跨模块重复/总览引用/全量 lint；两时机复用同一 doc-review profile；实测一次单元 FAIL→route→修→PASS 的完整回路"
      },
      {
        "id": "t12",
        "title": "端到端单仓跑通 + 报告（M3 验收）",
        "status": "pending",
        "verificationContract": "对一个真实仓完整跑完门0→单元滚动→汇编→终审；产物树符合 D016；五项审计结果可复算；墙钟/单元数/审查轮次/token 落 _meta/runs/<run-id>/"
      }
    ],
    "blockCompletion": true,
    "proposedAt": "2026-10-19T13:26:06.113Z"
  },
  "activePath": ".pi/goals/active_goal_2026101921263089_mvfafune-7ynfrl.md"
}

# Goal Prompt

在 devforge 内实现第二条 SOP「doc-from-zero」到 M3：可被 API 触发、按四道门滚动产出目标项目的 docs/contexts/ 文档体系，并通过五项确定性审计（覆盖/完整性/真实性/置信度诚实性/可跑）。

## Progress

- Status: running
- Auto-continue: on
- Sisyphus mode: no
- Time spent: 1m36s
- Tokens used: 255K (254,868) tokens
## Tasks

<!-- blockCompletion: true -->
- [ ] t1: 文档契约落成 skill（T000a） — contract: 契约文本与 topics/doc-contract.md 一致；六类条目模板 + 六条反模式 + 真实性规则齐全；skill 能渲染进 profile，coding/doc-fix/P008 共用同一份无副本
- [ ] t2: doc-lint 骨架：必填节 / 链接 / Runbook / 禁写扫描（T000b 结构部分） — contract: 对一份故意缺节、死链、Runbook 不全的文档报出具体行号与违例类型；退出码可判（0 通过/非 0 失败）；给出一正一反两份样本的运行记录
- [ ] t3: 清单抽取器 + 三道闭包校验 + 编号预分配（T001/T002/T003/T003b/T003c/T003d） — contract: 对真实仓生成 _meta/inventory.json（条目含 id/kind/path:line/canonical_name/aliases/status）；三道闭包（入口可达/文件覆盖/符号覆盖）可运行；反向验证——从清单删掉一个模块，闭包校验必须报错；单元编号只在门0分配一次
- [ ] t4: doc-lint 实体存在性 + 同名异写检测（接清单） — contract: 文档里写的幽灵实体 100% 被抓出并给行号；同一实体两种写法被抓出；对真实仓正确文档零误报（附样本）
- [ ] t5: 契约接入 coding 链路（T000c） — contract: coding 提示词含契约引用一行；预审门要求 doc-lint 退出码 0 且证据落 stages.md；coding-review 有对应复核项；单测锚点断言存在
- [ ] t6: doc-agent profile 新建 + doc-review 供给改造（T004/T005） — contract: doc-agent = deepseek-flash/high/codegraph+context7/语言 skills/coding 同款包；doc-review 增 codegraph + thinking_level=medium 且不开 context7；单元测试断言两侧供给符合 D018
- [ ] t7: SOP 注册 + 协议块 + 检查点续跑（T006/T007/T008） — contract: start {"sop":"doc-from-zero"} 返回 202；<doc-complete>/<doc-verdict>/<route> 三块解析有单测；缺/非法 route 保守回 phase3；_meta/state.json 可断点续跑且 --only 单点重派幂等
- [ ] t8: 勘察 / 解释 / 缺口转问题（T009/T010/T011） — contract: 勘察产出只记事实（抽查不含因果解释）；解释每条断言带置信度；标 🟢 的必须能给出 path:line 或重放命令；🔴 全部进 _meta/gaps.md
- [ ] t9: 条目生成 + 汇编 + 词表导出（T012/T013/T014） — contract: 生成遵守必填节表且不适用处写 N/A；汇编层只合成不新增事实（抽查可溯源到叶子）；图为 Mermaid 源码；glossary.md 为清单自动导出且与清单一致
- [ ] t10: 审计夹具 + 四项确定性审计（T015/T016/T017/T018） — contract: doc-lint 报告由引擎生成并注入审查输入；覆盖审计按档位条目集合对账（缺一条 FAIL）；真实性 100%；标 🟢 给不出证据判 FAIL；Runbook 从零 boot 可执行
- [ ] t11: 单元审 + 终审编排（T019/T020） — contract: 单元审只判本单元；终审查跨模块重复/总览引用/全量 lint；两时机复用同一 doc-review profile；实测一次单元 FAIL→route→修→PASS 的完整回路
- [ ] t12: 端到端单仓跑通 + 报告（M3 验收） — contract: 对一个真实仓完整跑完门0→单元滚动→汇编→终审；产物树符合 D016；五项审计结果可复算；墙钟/单元数/审查轮次/token 落 _meta/runs/<run-id>/

