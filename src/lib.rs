//! devforge — AI 自动化开发流水线编排平台
//!
//! 模块地图（对齐 docs/plantree/）：
//! - [`error`]：统一错误体系（原则 §1）
//! - [`logging`]：结构化日志（原则 §2）
//! - [`domain`]：三层对象模型与串行执行循环（P004）
//! - [`observability`]：run 上下文 + 观测根 + 心跳/死亡取证（plan 010 F6 靶）

pub mod config;
pub mod change;
pub mod docgen;
pub mod domain;
pub mod engine;
pub mod integration;
pub mod error;
pub mod logging;
pub mod observability;
pub mod runstate;
pub mod server;
pub mod sop;
pub mod workspace;
