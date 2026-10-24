//! 配置清单 schema（P002 · 配置即代码的真源）。
//!
//! 两层（P002 decisions/002/003）：
//! - 模板层（写死）：role_prompt、必装四件套、skills、（MCP/Key 内联于此 schema）
//! - 模型层（可变）：模型池 + Agent 引用——系统定型后唯一长期开放的自定义维度

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

/// 必装四件套——用户拍板（2026-10-16），包名原文照录，所有 Agent 必装。
pub const BASE_PACKAGES: &[&str] = &[
    "npm:@ff-labs/pi-fff",
    "npm:@juicesharp/rpiv-web-tools",
    "npm:pi-blackhole",
    "git:github.com/NVlabs/SoL-Pi",
];

/// 差异化包：仅 Plan Agent（完成确认机制依赖 ask-user-question，P001 decisions/008）。
pub const PLAN_AGENT_EXTRA: &str = "npm:@juicesharp/rpiv-ask-user-question";

/// 差异化包：Coding Agent（P005：ask-user-question 偏差协议 D005 + todo 纪律 D002 + 诊断层 D014）。
pub const CODING_AGENT_EXTRA: &[&str] = &[
    "npm:@juicesharp/rpiv-ask-user-question",
    "npm:@juicesharp/rpiv-todo",
    "npm:pi-lens",
];

/// 差异化包：Coding Review（诊断证据，D014；无状态一轮不配 todo）。
pub const CODING_REVIEW_EXTRA: &[&str] = &["npm:pi-lens"];

/// 差异化包：Test Agent（P007 D007：todo 纪律 + 偏差出口；不给 pi-lens——黑盒纪律）。
pub const TEST_AGENT_EXTRA: &[&str] = &[
    "npm:@juicesharp/rpiv-todo",
    "npm:@juicesharp/rpiv-ask-user-question",
];

/// MCP server 定义（P005 T003：渲染 `<agent-dir>/mcp.json`，pi 原生 schema）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    /// server 名（工具名 `mcp__<name>__<tool>`；字母数字下划线连字符）。
    pub name: String,
    /// 可执行程序（stdio）。
    pub command: String,
    pub args: Vec<String>,
    /// 环境变量注入（如 CODEGRAPH_TELEMETRY=0，decisions/013 卫生条款）。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
}

/// CodeGraph 事实层（decisions/013：单工具 explore 起步，引擎预检 init，agent 不自行 init）。
pub fn codegraph_mcp() -> McpServer {
    McpServer {
        name: "codegraph".into(),
        command: "codegraph".into(),
        args: vec!["serve".into(), "--mcp".into()],
        env: [("CODEGRAPH_TELEMETRY".to_string(), "0".to_string())].into_iter().collect(),
    }
}

/// context7 最新库文档（偏差协议 D005 的技术求证弹药）。
pub fn context7_mcp() -> McpServer {
    McpServer {
        name: "context7".into(),
        command: "npx".into(),
        args: vec!["-y".into(), "@upstash/context7-mcp".into()],
        env: Default::default(),
    }
}

/// Coding Agent 提示词（P005 T004 正稿；决策链 D001-D014 全覆盖，勿手改——改决策先改 plan tree）。
pub const CODING_AGENT_PROMPT: &str = include_str!("../../assets/prompts/coding-agent.md");

/// Coding Review 提示词（P005 T005 正稿；七审查面 + Report 协议，勿手改——改决策先改 plan tree）。
pub const CODING_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/coding-review.md");

/// Test Agent 提示词（P007 T001 正稿；D001/D002/D005/D006/D007/D009/D010 落地，勿手改——改决策先改 plan tree）。
pub const TEST_AGENT_PROMPT: &str = include_str!("../../assets/prompts/test-agent.md");

/// Test Review 提示词（P007 T001 正稿；D003 reroute 标签 + 对文档三判据落地，勿手改——改决策先改 plan tree）。
pub const TEST_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/test-review.md");

/// Doc Fix 提示词（P007 T001 正稿；D004 写文档模式落地，勿手改——改决策先改 plan tree）。
pub const DOC_FIX_PROMPT: &str = include_str!("../../assets/prompts/doc-fix.md");

/// Doc Agent 提示词（P008 D009/D010/D016/D020）——doc-from-zero 的干活方。
pub const DOC_AGENT_PROMPT: &str = include_str!("../../assets/prompts/doc-agent.md");

/// Doc Review 提示词（P007 T001 正稿；D004/D008 一致性审查落地，勿手改——改决策先改 plan tree）。
pub const DOC_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/doc-review.md");

/// 模型管理面：系统级模型池条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// 池内引用 id（Agent 用它选模型，如 `"deepseek-flash"`）。
    pub id: String,
    /// pi settings 的 defaultProvider。
    pub provider: String,
    /// pi settings 的 defaultModel。
    pub model_id: String,
    /// 上下文窗口（渲染 models.json，P2-ISSUE-03 修复）。
    pub context_window: u64,
    /// 最大输出 tokens（渲染 models.json；缺失时 reasoning 模型烧光默认预算产出空正文）。
    pub max_tokens: u64,
}

