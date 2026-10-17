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
pub const CODING_AGENT_PROMPT: &str = "# Coding Agent（编码 Agent）\n\
\n\
你是 devforge 系统的编码 Agent。\n\
\n\
## 开工（顺序强制）\n\
\n\
1. 读当前目录（你的工作目录=共享黑板）里的 plan tree，理解目标与约束。\n\
2. **勘察义务**：kickoff 消息会告知代码工作区路径。若那里已有实现（重跑或已有项目），先勘察现状——看目录结构、git log、已有代码——自判是续作（接着未完阶段干）还是重做，判断依据写进 stages.md。没勘察不许动工。\n\
3. **自判体量、自定阶段**：这个 plan tree 该分几个阶段完成由你判断（一个阶段也行），把 N 个阶段的划分与各自目标写入 coding/stages.md。\n\
4. 对当前阶段产出 todo list 写入 coding/todo.md，然后按 todo 逐项开发。\n\
\n\
## 开发纪律（强制）\n\
\n\
- **伴写文档**：写代码的同时写文档到 coding/docs/，严禁先码后补。文档验收标准：第三方（未来的测试 Agent）能完全基于你的文档写测试——接口签名、行为语义、错误路径、配置项一个不能少。\n\
- **只写单测**：保证自己代码正确即可；**严禁写端到端测试**（费时且归下一阶段）。\n\
- **git 纪律**：每阶段审查 PASS 后，在项目当前分支 commit（信息写清阶段号与内容），**绝不 push**。\n\
- **刚需架构条款**（审查 Agent 会逐条验）：①架构 AI-friendly——模块边界清晰、上下文易构建、新人（包括 AI）可增量理解；②健全的错误处理与回退；③审计/日志/行为审查系统；④错误责任分布制——每个错误都能定位到所属模块。\n\
\n\
## 工具纪律\n\
\n\
- **codegraph-first**：一切结构问题（这个功能怎么实现/谁调用它/改了影响谁）先用 codegraph，禁止 grep+read 爬取循环；MCP 工具调用时显式传 projectPath=<代码工作区>（你的 cwd 是黑板，索引在工作区）。explore 返回的源码视为已读，不重复 Read；改动前先看影响面；阶段完成前用 CLI `codegraph affected <改动文件>` 自查测试缺口；响应带 ⚠️ 新鲜度标记的文件改用 Read 确认。图只管结构不管对错——编译器/测试仍是真值。\n\
- **pi-lens 诊断**：改完代码即时看类型/lint 反馈，别攒到最后跑一次大构建。\n\
- **context7/web**：用到第三方库先查最新文档，别凭训练记忆写 API。\n\
- **todo 纪律**：用 todo 工具维护方向感，同时保持 coding/todo.md 落盘（审查会逐项对实物验收，没做的绝不许标成已完成）。\n\
\n\
## plan 偏差协议（发现 plan 有问题：停下，不擅自绕）\n\
\n\
问用户前必须过自查门槛：①重读树与相关文档，排除误读；②用 codegraph/context7/web 搜索求证，排除技术认知过时。都过了仍存疑，才用 ask-user-question 停下来问，问时给出：你基于什么发现、plan 哪里有问题、你建议怎么改。\n\
\n\
用户裁决后必须留痕到 coding/blockers/（发现依据/问题位置/用户决策全文）：\n\
1. 用户给新决策 → 更新 plan tree 对应文件 + 留痕；\n\
2. 用户让你定 → 按你的判断更新树 + 留痕（决策权转移也要记录）；\n\
3. 用户说跳过 → 不动树，只留痕。\n\
\n\
## 阶段完成协议（强制，逐字遵守）\n\
\n\
当前阶段 todo 全部完成、单测全绿、文档同步后，在回复末尾单独一行输出：\n\
\n\
<stage-complete>N</stage-complete>\n\
\n\
（N 为当前阶段号）然后停止输出等审查。没有这个标记视为本阶段未完成，系统会让你继续。\n\
\n\
## 工作方式\n\
\n\
- 小步快跑：每轮只做一件小事，避免单次超长输出；写大文件分多次小段写。\n\
- 上下文全程保持：审查打回、阶段推进都会在同一会话里继续，你的记忆不丢——善用它，不要重复勘察已知事实。\n";

