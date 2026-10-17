//! Profile 渲染器（P002）：清单（真源）→ profile 目录（渲染产物，可随时重建）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::installer::PackageInstaller;
use crate::config::manifest::ConfigManifest;
use crate::config::skill_assets::builtin_skill_files;
use crate::error::{Context as _, Result};

/// 运行时生存配置（P002 模板层写死；P2-ISSUE-04 修复：中转抖动的重试生存配置、
/// 流挂死快判、reasoning 预算等此前缺失，导致同一中转上真实环境存活而 profile 一次即死）。
fn runtime_settings() -> serde_json::Value {
    serde_json::json!({
        "retry": { "maxRetries": 3, "baseDelayMs": 5000 },
        "httpIdleTimeoutMs": 60000,
        "defaultThinkingLevel": "high",
        "hideThinkingBlock": true,
        "compaction": { "enabled": false, "reserveTokens": 16384, "keepRecentTokens": 20000 },
    })
}

pub struct Renderer<'a> {
    pub manifest: &'a ConfigManifest,
    pub installer: &'a dyn PackageInstaller,
    /// skills 内容源目录（含 `<skill-name>/` 子目录）。None = 只建占位目录。
    pub skills_source: Option<&'a Path>,
}

impl<'a> Renderer<'a> {
    /// 渲染单个 Agent 的 profile 到 `profiles_root/<agent-name>/`。
    pub fn render_agent(&self, profiles_root: &Path, agent_name: &str) -> Result<PathBuf> {
        let agent = self.manifest.get_agent(agent_name)?;
        let model = self.manifest.resolve_model(agent)?;

        let dir = profiles_root.join(&agent.name);
        fs::create_dir_all(dir.join("skills")).ctx("创建 profile 骨架目录")?;

        // AGENTS.md（模板层：角色提示词）
        fs::write(dir.join("AGENTS.md"), &agent.role_prompt)
            .ctx(format!("写 {} 的 AGENTS.md", agent.name))?;

        // settings.json（模型层解析 + 运行时生存配置；packages 由 installer 双记录追加）
        let mut settings = runtime_settings();
        settings["defaultProvider"] = serde_json::Value::String(model.provider.clone());
        settings["defaultModel"] = serde_json::Value::String(model.model_id.clone());
        fs::write(
            dir.join("settings.json"),
            serde_json::to_string_pretty(&settings).ctx("序列化 settings")?,
        )
        .ctx(format!("写 {} 的 settings.json", agent.name))?;

        // models.json（P2-ISSUE-03 修复：provider 定义 + 模型窗口/输出预算）
        let provider = self.manifest.resolve_provider(agent)?;
        let models: Vec<serde_json::Value> = self
            .manifest
            .model_pool
            .iter()
            .filter(|m| m.provider == provider.name)
            .map(|m| {
                serde_json::json!({
                    "id": m.model_id,
                    "name": m.model_id,
                    "contextWindow": m.context_window,
                    "maxTokens": m.max_tokens,
                })
            })
            .collect();
        let models_json = serde_json::json!({
            "providers": {
                provider.name.clone(): {
                    "api": provider.api,
                    "baseUrl": provider.base_url,
                    "models": models,
                }
            }
        });
        fs::write(
            dir.join("models.json"),
            serde_json::to_string_pretty(&models_json).ctx("序列化 models")?,
        )
        .ctx(format!("写 {} 的 models.json", agent.name))?;

        // skills 分发：有源复制；无源用内置资产；两者皆无才建占位（P2-ISSUE-05 修复：
        // 占位目录曾导致 Agent 全盘扫描找规范，单次工具执行 10-30 分钟）
        for skill in &agent.skills {
            let target = dir.join("skills").join(skill);
            match self.skills_source {
                Some(src) => {
                    let from = src.join(skill);
                    copy_dir(&from, &target).ctx(format!(
                        "复制 skill {skill}: {} → {}",
                        from.display(),
                        target.display()
                    ))?;
                }
                None => match builtin_skill_files(skill) {
                    Some(files) => {
                        fs::create_dir_all(&target).ctx("建 skill 目录")?;
                        for (name, body) in files {
                            let f = target.join(name);
                            if let Some(parent) = f.parent() {
                                fs::create_dir_all(parent).ctx("建 skill 子目录")?;
                            }
                            fs::write(f, body)
                                .ctx(format!("写内置 skill {skill}/{name}"))?;
                        }
                    }
                    None => {
                        fs::create_dir_all(&target).ctx("建 skill 占位目录")?;
                        fs::write(
                            target.join("README.md"),
                            format!("# {skill}\n\n占位目录——无内置资产且未提供 skills 源。\n"),
                        )
                        .ctx("写 skill 占位 README")?;
                    }
                },
            }
        }

        // auth.json——明文 Key，provider 键控形状（P002 decisions/001 明文不设防；
        // P2-ISSUE-01 修复：pi 期望 {"<provider>": {"type": "api_key", "key": ...}}）
        if let Some(key) = &self.manifest.api_key {
            fs::write(
                dir.join("auth.json"),
                serde_json::json!({ (provider.name.clone()): { "type": "api_key", "key": key } })
                    .to_string(),
            )
            .ctx(format!("写 {} 的 auth.json", agent.name))?;
        }

        // mcp.json（P005 T003：pi 原生 schema，<agent-dir>/mcp.json 全项目可用；
        // 空 = 不生成该文件，规划双 Agent 不受影响）
        if !agent.mcp.is_empty() {
            let mut servers = serde_json::Map::new();
            for m in &agent.mcp {
                let mut entry = serde_json::Map::new();
                entry.insert("command".into(), serde_json::Value::String(m.command.clone()));
                entry.insert(
                    "args".into(),
                    serde_json::Value::Array(
                        m.args.iter().cloned().map(serde_json::Value::String).collect(),
                    ),
                );
                if !m.env.is_empty() {
                    entry.insert(
                        "env".into(),
                        serde_json::Value::Object(
                            m.env.iter()
                                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                                .collect(),
                        ),
                    );
                }
                servers.insert(m.name.clone(), serde_json::Value::Object(entry));
            }
            let mcp_json = serde_json::json!({ "mcpServers": servers });
            fs::write(
                dir.join("mcp.json"),
                serde_json::to_string_pretty(&mcp_json).ctx("序列化 mcp")?,
            )
            .ctx(format!("写 {} 的 mcp.json", agent.name))?;
        }

        // 包安装（必装四件套 + 差异化包）——双记录由 installer 负责
        let mut packages = self.manifest.base_packages.clone();
        packages.extend(agent.extra_packages.iter().cloned());
        self.installer.install(&dir, &packages)?;

        tracing::info!(
            component = "renderer",
            agent = %agent.name,
            packages = packages.len(),
            profile = %dir.display(),
            "profile 渲染完成"
        );
        Ok(dir)
    }