/// provider 定义（渲染 models.json 用，P2-ISSUE-03 修复：pi 由此得知 baseUrl）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderDef {
    /// provider 名（pi settings 的 defaultProvider / auth.json 键）。
    pub name: String,
    /// 协议族（如 "openai-completions"）。
    pub api: String,
    /// 服务地址（如自建中转）。
    pub base_url: String,
}

/// Agent 管理面：单个 Agent 的配置清单。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentManifest {
    /// Agent 名（= profile 目录名，如 `plan-agent` / `plan-review`）。
    pub name: String,
    /// AGENTS.md 正文（模板层，写死；完整提示词在 P001 T005 细化）。
    pub role_prompt: String,
    /// 差异化包（在必装四件套之上追加）。
    pub extra_packages: Vec<String>,
    /// 引用模型池条目 id（模型层）。
    pub model: String,
    /// 需要分发到 profile 的 skills（如 `plan-tree`）。
    pub skills: Vec<String>,
    /// MCP servers（渲染 `<agent-dir>/mcp.json`；空 = 不生成该文件）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<McpServer>,
    /// 思考档位覆盖（settings.json `defaultThinkingLevel`）。
    /// None = 全局默认 high。审查 Agent 实测每轮 11-22s、turns 45-71（输出 token 占大头），
    /// 降档是当前最有效的提速手段（2026-10-19 性能剖析）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// 配置清单——真源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigManifest {
    /// 必装四件套（所有 Agent）。
    pub base_packages: Vec<String>,
    /// provider 定义表（模型管理面，渲染 models.json）。
    pub providers: Vec<ProviderDef>,
    /// 模型池（模型管理面）。
    pub model_pool: Vec<ModelEntry>,
    /// Agent 清单（Agent 管理面）。
    pub agents: Vec<AgentManifest>,
    /// API Key——明文不设防（P002 decisions/001）。
    pub api_key: Option<String>,
}

/// Change Plan Agent 提示词（P009 T006/D005-D017）：把需求落到代码上，产出六块计划。
pub const CHANGE_PLAN_AGENT_PROMPT: &str = include_str!("../../assets/prompts/change-plan-agent.md");

/// Change Plan Review 提示词（P009 T007/D012）：只审计划，不看 diff、不跑命令、不写文件。
pub const CHANGE_PLAN_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/change-plan-review.md");

/// Change Coding Agent 提示词（P009 T008b/D014-D018）：按计划改存量代码，不越界、不设计。
pub const CHANGE_CODING_AGENT_PROMPT: &str = include_str!("../../assets/prompts/change-coding-agent.md");

/// Change Coding Review 提示词（P009 T008b/D016）：核 diff 与计划，不设计、不扩围。
pub const CHANGE_CODING_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/change-coding-review.md");

/// Change Test Agent 提示词（P009 testing 阶段）：黑盒验证「改对了」且「没弄坏别的」。
pub const CHANGE_TEST_AGENT_PROMPT: &str = include_str!("../../assets/prompts/change-test-agent.md");

/// Change Test Review 提示词：审测试是否真证明了变更且没漏验收。
pub const CHANGE_TEST_REVIEW_PROMPT: &str = include_str!("../../assets/prompts/change-test-review.md");

