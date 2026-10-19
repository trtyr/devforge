# T000c · 契约与 doc-lint 接入 coding 链路 证据

- 日期：2026-10-19
- 改动：`src/config/manifest.rs`（CODING_AGENT_PROMPT / CODING_REVIEW_PROMPT）

## 一、编码侧（写的人）

`提交前预审` 从三条变四条，新增：

> **4. 文档门禁（doc-lint）**：文档按 `doc-contract` skill 写。发 stage-complete 前在代码工作区跑
> `devforge doc-lint <docs 目录> --repo <代码工作区>`，**退出码必须 0**；必填节缺失、幽灵实体/路径、死链以工具输出为准（不适用必须写 `N/A` 并给理由，省略不算）。命令原文 + 退出码 + 输出摘要写进 `coding/stages.md`。

`<stage-complete>` 的触发条件同步改为「预审**四条**全过（含 doc-lint 退出码 0）」。

## 二、审查侧（审的人）

审查面新增第 6 项：

> **doc-lint 复核（确定门禁）**：自己跑一遍 `devforge doc-lint <docs 目录> --repo <代码工作区>`，退出码非 0 = 文档 FAIL；抽查两条——必填节是否真对着契约（缺节且无 `N/A` = 撒谎）、文档提到的实体是否真存在（工具报幽灵而声称写了 = 撒谎）。**别只看 coder 自述的退出码。**

## 三、单测锚点

`coding_agent_prompt_has_preflight_audit` 追加断言：prompt 含 `doc-contract`、`devforge doc-lint`、`退出码必须 0`；review prompt 含 `doc-lint 复核`、`自己跑一遍`。

## 四、门禁

`cargo test` 220 通过 / 0 失败；`cargo clippy --all-targets` 0 警告。

## 五、为什么是门禁不是更多提示词

规则写进提示词只是"请求"，写进可执行检查才是"约束"：coder 可以声称文档写全了，但退出码不会撒谎，而审查官被要求自己重跑一遍——两道都是**可复算**的证据，不是叙述。
