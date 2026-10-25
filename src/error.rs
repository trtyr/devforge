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

    /// 编排器自身故障（不可恢复类）。
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
    pub fn is_agent_failure(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    /// 是否为质量失败（不自动重启，plan 010 D004）。
    pub fn is_quality_failure(&self) -> bool {
        matches!(self, Self::Quality(_))
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
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn display_includes_context_and_source() {
        let err: DevforgeError = std::fs::read_to_string("/nonexistent/file")
            .ctx("读取 plan tree")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("读取 plan tree"), "缺上下文: {msg}");
        assert!(msg.contains("IO"), "缺源头: {msg}");
    }

    #[test]
    fn source_chain_walkable() {
        let err: DevforgeError = serde_json::from_str::<String>("not json")
            .ctx("解析 RPC 记录")
            .unwrap_err();
        // 链：WithContext -> Json
        let source = err.source().expect("应有 source");
        assert!(source.to_string().contains("JSON"));
    }

    #[test]
    fn agent_failure_is_classified() {
        let e = DevforgeError::agent("plan-review", "进程异常退出");
        assert!(e.is_agent_failure());
        assert!(e.to_string().contains("plan-review"));

        let sys = DevforgeError::System("台账损坏".into());
        assert!(!sys.is_agent_failure());
        assert!(!sys.is_quality_failure());

        let q = DevforgeError::Quality("审查连续违例".into());
        assert!(q.is_quality_failure());
        assert!(!q.is_agent_failure());
    }
}
