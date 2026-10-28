//! devforge — AI 自动化开发流水线编排平台
//!
//! 模块地图（对齐 docs/plantree/）：
//! - [`error`]：统一错误体系（原则 §1）
//! - [`logging`]：结构化日志（原则 §2）
//! - [`domain`]：三层对象模型与串行执行循环（P004）
//! - [`observability`]：run 上下文 + 观测根 + 心跳/死亡取证（plan 010 F6 靶）

// 外部审计 #1（2026-10-28）：产品依赖 /proc 与 POSIX 进程接口（spawn 台账、孤儿对账、
// 进程检查），目标平台是 Linux 服务器。非 Linux 不做支持声明——直接给出可读的编译
// 错误，而不是让 libc 符号缺失之类的五个 E0425/E0433 一起炸出来。
#[cfg(not(target_os = "linux"))]
compile_error!(
    "devforge 的目标平台是 Linux（依赖 /proc 与 POSIX 进程接口）；其他平台未支持，见 README「平台」一节"
);

pub mod config;
pub mod change;
pub mod audit;
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
