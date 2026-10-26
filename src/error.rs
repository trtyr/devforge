//! 统一错误体系（architecture-principles §1：完整错误处理系统）。
//!
//! 设计要点：
//! - 可恢复与不可恢复分明：[`DevforgeError::Agent`] 是子 Agent 的失败，
//!   编排器可以捕获并决策（重试/打回/终止），不连坐整个流水线；
//!   [`DevforgeError::System`] 是编排器自身故障。
//! - 错误必须带上下文：用 [`Context::ctx`] 给错误链挂「谁、在哪、干什么」。

use thiserror::Error;

/// devforge 统一 Result 别名。
pub type Result<T, E = DevforgeError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DevforgeError {
    /// 子 Agent 相关失败（进程退出、RPC 无响应、协议破坏等）。
    /// 可恢复：编排器捕获后按剧本决策，不连带崩溃。
    #[error("agent 失败 [{role}]: {message}")]
    Agent { role: String, message: String },

    /// 基础设施/环境故障（外部瞬时故障）。
    ///
    /// 可恢复：唯一会被 engine::policy 自动重启的类（plan 010 D004）——
    /// 质量失败重启只会得到同一个结论，System 重启赌的是环境恢复。
    /// 自审 major：旧标签「编排器自身故障（不可恢复类）」与 6 行之隔的 Quality
    /// 文档、policy 的实际恢复策略两头矛盾，新人按标签会用反。
    #[error("系统错误: {0}")]
    System(String),

    /// 质量判定失败（审查打回、协议违例达上限、越界申报、验收不通过）。
    ///
    /// 与 [`DevforgeError::System`] 的区别只在恢复策略上（plan 010 D004）：
    /// 系统错误是外部故障，可以自动重启；质量失败重启只会得到同一个结论，必须停下来给人看。
    #[error("质量失败: {0}")]
    Quality(String),

    /// 配置/清单错误。
    #[error("配置错误: {0}")]
    Config(String),

    /// 数据模型/存储错误。
    #[error("存储错误: {0}")]
    Store(String),

    /// RPC 协议错误（记录损坏、意外 EOF、响应缺失等）。
    #[error("RPC 协议错误: {0}")]
    RpcProtocol(String),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// 带上下文的包装错误（用 [`Context::ctx`] 产生）。
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<DevforgeError>,
    },
}

impl DevforgeError {
    /// 剥开 WithContext 包装链取根因（引擎审计修复 #3：分类判定看根因不看包装）。
    pub fn root_cause(&self) -> &DevforgeError {
        let mut cur = self;
        while let DevforgeError::WithContext { source, .. } = cur {
            cur = source;
        }
        cur
    }

    /// 便捷构造：子 Agent 失败。
    pub fn agent(role: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Agent {
            role: role.into(),
            message: message.into(),
        }
    }

    /// 是否为子 Agent 失败（可恢复类，编排器可决策）。
    /// 地基批次 #1：走 `root_cause()`——`Quality`/`Agent` 被 `.ctx()` 包装后仍要认得出，
    /// 否则分类口径与 `is_infra_failure` 分裂。
    pub fn is_agent_failure(&self) -> bool {
        matches!(self.root_cause(), Self::Agent { .. })
    }

    /// 是否为质量失败（不自动重启，plan 010 D004）。同上：剥包装后再判。
    pub fn is_quality_failure(&self) -> bool {
        matches!(self.root_cause(), Self::Quality(_))
    }
}

/// 上下文扩展：给错误链挂上下文。
///
/// ```no_run
/// use devforge::error::{Context, Result};
/// fn read_cfg() -> Result<String> {
///     std::fs::read_to_string("cfg.json").ctx("读取配置文件")
/// }
/// ```
pub trait Context<T> {
    fn ctx(self, context: impl Into<String>) -> Result<T>;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: Into<DevforgeError>,
{
    fn ctx(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|source| DevforgeError::WithContext {
            context: context.into(),
            source: Box::new(source.into()),
        })
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
