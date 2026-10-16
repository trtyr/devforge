//! devforge — AI 自动化开发流水线编排平台
//!
//! 模块地图（对齐 docs/plantree/）：
//! - [`error`]：统一错误体系（原则 §1）
//! - [`logging`]：结构化日志（原则 §2）
//! - [`domain`]：三层对象模型与串行执行循环（P004）

pub mod config;
pub mod domain;
pub mod engine;
pub mod error;
pub mod logging;
