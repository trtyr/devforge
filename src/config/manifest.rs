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
    /// 内置默认清单：规划阶段的两个 Agent + 固定模型池（2026-10-17 用户拍板固定死：
    /// 审查 Agent → Grok 4.7，编码/规划 Agent → DeepSeek flash；真实 id 经 P1-CAL-01 校准）。
    pub fn planning_builtin() -> Self {
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
                },
                AgentManifest {
                    name: "plan-review".into(),
                    role_prompt: review_prompt.into(),
                    extra_packages: vec![],
                    model: "grok-4.7".into(), // 审查 Agent 固定 Grok 4.7（用户拍板 2026-10-17）
                    skills: vec!["plan-tree".into()],
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
        let m = ConfigManifest::planning_builtin();
        assert_eq!(m.base_packages, BASE_PACKAGES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(m.agents.len(), 2);
        // 差异化包仅 Plan Agent
        let pa = m.get_agent("plan-agent").unwrap();
        assert_eq!(pa.extra_packages, vec![PLAN_AGENT_EXTRA.to_string()]);
        let pr = m.get_agent("plan-review").unwrap();
        assert!(pr.extra_packages.is_empty());
    }

    #[test]
    fn model_resolution_from_pool() {
        let m = ConfigManifest::planning_builtin();
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
        let mut m = ConfigManifest::planning_builtin();
        m.agents[0].model = "ghost-model".into();
        let err = m.resolve_model(&m.agents[0]).unwrap_err();
        assert!(err.to_string().contains("不在模型池中"));
    }

    #[test]
    fn manifest_json_roundtrip() {
        let m = ConfigManifest::planning_builtin();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        m.save(&path).unwrap();
        let loaded = ConfigManifest::load(&path).unwrap();
        assert_eq!(m, loaded);
    }
}
