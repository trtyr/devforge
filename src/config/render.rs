//! Profile 渲染器（P002）：清单（真源）→ profile 目录（渲染产物，可随时重建）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::installer::PackageInstaller;
use crate::config::manifest::ConfigManifest;
use crate::error::{Context as _, DevforgeError, Result};

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

        // settings.json 骨架（模型层解析；packages 由 installer 双记录追加）
        let settings = serde_json::json!({
            "defaultProvider": model.provider,
            "defaultModel": model.model_id,
        });
        fs::write(
            dir.join("settings.json"),
            serde_json::to_string_pretty(&settings).ctx("序列化 settings")?,
        )
        .ctx(format!("写 {} 的 settings.json", agent.name))?;

        // skills 分发：有源复制，无源建占位
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
                None => {
                    fs::create_dir_all(&target).ctx("建 skill 占位目录")?;
                    fs::write(
                        target.join("README.md"),
                        format!("# {skill}\n\n占位目录——skill 内容待分发（P001 T005）。\n"),
                    )
                    .ctx("写 skill 占位 README")?;
                }
            }
        }

        // auth.json——明文 Key（P002 decisions/001：安全不设防）
        if let Some(key) = &self.manifest.api_key {
            fs::write(
                dir.join("auth.json"),
                serde_json::json!({ "api_key": key }).to_string(),
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
        assert_eq!(settings["defaultModel"], "DeepSeek flash");
        // skills 占位
        assert!(profile.join("skills/plan-tree/README.md").exists());
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
    fn api_key_written_plaintext() {
        let mut manifest = ConfigManifest::planning_builtin();
        manifest.api_key = Some("sk-test-123".into());
        let root = tempfile::tempdir().unwrap();
        let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
        let auth: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("auth.json")).unwrap()).unwrap();
        assert_eq!(auth["api_key"], "sk-test-123"); // 明文，decisions/001
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