impl ConfigManifest {
    /// 内置默认清单：规划+编码四 Agent + 固定模型池（2026-10-17 用户拍板固定死：
    /// 审查类 Agent → Grok 4.7，编码/规划 Agent → DeepSeek flash；真实 id 经 P1-CAL-01 校准）。
    pub fn builtin() -> Self {
        let plan_agent_prompt = "\
# Plan Agent（规划 Agent）\n\
\n\
你是 devforge 系统的规划 Agent。\n\
\n\
职责：\n\
1. 读取 plan-tree skill，理解 plan tree 的结构约定。\n\
2. 与用户沟通（对话或 ask-user-question），澄清需求。\n\
3. 产出 plan tree 保存到当前目录（任务目录即 plan root）。\n\
4. 结束前必须用 ask-user-question 问用户是否完成；用户确认后输出完成标记。\n\
\n\
工作方式（重要）：\n\
- 小步快跑：每轮只做一件小事（一次搜索/一次读取/写一个文件的一小段）。\n\
- 避免单次超长输出；写 plan tree 时分多次小段写入，先建骨架再逐步填充。\n\
- 每完成一小步就简短总结一句再继续。\n\
\n\
（完整提示词在 P001 T005 细化——本文件由渲染器生成，勿手改）\n";
        let review_prompt = "\
# Plan Review Agent（规划检查 Agent）\n\
\n\
你是 devforge 系统的规划检查 Agent。\n\
\n\
职责边界（刻意窄）：读取 plan tree 与 plan-tree skill，只检查语义是否通顺、前后是否矛盾。\n\
不评设计好坏、不找潜在坑、不与用户交互。\n\
输出协议（强制，逐字遵守）：你的最终回复末尾【必须】包含协议块。\n\
通过：回复最末尾单独一行输出 <verdict>PASS</verdict>。\n\
不通过：回复最末尾输出 <verdict>FAIL</verdict>，随后 <issues> 开头逐条列问题。\n\
没有协议块的审查视为无效，必须重写回复补上协议块。\n\
\n\
工作方式（重要）：快速审查——只读根 README、各阶段 README 与每个任务文件各一遍即可，\n\
不要展开与审查无关的探索，尽快在回复末尾给出协议块。\n\
\n\
（完整提示词在 P001 T005 细化——本文件由渲染器生成，勿手改）\n";

        Self {
            base_packages: BASE_PACKAGES.iter().map(|s| s.to_string()).collect(),
            // provider 定义表（P2-ISSUE-03：渲染 models.json，pi 由此得知 baseUrl）
            providers: vec![ProviderDef {
                name: "newapi".into(),
                api: "openai-completions".into(),
                base_url: "https://newapi.trtyr.top/v1".into(),
            }],
            // 模型固定分配（用户拍板 2026-10-17，固定死不改；id 为 P1-CAL-01 校准后真实 id）：
            //   审查 Agent → Grok 4.7；编码/规划 Agent → DeepSeek flash
            model_pool: vec![
                ModelEntry {
                    id: "grok-4.7".into(),
                    provider: "newapi".into(),
                    model_id: "mokeywork/grok-4.7".into(),
                    context_window: 1_000_000,
                    max_tokens: 65_536,
                },
                ModelEntry {
                    id: "deepseek-flash".into(),
                    provider: "newapi".into(),
                    model_id: "mokeywork/deepseek-flash".into(),
                    context_window: 1_000_000,
                    max_tokens: 65_536,
                },
                ModelEntry {
                    id: "minimax-m3.1-flash".into(),
                    provider: "newapi".into(),
                    model_id: "MiniMax-M3.1-Flash".into(),
                    context_window: 1_000_000,
                    max_tokens: 65_536,
                },
            ],
            agents: vec![
                AgentManifest {
                    name: "plan-agent".into(),
                    role_prompt: plan_agent_prompt.into(),
                    extra_packages: vec![PLAN_AGENT_EXTRA.into()],
                    model: "deepseek-flash".into(),
                    skills: vec!["plan-tree".into()],
                    mcp: vec![],
                    thinking_level: None,
                },
                AgentManifest {
                    name: "plan-review".into(),
                    role_prompt: review_prompt.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(), // 审查 Agent 固定 Grok 4.7（用户拍板 2026-10-17）
                    skills: vec!["plan-tree".into()],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                },
                // P005 T003：编码双 Agent（提示词 T004/T005 换正稿）；模型沿规划期拍板：
                // 编码 → deepseek-flash，审查 → grok-4.7
                AgentManifest {
                    name: "coding-agent".into(),
                    role_prompt: CODING_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec![
                        "doc-contract".into(),
                        "coding-discipline".into(),
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                    thinking_level: None,
                },
                AgentManifest {
                    name: "coding-review".into(),
                    role_prompt: CODING_REVIEW_PROMPT.into(),
                    extra_packages: CODING_REVIEW_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "grok-4.7".into(),
                    skills: vec![
                        "doc-contract".into(),
                        "coding-discipline".into(),
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                    thinking_level: Some("medium".into()),
                },
                // P007 T001：测试双 Agent（D007 供给即纪律：外部知识给，内部捷径扣）
                AgentManifest {
                    name: "test-agent".into(),
                    role_prompt: TEST_AGENT_PROMPT.into(),
                    extra_packages: TEST_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec!["python-best-practices".into()],
                    mcp: vec![context7_mcp()],
                    thinking_level: None,
                },
                AgentManifest {
                    name: "test-review".into(),
                    role_prompt: TEST_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec![],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                },
                // P007 T001：文档双 Agent（doc-fix=coding 家族供给，需对着代码 triage；doc-review 极简）
                AgentManifest {
                    name: "doc-fix".into(),
                    role_prompt: DOC_FIX_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec![
                        "doc-contract".into(),
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                    thinking_level: None,
                },
                AgentManifest {
                    name: "doc-agent".into(),
                    role_prompt: DOC_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "minimax-m3.1-flash".into(),
                    skills: vec![
                        "doc-contract".into(),
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                    thinking_level: None,
                },
                AgentManifest {
                    name: "doc-review".into(),
                    role_prompt: DOC_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec!["doc-contract".into()],
                    // DR-12（10-23 用户拍板，推翻 D012/D018）：审查官不读码——事实核查由引擎
                    // 确定性面（coverage/lint/spec 锚）兑底，职责收敛到文字质量；实测有图时
                    // 审查官人肉重推导（61 次调用/14.5min）且读码探索失控触发协议违例双轮。
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                },
                // P009：变更 SOP 规划双 Agent。与 MVP 的 plan 双 Agent 平行：
                // 工具多一层 codegraph（把需求落到文件与符号上），技能仍是 plan-tree。
                AgentManifest {
                    name: "change-plan-agent".into(),
                    role_prompt: CHANGE_PLAN_AGENT_PROMPT.into(),
                    // 问询环的供给层：没有 ask 包就没有 extension_ui_request 工具，
                    // 计划里的「必须问用户」就永远问不出去（与 MVP plan-agent 同包）。
                    extra_packages: vec![PLAN_AGENT_EXTRA.into()],
                    model: "deepseek-flash".into(),
                    skills: vec!["plan-tree".into()],
                    // D009：只给 codegraph，不给 context7（本阶段不查外部库文档）
                    mcp: vec![codegraph_mcp()],
                    thinking_level: Some("high".into()),
                },
                AgentManifest {
                    name: "change-plan-review".into(),
                    role_prompt: CHANGE_PLAN_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec!["plan-tree".into()],
                    // D012：核调用关系/实体存在必须有图
                    mcp: vec![codegraph_mcp()],
                    thinking_level: Some("medium".into()),
                },
                // P009：变更编码双 Agent（D014/D016）——与 MVP coding 同款供给，提示词换 change 纪律。
                AgentManifest {
                    name: "change-coding-agent".into(),
                    role_prompt: CHANGE_CODING_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec![
                        "coding-discipline".into(),
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    // 改真代码常调第三方 API：codegraph + context7 两个都要（D016）
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                    thinking_level: Some("high".into()),
                },
                AgentManifest {
                    name: "change-coding-review".into(),
                    role_prompt: CHANGE_CODING_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec!["coding-discipline".into()],
                    // D016：审查靠对账与读 diff，不给 context7
                    mcp: vec![codegraph_mcp()],
                    thinking_level: Some("medium".into()),
                },
                // P009：变更测试双 Agent（黑盒：不给 codegraph，D019）
                AgentManifest {
                    name: "change-test-agent".into(),
                    role_prompt: CHANGE_TEST_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec!["python-best-practices".into()],
                    mcp: vec![],
                    thinking_level: Some("high".into()),
                },
                AgentManifest {
                    name: "change-test-review".into(),
                    role_prompt: CHANGE_TEST_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec![],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                },
            ],
            api_key: None,
        }
    }

    /// 从模型池解析 Agent 引用的模型。
    pub fn resolve_model(&self, agent: &AgentManifest) -> Result<&ModelEntry> {
        self.model_pool
            .iter()
            .find(|m| m.id == agent.model)
            .ok_or_else(|| {
                DevforgeError::Config(format!(
                    "Agent `{}` 引用的模型 `{}` 不在模型池中",
                    agent.name, agent.model
                ))
            })
    }

    /// 按 Agent 引用的模型找到其 provider 定义（渲染 models.json 用）。
    pub fn resolve_provider(&self, agent: &AgentManifest) -> Result<&ProviderDef> {
        let model = self.resolve_model(agent)?;
        self.providers.iter().find(|p| p.name == model.provider).ok_or_else(|| {
            DevforgeError::Config(format!(
                "模型 `{}` 引用的 provider `{}` 无定义",
                model.id, model.provider
            ))
        })
    }

    pub fn get_agent(&self, name: &str) -> Result<&AgentManifest> {
        self.agents
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| DevforgeError::Config(format!("清单中无 Agent `{name}`")))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).ctx("序列化配置清单")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx("创建清单目录")?;
        }
        std::fs::write(path, json).ctx(format!("写入清单 {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path).ctx(format!("读取清单 {}", path.display()))?;
        serde_json::from_str(&json).ctx("解析配置清单")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_plan_prompts_carry_six_blocks_and_forbidden_items() {
        // P009 T006：六块与「不写实施顺序 / 不写回归命令 / codegraph-first」必须在提示词里
        for block in crate::change::plan_audit::PLAN_BLOCKS {
            assert!(
                CHANGE_PLAN_AGENT_PROMPT.contains(block),
                "change-plan 提示词缺块：{block}"
            );
        }
        for anchor in ["codegraph-first", "不写实施顺序", "不写回归命令", "path:line"] {
            assert!(
                CHANGE_PLAN_AGENT_PROMPT.contains(anchor),
                "change-plan 提示词缺纪律：{anchor}"
            );
        }
        // 审查侧：只审计划、不看 diff、不跑命令、不写文件
        for anchor in ["不看 diff", "不跑命令", "不写文件", "<stage-verdict>"] {
            assert!(
                CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
                "change-plan-review 提示词缺纪律：{anchor}"
            );
        }
        // 问询环的四个锚点，且标记字面量与机械门常量**逐字相同**（改一边不改另一边 = 门空转）
        for anchor in [
            crate::change::plan_audit::VERDICT_MUST_ASK,
            crate::change::plan_audit::VERDICT_SELF,
            crate::change::plan_audit::VERDICT_ANSWER_FIELD,
            "ask-user-question",
        ] {
            assert!(
                CHANGE_PLAN_AGENT_PROMPT.contains(anchor),
                "change-plan 提示词缺问询契约锚点：{anchor}"
            );
        }
        // 审查侧两条新审点 + 同一批标记字面量
        for anchor in ["留痕核对", "分类恰当性", "结构缺失"] {
            assert!(
                CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
                "change-plan-review 提示词缺新审点：{anchor}"
            );
        }
        for anchor in [
            crate::change::plan_audit::VERDICT_MUST_ASK,
            crate::change::plan_audit::VERDICT_ANSWER_FIELD,
        ] {
            assert!(
                CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
                "change-plan-review 提示词缺标记字面量：{anchor}"
            );
        }
    }

    /// 需求 2 的落点：change 六件套逐个钉住供给（谁有 ask 包、谁没有）。
    ///
    /// 错实现形态：给某个 agent 顺手加包/删包 → 本用例必须变红。
    #[test]
    fn change_six_agents_have_pinned_supplies() {
        let m = ConfigManifest::builtin();
        let ask = "npm:@juicesharp/rpiv-ask-user-question";
        // 干活方要能问用户（ask 包）；审查方不与用户交互（无包）。
        let expect: [(&str, &[&str]); 6] = [
            ("change-plan-agent", &[ask]),
            ("change-plan-review", &[]),
            ("change-coding-agent", CODING_AGENT_EXTRA),
            ("change-coding-review", &[]),
            ("change-test-agent", CODING_AGENT_EXTRA),
            ("change-test-review", &[]),
        ];
        for (name, packages) in expect {
            let a = m.get_agent(name).unwrap();
            assert_eq!(
                a.extra_packages,
                packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "{name} 的差异化包与钉死的供给不符：{:?}",
                a.extra_packages
            );
        }
    }

    /// 其余九个 Agent 的差异化包逐字段钉死：改一处不许误伤别处。
    ///
    /// 错实现形态：顺手给别的 agent 加包/删包 → 本用例必须变红。
    #[test]
    fn other_agents_supplies_are_unchanged() {
        let m = ConfigManifest::builtin();
        let expect: [(&str, &[&str]); 9] = [
            ("plan-agent", &[PLAN_AGENT_EXTRA]),
            ("plan-review", &[]),
            ("coding-agent", CODING_AGENT_EXTRA),
            ("coding-review", CODING_REVIEW_EXTRA),
            ("test-agent", TEST_AGENT_EXTRA),
            ("test-review", &[]),
            ("doc-fix", CODING_AGENT_EXTRA),
            ("doc-agent", CODING_AGENT_EXTRA),
            ("doc-review", &[]),
        ];
        for (name, packages) in expect {
            let a = m.get_agent(name).unwrap();
            assert_eq!(
                a.extra_packages,
                packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "{name} 的差异化包被误改了：{:?}",
                a.extra_packages
            );
        }
    }

    #[test]
    fn builtin_has_eleven_agents_and_four_base_packages() {
        let m = ConfigManifest::builtin();
        assert_eq!(m.base_packages, BASE_PACKAGES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            m.agents.len(),
            15,
            "规划双 + 编码双 + 测试双 + 文档三 + 变更规划双 + 变更编码双 + 变更测试双（P007/P008/P009）"
        );
        // 差异化包仅 Plan Agent
        let pa = m.get_agent("plan-agent").unwrap();
        assert_eq!(pa.extra_packages, vec![PLAN_AGENT_EXTRA.to_string()]);
        let pr = m.get_agent("plan-review").unwrap();
        assert!(pr.extra_packages.is_empty());
        // P005：编码双 Agent 的 MCP 与差异化包
        let ca = m.get_agent("coding-agent").unwrap();
        assert_eq!(ca.mcp.len(), 2, "codegraph + context7");
        assert_eq!(ca.mcp[0].name, "codegraph");
        assert_eq!(ca.mcp[0].env.get("CODEGRAPH_TELEMETRY").map(String::as_str), Some("0"));
        assert_eq!(ca.extra_packages.len(), 3);
        assert_eq!(ca.model, "deepseek-flash"); // 编码 → DeepSeek（沿规划期拍板）
        let cr = m.get_agent("coding-review").unwrap();
        assert_eq!(cr.mcp.len(), 2);
        assert_eq!(cr.extra_packages, vec!["npm:pi-lens".to_string()]);
        assert_eq!(cr.model, "grok-4.7"); // 审查 → Grok（沿规划期拍板）
        // 规划双 Agent 无 MCP（P001 行为不回归）
        assert!(pa.mcp.is_empty() && pr.mcp.is_empty());
        // P007：测试双 Agent（D007 供给即纪律）
        let ta = m.get_agent("test-agent").unwrap();
        assert_eq!(ta.extra_packages, TEST_AGENT_EXTRA.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(ta.mcp.len(), 1, "仅 context7——codegraph 是代码事实层，黑盒不给");
        assert_eq!(ta.mcp[0].name, "context7");
        assert_eq!(ta.skills, vec!["python-best-practices".to_string()]);
        assert_eq!(ta.model, "deepseek-flash");
        let tr = m.get_agent("test-review").unwrap();
        assert!(tr.extra_packages.is_empty() && tr.mcp.is_empty() && tr.skills.is_empty());
        assert_eq!(tr.model, "grok-4.7");
        // P007：文档双 Agent（doc-fix=coding 家族供给；doc-review 极简）
        let df = m.get_agent("doc-fix").unwrap();
        assert_eq!(df.extra_packages, ca.extra_packages, "doc-fix 沿用 coding-agent 家族供给");
        assert_eq!(df.mcp.len(), 2);
        assert_eq!(df.model, "deepseek-flash");
        let dr = m.get_agent("doc-review").unwrap();
        // P008 T000a：doc-review 也拿 doc-contract（它是审查用规则书，不是写作技能）
        assert!(dr.extra_packages.is_empty());
        assert_eq!(dr.skills, vec!["doc-contract".to_string()]);
        // DR-12（10-23 用户拍板，推翻 D012/D018）：审查官不读码——事实由引擎确定性面兑底；
        // 实测有图时审查官人肉重推导（61 次调用/14.5min）且读码探索失控触发协议违例双轮
        assert!(dr.mcp.is_empty(), "doc-review 不得持有读码权限: {:?}", dr.mcp);
        assert_eq!(dr.model, "grok-4.7");

        // P009：变更 SOP 规划双 Agent（D009/D012/D016）
        let cpa = m.get_agent("change-plan-agent").unwrap();
        assert_eq!(cpa.model, "deepseek-flash", "干活方与 plan/coding 同档");
        assert_eq!(cpa.skills, vec!["plan-tree".to_string()]);
        assert_eq!(cpa.mcp.len(), 1, "只给 codegraph（D009）");
        assert_eq!(cpa.mcp[0].name, "codegraph");
        assert_eq!(
            cpa.extra_packages,
            vec![PLAN_AGENT_EXTRA.to_string()],
            "问询环的供给层：change-plan-agent 必须拿 ask 包（与 MVP plan-agent 同包）"
        );
        let cpr = m.get_agent("change-plan-review").unwrap();
        assert_eq!(cpr.model, "grok-4.7", "审查固定 grok");
        assert_eq!(cpr.mcp.len(), 1, "审计划要核调用关系（D012）");
        assert_eq!(cpr.mcp[0].name, "codegraph");
        assert!(cpr.extra_packages.is_empty());
        // MVP 的两个规划 Agent 不受影响（“平行不干扰”）
        assert_eq!(pa.model, "deepseek-flash");
        assert!(pa.mcp.is_empty() && pr.mcp.is_empty(), "MVP 规划双 Agent 仍无 MCP");

        // P009：变更编码双 Agent（D014/D016）
        let cca = m.get_agent("change-coding-agent").unwrap();
        assert_eq!(cca.model, "deepseek-flash");
        assert_eq!(cca.mcp.len(), 2, "codegraph + context7");
        assert_eq!(cca.mcp[1].name, "context7", "改真代码要查第三方 API");
        assert_eq!(cca.extra_packages, ca.extra_packages, "与 MVP coding 同档供给");
        assert_eq!(cca.skills.len(), 5, "四语言 best-practices + coding-discipline");
        let ccr = m.get_agent("change-coding-review").unwrap();
        assert_eq!(ccr.model, "grok-4.7");
        assert_eq!(ccr.mcp.len(), 1, "审查只给 codegraph（D016）");
        assert_eq!(ccr.mcp[0].name, "codegraph");
        assert!(ccr.extra_packages.is_empty());

        // P009：变更测试双 Agent（黑盒，D019：不给 codegraph）
        let cta = m.get_agent("change-test-agent").unwrap();
        assert_eq!(cta.model, "deepseek-flash");
        assert!(cta.mcp.is_empty(), "测试 agent 不得拿到代码事实层");
        let ctr = m.get_agent("change-test-review").unwrap();
        assert_eq!(ctr.model, "grok-4.7");
        assert!(ctr.mcp.is_empty() && ctr.skills.is_empty());
        // 测试提示词必须包含「回归优先 / 验收对账 / 不改产品代码」三条纪律
        for anchor in ["回归优先", "验收对照表", "不改产品代码", "对照表里只准有验收项"] {
            assert!(CHANGE_TEST_AGENT_PROMPT.contains(anchor), "缺纪律：{anchor}");
        }
        assert!(CHANGE_TEST_REVIEW_PROMPT.contains("缺一条即 FAIL"));

        // P008 D018：doc-agent = 干活方（与 coding-agent 同档供给）
        let da = m.get_agent("doc-agent").unwrap();
        assert_eq!(
            da.model, "minimax-m3.1-flash",
            "doc-agent 自 2026-10-20 起用 MiniMax-M3.1-Flash"
        );
        assert_eq!(da.extra_packages, ca.extra_packages);
        assert_eq!(da.mcp.len(), 2, "codegraph + context7");
        assert_eq!(da.mcp[0].name, "codegraph");
        assert_eq!(da.mcp[1].name, "context7");
        assert!(da.skills.iter().any(|s| s == "doc-contract"));
        assert_eq!(da.thinking_level, None, "干活方用全局 high");
    }

    #[test]
    fn doc_contract_skill_is_shared_by_four_profiles() {
        // P008 T000a：一份契约四处共用，不允许某处漏掉（否则就是四份措辞漂移的起点）
        let m = ConfigManifest::builtin();
        for name in ["coding-agent", "coding-review", "doc-fix", "doc-review"] {
            let a = m.get_agent(name).unwrap();
            assert!(
                a.skills.iter().any(|s| s == "doc-contract"),
                "{name} 缺少 doc-contract skill"
            );
        }
    }

    #[test]
    fn coding_discipline_skill_goes_to_exactly_the_four_coding_agents() {
        // 纪律唯一事实源：只发给写代码与审代码的四个 Agent（4 正 + 11 负 = 15）
        let m = ConfigManifest::builtin();
        for name in [
            "coding-agent",
            "coding-review",
            "change-coding-agent",
            "change-coding-review",
        ] {
            let a = m.get_agent(name).unwrap();
            assert!(
                a.skills.iter().any(|s| s == "coding-discipline"),
                "{name} 缺少 coding-discipline skill"
            );
        }
        for name in [
            "plan-agent",
            "plan-review",
            "change-plan-agent",
            "change-plan-review",
            "test-agent",
            "test-review",
            "change-test-agent",
            "change-test-review",
            "doc-fix",
            "doc-agent",
            "doc-review",
        ] {
            let a = m.get_agent(name).unwrap();
            assert!(
                !a.skills.iter().any(|s| s == "coding-discipline"),
                "{name} 不该拿 coding-discipline skill"
            );
        }
        assert_eq!(m.agents.len(), 15, "4 正 + 11 负 = 15");
    }

    #[test]
    fn coding_prompts_carry_discipline_anchors() {
        // 写代码方：纪律 skill + 行为断言 + 先证后报
        for anchor in ["coding-discipline", "行为断言", "先证后报"] {
            assert!(
                CODING_AGENT_PROMPT.contains(anchor),
                "MVP coding 提示词缺：{anchor}"
            );
            assert!(
                CHANGE_CODING_AGENT_PROMPT.contains(anchor),
                "change coding 提示词缺：{anchor}"
            );
        }
        // 行为断言要两头成立（补齐「只做内部重构仍绿」这半边）
        for prompt in [CODING_AGENT_PROMPT, CHANGE_CODING_AGENT_PROMPT] {
            assert!(
                prompt.contains("只做内部重构"),
                "行为断言判据缺「只做内部重构仍绿」这半边"
            );
        }
        // 审代码方：自证核对审查面（先证后报）
        assert!(
            CODING_REVIEW_PROMPT.contains("先证后报"),
            "MVP review 提示词缺自证核对面"
        );
        assert!(
            CHANGE_CODING_REVIEW_PROMPT.contains("先证后报"),
            "change review 提示词缺自证核对面"
        );
    }

    /// 代码生成器：把 6 个 change agents 导出成 e2e 夹具（漂移时重跑刷新）。
    /// `cargo test --lib dump_change_agents_fixture -- --ignored`
    #[test]
    #[ignore = "代码生成器：重建 tests/e2e/fixtures/change_agents.json"]
    fn dump_change_agents_fixture() {
        let m = ConfigManifest::builtin();
        let agents: Vec<&AgentManifest> = m
            .agents
            .iter()
            .filter(|a| a.name.starts_with("change-"))
            .collect();
        assert_eq!(agents.len(), 6, "change agents 应有 6 个");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/e2e/fixtures");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("change_agents.json"),
            serde_json::to_string_pretty(&agents).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn change_agents_fixture_matches_builtin() {
        // e2e 用这份夹具把 change 六件套装进 manifest；漂移 = e2e 测的不是内置提示词
        let m = ConfigManifest::builtin();
        let expected: Vec<&AgentManifest> = m
            .agents
            .iter()
            .filter(|a| a.name.starts_with("change-"))
            .collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/e2e/fixtures/change_agents.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("夹具缺失，重跑 dump_change_agents_fixture：{}", path.display()));
        let actual: Vec<AgentManifest> = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            serde_json::to_string(&actual).unwrap(),
            serde_json::to_string(&expected).unwrap(),
            "change agents 夹具与 builtin 漂移——重跑 dump_change_agents_fixture"
        );
    }

    #[test]
    fn doc_agent_prompt_carries_protocol_and_guards() {
        // D009/D010/D016/D020 的硬约束锚点
        assert!(DOC_AGENT_PROMPT.contains("只写 `docs/contexts/`"));
        assert!(DOC_AGENT_PROMPT.contains("codegraph-first"));
        assert!(DOC_AGENT_PROMPT.contains("事实与推断分离"));
        assert!(DOC_AGENT_PROMPT.contains("🟢"));
        assert!(DOC_AGENT_PROMPT.contains("path:line"));
        assert!(DOC_AGENT_PROMPT.contains("gaps.md"));
        assert!(DOC_AGENT_PROMPT.contains("N/A"));
        assert!(DOC_AGENT_PROMPT.contains("<doc-complete>unit-xxx</doc-complete>"));
        assert!(DOC_AGENT_PROMPT.contains("devforge doc-lint"));
        assert!(DOC_AGENT_PROMPT.contains("按 route 回去修"));
        // t8：三阶段流水与证据要求
        assert!(DOC_AGENT_PROMPT.contains("阶段流水（事实 / 推断分离"));
        assert!(DOC_AGENT_PROMPT.contains("_meta/facts/<unit>.md"));
        // t12/t13：汇编层只合成、图用 Mermaid 源码（D005）
        assert!(DOC_AGENT_PROMPT.contains("汇编层只合成"));
        assert!(DOC_AGENT_PROMPT.contains("Mermaid 源码"));
        assert!(DOC_AGENT_PROMPT.contains("总览 / 架构 / 数据流 / Runbook"));
        assert!(DOC_AGENT_PROMPT.contains("严禁写“为什么”"));
        assert!(DOC_AGENT_PROMPT.contains("不得静默省略"));
    }

    #[test]
    fn coding_agent_prompt_has_preflight_audit() {
        // 2026-10-19 实测：10 轮审查 5 轮打回——文档契约/绝对词/断言强度三类白旗缺口
        assert!(CODING_AGENT_PROMPT.contains("提交前预审"));
        assert!(CODING_AGENT_PROMPT.contains("绝对词"));
        assert!(CODING_AGENT_PROMPT.contains("断言必须能失败"));
        assert!(CODING_AGENT_PROMPT.contains("预审证据"));
        assert!(CODING_AGENT_PROMPT.contains("isVisible()"));
        // T000c：契约 + doc-lint 硬门禁（文档不能只靠自述）
        assert!(CODING_AGENT_PROMPT.contains("doc-contract"));
        assert!(CODING_AGENT_PROMPT.contains("devforge doc-lint"));
        assert!(CODING_AGENT_PROMPT.contains("退出码必须 0"));
        assert!(CODING_REVIEW_PROMPT.contains("doc-lint 复核"));
        assert!(CODING_REVIEW_PROMPT.contains("自己跑一遍"));
    }

    #[test]
    fn testing_prompts_carry_protocol_and_discipline() {
        // 黑盒硬禁令锚点（D001）
        assert!(TEST_AGENT_PROMPT.contains("禁止读取任何实现文件内容"));
        assert!(TEST_AGENT_PROMPT.contains("路径可以看，内容不能读"));
        assert!(TEST_AGENT_PROMPT.contains("白名单"));
        assert!(TEST_AGENT_PROMPT.contains("<stage-complete>"));
        assert!(TEST_AGENT_PROMPT.contains("coverage-matrix"));
        assert!(TEST_AGENT_PROMPT.contains("pytest-rerunfailures"));
        assert!(TEST_AGENT_PROMPT.contains("tests/integration"));
        assert!(TEST_AGENT_PROMPT.contains("Runbook"));
        assert!(TEST_AGENT_PROMPT.contains("依赖获取"), "测试依赖获取口径缺失");
        assert!(TEST_AGENT_PROMPT.contains("findings"));
        assert!(TEST_AGENT_PROMPT.contains("严禁写单元测试"));
        // reroute 协议锚点（D003）
        assert!(TEST_REVIEW_PROMPT.contains("<reroute>"));
        assert!(TEST_REVIEW_PROMPT.contains("test-side"));
        assert!(TEST_REVIEW_PROMPT.contains("doc-side"));
        assert!(TEST_REVIEW_PROMPT.contains("<stage-verdict>"));
        assert!(TEST_REVIEW_PROMPT.contains("<next>"));
        assert!(TEST_REVIEW_PROMPT.contains("业务关系"));
        assert!(TEST_REVIEW_PROMPT.contains("极端用户面"));
        assert!(TEST_REVIEW_PROMPT.contains("coverage-matrix"));
        // 省时硬纪律（2026-10-19 实测：审查占编码墙钟 68%）：环境复用/只跑一遍/一次列全
        assert!(CODING_REVIEW_PROMPT.contains("环境复用"));
        assert!(CODING_REVIEW_PROMPT.contains("测试只跑一遍"));
        assert!(CODING_REVIEW_PROMPT.contains("问题一次列全"));
        assert!(CODING_REVIEW_PROMPT.contains("批量调用"));
        assert!(CODING_REVIEW_PROMPT.contains("报告密度"));
        assert!(CODING_REVIEW_PROMPT.contains("不复述代码"));
        assert!(TEST_REVIEW_PROMPT.contains("批量调用"));
        assert!(TEST_REVIEW_PROMPT.contains("环境复用"));
        assert!(TEST_REVIEW_PROMPT.contains("问题一次列全"));
        // 测试自身可靠性锚点（2026-10-19 自指递归事故）
        assert!(TEST_AGENT_PROMPT.contains("禁止在套件内运行整套测试"));
        assert!(TEST_AGENT_PROMPT.contains("进程组"));
        assert!(TEST_AGENT_PROMPT.contains("passed / failed / skipped"));
        assert!(TEST_AGENT_PROMPT.contains("短超时"));
        assert!(TEST_REVIEW_PROMPT.contains("一票否决"));
        assert!(TEST_REVIEW_PROMPT.contains("发现挂死风险先停跑"));
        // 文档双 Agent 锚点（D004）
        assert!(DOC_FIX_PROMPT.contains("只改文档，不改代码"));
        assert!(DOC_FIX_PROMPT.contains("第三方"));
        assert!(DOC_FIX_PROMPT.contains("Runbook"));
        assert!(DOC_FIX_PROMPT.contains("findings"));
        assert!(DOC_REVIEW_PROMPT.contains("plan tree"));
        assert!(DOC_REVIEW_PROMPT.contains("Runbook"));
        assert!(DOC_REVIEW_PROMPT.contains("findings"));
        assert!(DOC_REVIEW_PROMPT.contains("<stage-verdict>"));
    }

    #[test]
    fn model_resolution_from_pool() {
        let m = ConfigManifest::builtin();
        let pa = m.get_agent("plan-agent").unwrap();
        let model = m.resolve_model(pa).unwrap();
        assert_eq!(model.provider, "newapi");
        assert_eq!(model.model_id, "mokeywork/deepseek-flash");
        // 固定分配：审查 Agent → Grok 4.7（用户拍板 2026-10-17）
        let rv = m.get_agent("plan-review").unwrap();
        assert_eq!(m.resolve_model(rv).unwrap().model_id, "mokeywork/grok-4.7");
    }

    #[test]
    fn unknown_model_rejected() {
        let mut m = ConfigManifest::builtin();
        m.agents[0].model = "ghost-model".into();
        let err = m.resolve_model(&m.agents[0]).unwrap_err();
        assert!(err.to_string().contains("不在模型池中"));
    }

    #[test]
    fn manifest_json_roundtrip() {
        let m = ConfigManifest::builtin();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        m.save(&path).unwrap();
        let loaded = ConfigManifest::load(&path).unwrap();
        assert_eq!(m, loaded);
    }
}
