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
                            fs::write(target.join(name), body)
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
        let manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
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
        let mut manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
        let review = manifest.get_agent("plan-review").unwrap();
        assert!(review.role_prompt.contains("<verdict>PASS</verdict>"));
        assert!(review.role_prompt.contains("<verdict>FAIL</verdict>"));
        assert!(review.role_prompt.contains("快速审查"));
        let plan = manifest.get_agent("plan-agent").unwrap();
        assert!(plan.role_prompt.contains("小步快跑"));
    }

    #[test]
    fn skills_copied_from_source_when_given() {
        let manifest = ConfigManifest::planning_builtin();
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
        let manifest = ConfigManifest::planning_builtin();
        let root = tempfile::tempdir().unwrap();
        let rendered = renderer(&manifest).render_all(root.path()).unwrap();
        assert_eq!(rendered.len(), 2);
        assert!(root.path().join("plan-agent").is_dir());
        assert!(root.path().join("plan-review").is_dir());
    }

    #[test]
    fn unknown_agent_rejected() {
        let manifest = ConfigManifest::planning_builtin();
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
