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

/// Code Review Agent 提示词（P013 全量代码审计：批次审查官，六视角综合 + 结构化发现协议）。
pub const CODE_REVIEW_AGENT_PROMPT: &str = include_str!("../../assets/prompts/code-review-agent.md");

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
    /// 工具级路径权限（P012 topic 07）：渲染 `.pi/pathguard.json`，pi 侧 path-guard 插件
    /// 拦截 read/write/edit。None = 不生成（老清单兼容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_permissions: Option<PathPermissions>,
}

/// 工具级路径权限声明（deny-first：deny 永远赢；read 拒绝连带堵 write/edit）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PathPermissions {
    /// 允许读的 glob（空 = 不限，仅受 deny_read 约束）。
    #[serde(default)]
    pub read: Vec<String>,
    /// 允许写的 glob（空 = 不限，仅受 deny_write 约束）。
    #[serde(default)]
    pub write: Vec<String>,
    /// 拒绝读（优先级最高；deny_read 命中连 write/edit 一起堵）。
    #[serde(default)]
    pub deny_read: Vec<String>,
    /// 拒绝写（优先级最高；profile 目录与 guard 配置自身默认追加在 deny_write，防提权）。
    #[serde(default)]
    pub deny_write: Vec<String>,
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
                ModelEntry {
                    id: "person-deepseek-flash".into(),
                    provider: "newapi".into(),
                    model_id: "person/deepseek-flash".into(), // person 渠道（mokeywork 渠道 10-25 实测 403）
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
                    path_permissions: None,
                },
                AgentManifest {
                    name: "plan-review".into(),
                    role_prompt: review_prompt.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(), // 审查 Agent 固定 Grok 4.7（用户拍板 2026-10-17）
                    skills: vec!["plan-tree".into()],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
                },
                AgentManifest {
                    name: "test-review".into(),
                    role_prompt: TEST_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec![],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
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
                    path_permissions: None,
                },
                AgentManifest {
                    name: "change-test-review".into(),
                    role_prompt: CHANGE_TEST_REVIEW_PROMPT.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(),
                    skills: vec![],
                    mcp: vec![],
                    thinking_level: Some("medium".into()),
                    path_permissions: None,
                },
                // P013：全量代码审计单 Agent（批次综合六视角；量大走便宜模型；
                // codegraph 供 V5 一致性全调用点比对；CODING_AGENT_EXTRA 供只读检查工具）
                AgentManifest {
                    name: "code-review".into(),
                    role_prompt: CODE_REVIEW_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "person-deepseek-flash".into(), // person 渠道 deepseek-flash（mokeywork 渠道 403，用户 10-25 拍板）
                    skills: vec!["rust-best-practices".into(), "python-best-practices".into(), "ts-js-best-practices".into(), "go-best-practices".into()],
                    mcp: vec![codegraph_mcp()],
                    thinking_level: Some("medium".into()),
                    path_permissions: None,
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
mod manifest_tests;
