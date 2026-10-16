//! 包安装桥接：pi install 一把梭（用户拍板 2026-10-16）。
//!
//! pi install 自动完成双记录（settings.json `packages` 数组 + `npm/package.json` dependencies），
//! 桥接只需在 profile 的配置目录语境下调用它。单测用 [`FakeInstaller`] 模拟双记录，不碰网络。

use std::path::Path;
use std::process::Command;

use crate::error::{Context as _, DevforgeError, Result};

/// 构造 pi install 命令（纯函数，可测）：`PI_CODING_AGENT_DIR=<profile> pi install <pkg>`。
pub fn build_install_command(profile_dir: &Path, package: &str) -> Command {
    let mut cmd = Command::new("pi");
    cmd.env("PI_CODING_AGENT_DIR", profile_dir)
        .arg("install")
        .arg(package);
    cmd
}

/// 包安装器抽象。真桥 = [`PiInstallBridge`]；测试 = fake（模拟双记录）。
pub trait PackageInstaller {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()>;
}

/// 真桥：逐包调 pi install（运行期用；单测不真跑——避免网络与全局 pi 依赖）。
pub struct PiInstallBridge;

impl PackageInstaller for PiInstallBridge {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()> {
        for pkg in packages {
            let status = build_install_command(profile_dir, pkg)
                .status()
                .ctx(format!("执行 pi install {pkg}"))?;
            if !status.success() {
                return Err(DevforgeError::Config(format!(
                    "pi install {pkg} 失败（exit: {status}）"
                )));
            }
        }
        Ok(())
    }
}

/// 测试桥：模拟 pi install 的双记录行为——
/// 追加 `<profile>/settings.json` 的 `packages` 数组 + 写 `<profile>/npm/package.json`。
#[derive(Default)]
pub struct FakeInstaller;

impl PackageInstaller for FakeInstaller {
    fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()> {
        // 记录 1：settings.json packages 数组追加
        let settings_path = profile_dir.join("settings.json");
        let mut settings: serde_json::Value = if settings_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&settings_path).ctx("读 settings")?)
                .unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        let arr = settings
            .as_object_mut()
            .expect("settings 应为对象")
            .entry("packages")
            .or_insert_with(|| serde_json::json!([]));
        for pkg in packages {
            arr.as_array_mut().expect("packages 应为数组").push(serde_json::json!(pkg));
        }
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap())
            .ctx("写 settings")?;

        // 记录 2：npm/package.json dependencies
        let npm_dir = profile_dir.join("npm");
        std::fs::create_dir_all(&npm_dir).ctx("建 npm 目录")?;
        let pkg_json_path = npm_dir.join("package.json");
        let mut pkg_json: serde_json::Value = if pkg_json_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&pkg_json_path).ctx("读 package.json")?)
                .unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({"name": "pi-extensions", "private": true, "dependencies": {}})
        };
        let deps = pkg_json
            .as_object_mut()
            .expect("package.json 应为对象")
            .entry("dependencies")
            .or_insert_with(|| serde_json::json!({}));
        for pkg in packages {
            deps.as_object_mut()
                .expect("dependencies 应为对象")
                .insert(pkg.clone(), serde_json::json!("*"));
        }
        std::fs::write(&pkg_json_path, serde_json::to_string_pretty(&pkg_json).unwrap())
            .ctx("写 package.json")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_command_carries_profile_env() {
        let cmd = build_install_command(Path::new("/profiles/plan-agent"), "npm:pi-blackhole");
        assert_eq!(cmd.get_program(), "pi");
        let env = cmd.get_envs().find(|(k, _)| *k == "PI_CODING_AGENT_DIR").unwrap();
        assert_eq!(env.1, Some(Path::new("/profiles/plan-agent").as_os_str()));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, vec!["install", "npm:pi-blackhole"]);
    }

    #[test]
    fn fake_installer_writes_dual_records() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("plan-agent");
        std::fs::create_dir_all(&profile).unwrap();
        FakeInstaller
            .install(&profile, &["npm:pi-blackhole".into(), "npm:pi-fff".into()])
            .unwrap();

        // 记录 1：settings.json packages
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(profile.join("settings.json")).unwrap())
                .unwrap();
        let pkgs = settings["packages"].as_array().unwrap();
        assert_eq!(pkgs.len(), 2);

        // 记录 2：npm/package.json dependencies
        let pkg_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(profile.join("npm/package.json")).unwrap())
                .unwrap();
        let deps = pkg_json["dependencies"].as_object().unwrap();
        assert!(deps.contains_key("npm:pi-blackhole"));
        assert!(deps.contains_key("npm:pi-fff"));
    }
}
