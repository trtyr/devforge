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
    /// 模型池（模型管理面）。
    pub model_pool: Vec<ModelEntry>,
    /// Agent 清单（Agent 管理面）。
    pub agents: Vec<AgentManifest>,
    /// API Key——明文不设防（P002 decisions/001）。
    pub api_key: Option<String>,
}

impl ConfigManifest {
    /// 内置默认清单：规划阶段的两个 Agent + DeepSeek flash 模型池（用户拍板 2026-10-16，
    /// 「又强又便宜」；确切 provider/model id 落地联调时对 newapi 目录核对——P001 T005）。
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
（完整提示词在 P001 T005 细化——本文件由渲染器生成，勿手改）\n";
        let review_prompt = "\
# Plan Review Agent（规划检查 Agent）\n\
\n\
你是 devforge 系统的规划检查 Agent。\n\
\n\
职责边界（刻意窄）：读取 plan tree 与 plan-tree skill，只检查语义是否通顺、前后是否矛盾。\n\
不评设计好坏、不找潜在坑、不与用户交互。输出统一 verdict 块（PASS/FAIL + 问题清单）。\n\
\n\
（完整提示词在 P001 T005 细化——本文件由渲染器生成，勿手改）\n";

        Self {
            base_packages: BASE_PACKAGES.iter().map(|s| s.to_string()).collect(),
            model_pool: vec![ModelEntry {
                id: "deepseek-flash".into(),
                provider: "newapi".into(),
                model_id: "DeepSeek flash".into(), // 用户原话照录；T005 核对确切 id
            }],
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
                    model: "deepseek-flash".into(),
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
        assert_eq!(model.model_id, "DeepSeek flash");
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