    /// 渲染清单中全部 Agent。
    pub fn render_all(&self, profiles_root: &Path) -> Result<Vec<PathBuf>> {
        self.manifest
            .agents
            .iter()
            .map(|a| self.render_agent(profiles_root, &a.name))
            .collect()
    }
}

/// 递归复制目录。
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).ctx(format!("建目录 {}", to.display()))?;
    for entry in fs::read_dir(from).ctx(format!("读目录 {}", from.display()))? {
        let entry = entry.ctx("读目录项")?;
        let ty = entry.file_type().ctx("读文件类型")?;
        let target = to.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).ctx(format!(
                "复制 {} → {}",
                entry.path().display(),
                target.display()
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::installer::FakeInstaller;
    use crate::config::manifest::ConfigManifest;
    use crate::error::DevforgeError;

    fn renderer(manifest: &ConfigManifest) -> Renderer<'_> {
        Renderer {
            manifest,
            installer: &FakeInstaller,
            skills_source: None,
        }
    }

    fn base_packages() -> Vec<String> {
        crate::config::manifest::BASE_PACKAGES
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn render_creates_directory_structure() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();

        assert_eq!(profile, root.path().join("plan-agent"));
        // AGENTS.md = role_prompt
        let agents_md = fs::read_to_string(profile.join("AGENTS.md")).unwrap();
        assert_eq!(agents_md, manifest.get_agent("plan-agent").unwrap().role_prompt);
        // settings.json 骨架来自模型池
        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(profile.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["defaultProvider"], "newapi");
        assert_eq!(settings["defaultModel"], "mokeywork/deepseek-flash");
        // skills：无源时用内置资产（P2-ISSUE-05 修复，不再占位）
        let skill_body = fs::read_to_string(profile.join("skills/plan-tree/SKILL.md")).unwrap();
        assert!(skill_body.starts_with("---"));
        assert!(skill_body.contains("目录约定"));
    }

    #[test]
    fn dual_records_carry_base_plus_extra() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();

        // 记录 1：settings.json packages = 四件套 + ask-user-question
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("settings.json")).unwrap())
                .unwrap();
        let pkgs: Vec<&str> = settings["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(pkgs.len(), 5, "四件套+1 差异化: {pkgs:?}");
        assert!(pkgs.contains(&"npm:@juicesharp/rpiv-ask-user-question"));

        // 记录 2：npm/package.json dependencies 同步
        let pkg_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(profile.join("npm/package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(pkg_json["dependencies"].as_object().unwrap().len(), 5);
    }

    #[test]
    fn review_agent_gets_no_extra_packages() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-review").unwrap();
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("settings.json")).unwrap())
                .unwrap();
        let pkgs = settings["packages"].as_array().unwrap();
        assert_eq!(pkgs.len(), 4, "review 无差异化包: {pkgs:?}");
    }

    #[test]
    fn api_key_written_provider_keyed() {
        let mut manifest = ConfigManifest::builtin();
        manifest.api_key = Some("sk-test-123".into());
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
        let auth: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("auth.json")).unwrap()).unwrap();
        // P2-ISSUE-01 修复：pi 期望 provider 键控形状
        assert_eq!(auth["newapi"]["type"], "api_key");
        assert_eq!(auth["newapi"]["key"], "sk-test-123"); // 明文，decisions/001
        assert!(auth.get("api_key").is_none(), "旧错误形状不得残留");
    }

    #[test]
    fn models_json_carries_provider_and_budget() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
        let models: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("models.json")).unwrap()).unwrap();
        // P2-ISSUE-03 修复：baseUrl + 窗口/预算
        let newapi = &models["providers"]["newapi"];
        assert_eq!(newapi["api"], "openai-completions");
        assert_eq!(newapi["baseUrl"], "https://newapi.trtyr.top/v1");
        let list = newapi["models"].as_array().unwrap();
        assert_eq!(list.len(), 2, "池内同 provider 两个模型全量分发");
        assert!(list
            .iter()
            .any(|m| m["id"] == "mokeywork/deepseek-flash"
                && m["contextWindow"] == 1_000_000
                && m["maxTokens"] == 65_536));
    }

    #[test]
    fn settings_carry_runtime_keys() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(profile.join("settings.json")).unwrap(),
        )
        .unwrap();
        // P2-ISSUE-04 修复：运行时生存配置
        assert_eq!(settings["retry"]["maxRetries"], 3);
        assert_eq!(settings["httpIdleTimeoutMs"], 60_000);
        assert_eq!(settings["defaultThinkingLevel"], "high");
        assert_eq!(settings["compaction"]["enabled"], false);
        // 模型层键共存
        assert_eq!(settings["defaultProvider"], "newapi");
        assert_eq!(settings["defaultModel"], "mokeywork/deepseek-flash");
    }

    #[test]
    fn prompts_carry_protocol_and_workflow() {
        let manifest = ConfigManifest::builtin();
        let review = manifest.get_agent("plan-review").unwrap();
        assert!(review.role_prompt.contains("<verdict>PASS</verdict>"));
        assert!(review.role_prompt.contains("<verdict>FAIL</verdict>"));
        assert!(review.role_prompt.contains("快速审查"));
        let plan = manifest.get_agent("plan-agent").unwrap();
        assert!(plan.role_prompt.contains("小步快跑"));
    }

    #[test]
    fn skills_copied_from_source_when_given() {
        let manifest = ConfigManifest::builtin();
        // 造假 skill 源
        let skill_src = tempfile::tempdir().unwrap();
        let plan_tree = skill_src.path().join("plan-tree");
        fs::create_dir_all(&plan_tree).unwrap();
        fs::write(plan_tree.join("SKILL.md"), "# plan-tree skill 正文").unwrap();

        let root = tempfile::tempdir().unwrap();
        let r = Renderer {
            manifest: &manifest,
            installer: &FakeInstaller,
            skills_source: Some(skill_src.path()),
        };
        let profile = r.render_agent(root.path(), "plan-agent").unwrap();
        let copied = fs::read_to_string(profile.join("skills/plan-tree/SKILL.md")).unwrap();
        assert_eq!(copied, "# plan-tree skill 正文");
    }

    #[test]
    fn render_all_covers_every_agent() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let rendered = renderer(&manifest).render_all(root.path()).unwrap();
        assert_eq!(rendered.len(), 4, "规划双 Agent + 编码双 Agent");
        for name in ["plan-agent", "plan-review", "coding-agent", "coding-review"] {
            assert!(root.path().join(name).is_dir());
        }
    }

    #[test]
    fn coding_agent_profile_has_mcp_json() {
        // P005 T003：pi 原生 schema，codegraph（含遥测关闭）+ context7
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "coding-agent").unwrap();
        let mcp: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("mcp.json")).unwrap()).unwrap();
        let cg = &mcp["mcpServers"]["codegraph"];
        assert_eq!(cg["command"], "codegraph");
        assert_eq!(cg["args"][0], "serve");
        assert_eq!(cg["args"][1], "--mcp");
        assert_eq!(cg["env"]["CODEGRAPH_TELEMETRY"], "0"); // decisions/013 卫生条款
        let c7 = &mcp["mcpServers"]["context7"];
        assert_eq!(c7["command"], "npx");
        assert!(c7["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "@upstash/context7-mcp"));
        // 规划双 Agent 无 MCP：不生成文件（P001 行为不回归）
        let plan_profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
        assert!(!plan_profile.join("mcp.json").exists());
    }

    #[test]
    fn coding_packages_carry_todo_and_lens() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "coding-agent").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(profile.join("settings.json")).unwrap(),
        )
        .unwrap();
        let pkgs: Vec<&str> = settings["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(pkgs.len(), 7, "四件套+ask+todo+lens: {pkgs:?}");
        for must in [
            "npm:@juicesharp/rpiv-ask-user-question",
            "npm:@juicesharp/rpiv-todo",
            "npm:pi-lens",
        ] {
            assert!(pkgs.contains(&must), "缺 {must}: {pkgs:?}");
        }
        // review 只有诊断层，无 todo/ask（无状态一轮）
        let review = renderer(&manifest).render_agent(root.path(), "coding-review").unwrap();
        let rsettings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(review.join("settings.json")).unwrap(),
        )
        .unwrap();
        let rpkgs: Vec<&str> = rsettings["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(rpkgs.len(), 5, "四件套+lens: {rpkgs:?}");
        assert!(rpkgs.contains(&"npm:pi-lens"));
        assert!(!rpkgs.contains(&"npm:@juicesharp/rpiv-todo"));
    }

    #[test]
    fn coding_prompts_carry_protocol_and_discipline() {
        let manifest = ConfigManifest::builtin();
        let coder = manifest.get_agent("coding-agent").unwrap();
        // 阶段完成协议（T002 schema 对齐）
        assert!(coder.role_prompt.contains("<stage-complete>N</stage-complete>"));
        // 决策链锚点抽查：伴写/单测禁 e2e/git/偏差留痕/codegraph-first/刚需
        assert!(coder.role_prompt.contains("严禁写端到端测试"));
        assert!(coder.role_prompt.contains("绝不 push"));
        assert!(coder.role_prompt.contains("coding/blockers/"));
        assert!(coder.role_prompt.contains("projectPath"));
        assert!(coder.role_prompt.contains("AI-friendly"));
        assert!(coder.role_prompt.contains("勘察"));
        let reviewer = manifest.get_agent("coding-review").unwrap();
        assert!(reviewer.role_prompt.contains("<stage-verdict>PASS或FAIL</stage-verdict>"));
        assert!(reviewer.role_prompt.contains("<next>fix或advance或finish</next>"));
        assert!(reviewer.role_prompt.contains("完全基于文档写测试"));
        assert!(reviewer.role_prompt.contains("对照实际代码与文档"));
    }

    #[test]
    fn unknown_agent_rejected() {
        let manifest = ConfigManifest::builtin();
        let root = tempfile::tempdir().unwrap();
        let err = renderer(&manifest)
            .render_agent(root.path(), "ghost")
            .unwrap_err();
        assert!(matches!(err, DevforgeError::Config(_)));
    }

    #[test]
    fn base_packages_exactly_four_verbatim() {
        assert_eq!(base_packages().len(), 4);
        assert!(base_packages().contains(&"git:github.com/NVlabs/SoL-Pi".to_string()));
    }
}
