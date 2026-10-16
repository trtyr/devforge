//! 配置管理（P002 · 双管理面：Agent 管理面 + 模型管理面）。
//!
//! 配置即代码：清单为真源，profile 目录 = 渲染产物。
//! 模板层写死（提示词/必装四件套/skills/MCP/Key），模型层可变（模型池 + Agent 引用）。

pub mod installer;
pub mod manifest;
pub mod render;

pub use installer::{build_install_command, PackageInstaller, PiInstallBridge};
pub use manifest::{AgentManifest, ConfigManifest, ModelEntry};
pub use render::Renderer;
