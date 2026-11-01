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

// 集成目标 ref：**由消费时探测**（`consumer::resolve_target_ref`——目标仓当前
// 检出分支，2026-11-01 裁决①）。旧的硬编码 `refs/heads/master` 常量已删
// （R3-07：main 缺省仓恒失败）；仍不自动 push origin（git 节奏仍归用户），
// 也不做配置化（配置写错 → 合到错分支的新失败面，原裁决理由维持）。

/// gates 子结构：集成验证的门开关（auto-integrate 的后验门）。
/// Default（derive）= 双 false，与「缺省全 off」口径一致。
/// Copy：全 bool 值对象（消费闭包按值捕获一份，不借用外层状态）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrationGates {
    pub clippy: bool,
    pub doc_lint: bool,
}

/// 集成开关集（文件缺失/损坏 = 全 off 缺省——「文件不存在 = 功能不存在」）。
/// Copy：全 bool 值对象（消费循环内层闭包按值捕获）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct IntegrationSettings {
    /// 自动集成总开关：**缺省关**。打开 = 运营者授权机器在验证通过后**直接改写目标
    /// 分支**（git update-ref，无逐条人工批准环节——R4-P29 裁决 2026-11-01：开关
    /// 本身就是批准动作，「开它 = 授权机器直改 master」）。关闭时消费循环零动作
    /// （连队列都不读，不产生任何集成 worktree）。
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
