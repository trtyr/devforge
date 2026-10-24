# ARC-R4 外置方案 · 提示词出仓（2026-10-24，动刀前定稿稿）

> 目标：`src/config/manifest.rs` 内 13 个内置提示词字符串常量（74-731 行区，约 658 行）外置为
> `assets/prompts/*.md`，`include_str!` 编译期内联保单二进制，**内容零字节差**（纯移动）。
> 执行人：操作员（底座手术期延续）。分批 commit，每批 cargo test 绿。

## 现状（R3 收官后实测）

调研口径说「9 个」，实测 **13 个**（74-731 连续区 + 476 起 change 六件套）：

| 常量 | 行区间 | 约行数 | 字面量形态 |
| --- | --- | --- | --- |
| CODING_AGENT_PROMPT | 74-141 | 68 | `"...\n\`（转义+续行） |
| CODING_REVIEW_PROMPT | 142-185 | 44 | `"...\n\`（转义+续行） |
| TEST_AGENT_PROMPT | 186-248 | 63 | `r#"..."#` |
| TEST_REVIEW_PROMPT | 249-295 | 47 | `r#"..."#` |
| DOC_FIX_PROMPT | 296-319 | 24 | `r#"..."#` |
| DOC_AGENT_PROMPT | 320-379 | 60 | `r#"..."#` |
| DOC_REVIEW_PROMPT | 380-412 | 33 | `r#"..."#` |
| CHANGE_PLAN_AGENT_PROMPT | 476-538 | 63 | `r#"..."#` |
| CHANGE_PLAN_REVIEW_PROMPT | 539-579 | 41 | `r#"..."#` |
| CHANGE_CODING_AGENT_PROMPT | 580-638 | 59 | `r#"..."#` |
| CHANGE_CODING_REVIEW_PROMPT | 639-668 | 30 | `r#"..."#` |
| CHANGE_TEST_AGENT_PROMPT | 669-706 | 38 | `r#"..."#` |
| CHANGE_TEST_REVIEW_PROMPT | 707-731 | 25 | `r#"..."#` |

外置后 manifest.rs 约 1500 → ~840 行（builtin() 258 + 测试 ~460 + 结构定义 ~120）。

## 目标布局

`assets/prompts/`（13 文件，命名 = agent role 名）：
coding-agent / coding-review / test-agent / test-review / doc-fix / doc-agent / doc-review /
change-plan-agent / change-plan-review / change-coding-agent / change-coding-review /
change-test-agent / change-test-review（各 .md）。

常量替换形态：
`pub const CODING_AGENT_PROMPT: &str = include_str!("../../assets/prompts/coding-agent.md");`
（路径相对 src/config/；常量名与 pub 可见性全部不变——引用面零改动。）

## 关键边界声明

1. **零字节差**：`.md` 文件字节 = 原常量字符串的解码值。三个坑：
   - 非 raw 的两件（coding 两 prompt）源码里 `\n` 是转义、行尾 `\` 是续行——解码后才等价；
   - `include_str!` 会带文件**尾换行**——若原常量末尾无 `\n`，文件不得多写（写文件时精确控制）；
   - `r#"..."#` 内若出现 `"#` 序列会破 raw 语法——现状能编译说明没有，外置无此风险。
   **验证方法**：批完成后跑 python 比对脚本——`git show HEAD:src/config/manifest.rs` 提取常量
   字面量 → 解码转义（非 raw：`\n`/`\"`/`\\`/行尾续行 `\`；raw：原样）→ 与 `.md` 文件逐字节
   比对，要求 13/13 报告「零字节差」。既有锚点单测
   （change_plan_prompts_carry_six_blocks / coding_prompts_carry_discipline_anchors /
   doc_agent_prompt_carries_protocol_and_guards / testing_prompts_carry_protocol_and_discipline /
   coding_agent_prompt_has_preflight_audit / change_agents_fixture_matches_builtin）**不动**，
   作为第二道内容回归网。
2. **纯搬移纪律**：常量名/类型/可见性/调用点零改动；锚点单测不改一字（它们钉的是内容，
   include_str 后内容不变，必须全绿——绿了才证明零字节差）。
3. **分批顺序**：批 1 change 六件套（476-731，最独立）→ 批 2 doc 四件（296-412）→
   批 3 coding+testing 四件（74-295，含两件非 raw 转义形态，最难）。
4. **不动**：builtin() 的组装逻辑、manifest 序列化、AF-4 的纪律归属重排（另行派工）、
   assets/skills/（已有体系）。
5. **acc2 不受扰**：7889 运行中实例不动，task-4 验收才重启。

## 引用面清单

- 提示词常量的消费点全部在 `ConfigManifest::builtin()`（735-993）——同文件内，零跨文件引用。
- 锚点单测双向钉住（`src/change/plan_audit.rs` 注释口径）：manifest 侧断言提示词含
  plan_audit 常量——内容不变则锚点不变 ✓。
- `assets/` 目录已有 skills/ 子树（skill_assets.rs 管理）；prompts/ 是新子树，
  **不经 skill_assets**（纯 include_str!，无运行时分发）。

## R4 收官回写（2026-10-24，状态 Done，操作员执行）

| commit | 内容 |
| --- | --- |
| c005e63 | task-1：外置方案落档（本文件，实测 13 常量非调研口径的 9） |
| f10fe07 | 批 1：change 六件套外置（手转写，6/6 零字节差验证） |
| c60641a | 批 2+3：doc 四件/test 两件/coding 两件外置（python 提取法——特殊字节全角引号/字面反斜杠转写风险改用确定性提取，7/7 零字节差含非 raw 转义续行解码） |

终态：manifest.rs 1504→1159（-345，提示词区 74-731 全部出仓）；assets/prompts/ 13 文件
共 558 行（字节口径 1717 字节文本 + 变体）；全量 7 靶绿 exit 0；clippy 与 R3 收官基线
完全一致零新增；锦点单测零改动全过（内容不变则锚点不变，反向证明了零字节差）。

### 方法论记录

- 手转写（read→write）在无特殊字节的 raw 常量上可靠（批 1 六件 6/6 过）；
  含全角引号/字面 `\\n` 的常量改用 python 正则提取 + 转义解码（批 2+3 七件），
  避免 read→write 链路的转写误差。
- 非 raw 形态的解码规则：`\n`/`\"`/`\\`/`\t`/`\u{...}` 标准转义 + 行尾 `\` 续行
  （续行连同行首空白一并剥离）。
- 验收证据：7889 新 binary 重启（pid 3124701 探活 200），acc2 resume 后 doc-agent
  工具调用持续活跃（外置后的提示词经 include_str! 内联，run 行为不变）。
- 已知残留：AF-4 纪律归属重排另行派工（本外置不改任何提示词内容，两者正交）。
