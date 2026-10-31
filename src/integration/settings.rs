//! A10 · 集成开关读写面（P019 配置台）：`<plans_root>/integrations/settings.json`。
//!
//! schema（ui-9 裁决，gap-E 需求定稿）：`{"auto_integrate_enabled": bool,
//! "gates": {"clippy": bool, "doc_lint": bool}}`——**缺省全 off**（文件缺失 =
//! 功能不存在，与 ui-9「文件不存在 = 一切关闭」同口径；损坏文件也回落全 off
//! + WARN，不让手滑值炸读取方）。
//!
//! 消费方：调度器 tick 每次热读该文件（ui-11 裁决——编辑即生效，无需重启）；
//! gap-E auto-integrate 尚未在 master 落地，本读写面先行，消费端接线归 gap-E
//! 落地时补。写入原子化（tmp + rename），防半写文件被 tick 读走。

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, Result};

/// gates 子结构：集成验证的门开关（auto-integrate 的后验门）。
/// Default（derive）= 双 false，与「缺省全 off」口径一致。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrationGates {
    pub clippy: bool,
    pub doc_lint: bool,
}

/// 集成开关集（文件缺失/损坏 = 全 off 缺省——「文件不存在 = 功能不存在」）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IntegrationSettings {
    pub auto_integrate_enabled: bool,
    #[serde(default)]
    pub gates: IntegrationGates,
}

/// settings.json 的路径（`<plans_root>/integrations/settings.json`）。
pub fn settings_path(plans_root: &std::path::Path) -> std::path::PathBuf {
    plans_root.join("integrations").join("settings.json")
}

/// 读（纯函数化）：文件缺失 = 缺省全 off；损坏 = 缺省全 off + WARN（生效值
/// 端点/响应里可见，不让手滑值炸读取方）。
pub fn load(path: &std::path::Path) -> IntegrationSettings {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    component = "integration",
                    event = "integration.settings_corrupt",
                    error = %e,
                    path = %path.display(),
                    "integrations/settings.json 损坏，回落全 off（修复或删除该文件后重写）"
                );
                IntegrationSettings::default()
            }
        },
        Err(_) => IntegrationSettings::default(),
    }
}

/// 写（原子化）：tmp + rename；父目录自动创建。半写文件永不被 tick 读走。
pub fn save(path: &std::path::Path, settings: &IntegrationSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ctx("创建 integrations 目录")?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings).ctx("序列化集成设置")?)
        .ctx("写临时文件")?;
    std::fs::rename(&tmp, path).ctx("原子替换 settings.json")?;
    Ok(())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