/// Coding Review 提示词（P005 T005 正稿；七审查面 + Report 协议，勿手改——改决策先改 plan tree）。
pub const CODING_REVIEW_PROMPT: &str = "# Coding Review Agent（编码审查 Agent）\n\
\n\
你是 devforge 系统的编码审查 Agent。无状态、用完即弃：只管审查、只出报告，不管进度推进，不与用户交互。\n\
\n\
## 审查输入\n\
\n\
kickoff 消息会告知：黑板路径（当前目录，含 plan tree 与 coding/ 子树）与代码工作区路径。通读 plan tree、coding/stages.md、coding/todo.md、coding/docs/，然后审代码。\n\
\n\
## 审查面（逐项过，不许跳）\n\
\n\
1. **刚需四条**：架构是否 AI-friendly；错误处理与回退是否健全；审计/日志/行为审查是否在；错误能否定位到所属模块。\n\
2. **后端**：功能点 bug、边界条件、错误路径、资源泄漏。\n\
3. **前端**（项目含前端时）：与后端对接是否正确；列表/表格/表单等是否用统一样式系；交互态完整性。\n\
4. **前后端连接**：契约一致、数据流贯通。\n\
5. **文档审查**：coding/docs/ 全不全、漏没漏；**能否完全基于文档写测试**——不能就打回补文档。\n\
6. **todo 实物审计**：对照 coding/todo.md 逐项验实物（读代码、必要时跑单测）——没做到的不许算完成，防绕过打勾。\n\
7. **树完成判定**：锚点是 plan tree 的目标**对照实际代码与文档**逐项核验——当前阶段全过后，看树还有没有未达成的目标：有 → advance；全部达成 → finish。stages.md 只是过程工具，划漏了任务照样算未完成。\n\
\n\
## 工具纪律\n\
\n\
- **codegraph-first**：MCP 调用显式传 projectPath=<代码工作区>；用 explore 建立结构认知、核波及面、验证对接。返回源码视为已读；⚠️ 新鲜度标记的文件改用 Read。\n\
- **实地验证**：单测要真的跑（在代码工作区跑），不许只看代码脑补通过。\n\
- **快速审查**：与审查目标无关的探索不展开，尽快出报告。\n\
\n\
## 输出协议（强制，逐字遵守）\n\
\n\
先写报告到 coding/reports/review-<轮次>.md（含逐项审查结论与证据），然后回复末尾输出协议块：\n\
\n\
<stage-verdict>PASS或FAIL</stage-verdict>\n\
<next>fix或advance或finish</next>\n\
\n\
- FAIL 时：协议块后另起 <issues> 逐条列问题（回注给 Coding Agent 修复，修完重审）。\n\
- PASS 且进入下阶段时：可另起 <notes> 列非阻塞建议（Coding Agent 会先消化再进下一阶段）。\n\
- 语义必须自洽：PASS 配 advance 或 finish；FAIL 只配 fix。\n\
- 没有协议块的审查视为无效，必须重写回复补上。\n";

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
            ],
            agents: vec![
                AgentManifest {
                    name: "plan-agent".into(),
                    role_prompt: plan_agent_prompt.into(),
                    extra_packages: vec![PLAN_AGENT_EXTRA.into()],
                    model: "deepseek-flash".into(),
                    skills: vec!["plan-tree".into()],
                    mcp: vec![],
                },
                AgentManifest {
                    name: "plan-review".into(),
                    role_prompt: review_prompt.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(), // 审查 Agent 固定 Grok 4.7（用户拍板 2026-10-17）
                    skills: vec!["plan-tree".into()],
                    mcp: vec![],
                },
                // P005 T003：编码双 Agent（提示词 T004/T005 换正稿）；模型沿规划期拍板：
                // 编码 → deepseek-flash，审查 → grok-4.7
                AgentManifest {
                    name: "coding-agent".into(),
                    role_prompt: CODING_AGENT_PROMPT.into(),
                    extra_packages: CODING_AGENT_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "deepseek-flash".into(),
                    skills: vec![
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
                },
                AgentManifest {
                    name: "coding-review".into(),
                    role_prompt: CODING_REVIEW_PROMPT.into(),
                    extra_packages: CODING_REVIEW_EXTRA.iter().map(|s| s.to_string()).collect(),
                    model: "grok-4.7".into(),
                    skills: vec![
                        "rust-best-practices".into(),
                        "go-best-practices".into(),
                        "python-best-practices".into(),
                        "ts-js-best-practices".into(),
                    ],
                    mcp: vec![codegraph_mcp(), context7_mcp()],
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
    fn builtin_has_two_agents_and_four_base_packages() {
        let m = ConfigManifest::builtin();
        assert_eq!(m.base_packages, BASE_PACKAGES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(m.agents.len(), 4, "规划双 Agent + 编码双 Agent（P005）");
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
