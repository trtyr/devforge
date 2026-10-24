//! doc SOP 的提示词与产出目录约定（R4 将外置 assets/prompts/，先同居于此）。

use std::path::Path;

/// 产出目录（D016）：目标项目内。
pub const DOCS_DIR: &str = "docs/contexts";
/// 元数据目录（清单/词表/置信度/缺口）。
pub const META_DIR: &str = "docs/contexts/_meta";

/// 打回回注前缀：必须带回 route，让 agent 回对那一层。
pub const DOC_REMEDIATION_PREFIX: &str = "审查未通过，以下是必须修复的问题（请按 route 回到对应阶段修）：";

/// doc-agent 开场指令：告知仓库、产出目录、本单元、文档档位。
pub fn doc_kickoff(repo: &Path, unit: &str, doc_level: &str) -> String {
    format!(
        "你是文档 Agent：本单元 `{unit}`（档位 `{doc_level}`）。\
\n目标仓库（只读，除了产出目录）：{}\n产出目录：{DOCS_DIR}/（本单元的所有文档写这里；元数据写 {META_DIR}/）\
\n清单：{META_DIR}/inventory.json（先读它，本单元包含哪些文件/符号/入口）\
\n节奏（DR-11 穿插推进，强制）：按源文件粒度边读边写——读完一个源文件，立即在同回合批量写出它的全部文档（多个写调用并行、路径不相交），再进入下一个源文件；本单元的 index/runbook/overview 等枢纽文档压轴最后写（依赖全部叶子）；单回合写入总量 ≤30K tokens 超限拆回合。禁止全部看完再动笔。\
\n纪律：只写 {DOCS_DIR}/ 下；codegraph-first；勘察只记事实、解释标 🟢/🟡/🔴；按 doc-contract 写、不适用写 N/A。\
\n交审前自检后用 <doc-complete>{unit}</doc-complete> 收尾。",
        repo.display()
    )
}

/// 审查指令：全新实例，带轮次号与单元号。
pub fn doc_review_instruction(unit: &str, round: usize) -> String {
    format!(
        "你是文档审查 Agent（第 {round} 轮，审单元 `{unit}`）。\
\n审查面：覆盖（清单条目 ↔ 文档，按档位集合）、完整性（必填节 / N-A 规则）、真实性（实体存在）、置信度诚实性（标 🟢 必须给得出 path:line）、可跑（**照 Runbook 从零 boot 一遍**：装依赖→配置→启动→健康检查→种子数据，跑不起来 = FAIL）。\
\n引擎审计是确定性事实（覆盖/lint/Runbook 命令块），直接采信并逐条验收，不要重算。\
\n**不读代码（DR-12）**：你的输入只有单元文档与引擎审计文本——事实以审计为准，不需要也不应该打开源码；文字质量/结构/一致性才是你的职责。\
\n**输出协议（强制，裁决前置）**：回复的**第一行**就是完整协议块——\
\n<doc-verdict>PASS|FAIL</doc-verdict> 换行 <route>...</route> 换行 <issues>每行一条</issues>——\
\n先把裁决交了，再写审查报告正文并落盘 {META_DIR}/../_meta/reviews/review-{round}.md。\
\n叙述从简：探索过程不复述，报告正文控制在 4000 字内；协议块被截断 = 本轮作废。"
    )
}

/// 终审指令（同一 doc-review profile，全新实例）：
/// 跨模块重复、总览/汇编引用一致性、全量 lint 与覆盖——单元审看不到的东西只在这里查。
pub fn doc_final_instruction(audit: &str) -> String {
    format!(
        "你是文档审查 Agent（**终审轮**，全库范围）。\
\n审查面（单元审没看到的部分）：\
\n1. **跨模块重复**：引擎分两段给——「同名出现（提示级）」只是提示（实体名在多篇里互相引用是正常写法，**不单独判 FAIL**）；「跨模块重复正文」才是判据（同一段解释被两篇以上具体描述 = FAIL，应改成引用）。\
\n2. **汇编层引用**：总览/架构/数据流里的每个引用是否指向存在的叶子文档；上层断言能否溯源到叶子证据。\
\n3. **全量 lint 与覆盖**：引擎审计已给退出码与缺口清单，逐条验收（禁止重算）。\
\n4. **置信度诚实性抽样**：抽 3 条标 🟢 的断言，看能不能真给 path:line。\
\n## 引擎审计（确定性，勿质疑）\n\n{audit}\n\n写 {META_DIR}/../_meta/reviews/final.md，然后输出协议块：<doc-verdict>PASS|FAIL</doc-verdict> + <route>...</route> + <issues>...</issues>。"
    )
}
