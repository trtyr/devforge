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
    /// 子 Agent 的**决策类**失败（进程退出、模型通道错误、require_success 拒收等）。
    /// 可恢复：编排器捕获后按剧本决策，不连带崩溃。
    ///
    /// 注意（自审 ref 142）：**响应缺失/记录损坏/意外 EOF 归 [`DevforgeError::RpcProtocol`]**，
    /// 不归这里——两者恢复策略不同（协议破坏 = 坏数据，不是外部故障）。旧文档把
    /// 「RPC 无响应、协议破坏」写进本变体，按文档写判定会写错。
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

    /// 是否为配置错误（确定性：修配置后重跑，不是瞬时故障）。
    ///
    /// 自审 ref 69：旧口径只有 Agent/Quality 两个谓词，消费方被迫自己写 `matches!`，
    /// 且同一变体在不同层用互斥匹配策略（顶层 vs root_cause，修复漏掉了重试层）。
    pub fn is_config_error(&self) -> bool {
        matches!(self.root_cause(), Self::Config(_))
    }

    /// 是否为存储/数据模型错误（重启不修数据）。
    pub fn is_store_error(&self) -> bool {
        matches!(self.root_cause(), Self::Store(_))
    }

    /// 是否为 RPC 协议错误（坏数据，不是外部故障）。
    pub fn is_protocol_error(&self) -> bool {
        matches!(self.root_cause(), Self::RpcProtocol(_))
    }

    /// 是否应当自动重启（**唯一判定源**，engine::policy 与 HTTP 层都读它）。
    ///
    /// 可重启：`System`（外部瞬时故障）或 `Io`（spawn 层瞬时资源耗尽）。
    /// 其余一律不重启——重启只会得到同一个结论（Quality/Config）、或数据本身坏了
    /// （Store/Json/RpcProtocol）、或要交给编排器决策（Agent）。
    pub fn is_restartable(&self) -> bool {
        matches!(self.root_cause(), Self::System(_) | Self::Io(_))
    }

    /// 机器可读错误码（自审 ref 144）：API 层不该靠中文关键词嗅探映射状态码。
    ///
    /// 取值是稳定的短标识：`agent` / `system` / `quality` / `config` / `store` /
    /// `rpc` / `io` / `json` / `context`。显示给用户仍用 Display。
    pub fn code(&self) -> &'static str {
        match self.root_cause() {
            Self::Agent { .. } => "agent",
            Self::System(_) => "system",
            Self::Quality(_) => "quality",
            Self::Config(_) => "config",
            Self::Store(_) => "store",
            Self::RpcProtocol(_) => "rpc",
            Self::Io(_) => "io",
            Self::Json(_) => "json",
            Self::WithContext { .. } => "context",
        }
    }

    /// 错误链上的上下文层（自审 ref 146：模块头承诺「谁、在哪、干什么」，
    /// 但 `WithContext.context` 是不透明字符串——这里给出可机读的逐层访问器）。
    pub fn context_layers(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut cur = self;
        while let Self::WithContext { context, source } = cur {
            out.push(context.as_str());
            cur = source;
        }
        out
    }

    /// 链上的子 Agent 角色（自审 ref 146：`Agent.role` 此前被压进 Display 文本，
    /// 事后无法按角色过滤）。
    pub fn agent_role(&self) -> Option<&str> {
        match self.root_cause() {
            Self::Agent { role, .. } => Some(role.as_str()),
            _ => None,
        }
    }

    /// 人读文本（**净化版**，自审 ref 143）：子 Agent 的错误文本是不可信输入，
    /// 进 stderr/API 前要限长 + 去控制字符/ANSI 转义（旧口径原样透传）。
    pub fn sanitized_message(&self) -> String {
        sanitize_untrusted(&self.to_string())
    }
}

/// 不可信文本净化：去 ANSI 转义与控制字符、压成单行、限长 2000 字符。
///
/// 用途：子 Agent 产出（错误消息、失败明细）进入人类可读层/API 响应体之前。
pub fn sanitize_untrusted(text: &str) -> String {
    const MAX: usize = 2000;
    let mut out = String::with_capacity(text.len().min(MAX));
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        // ANSI：ESC '[' … 终止字节（@-~）
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                let _ = chars.next();
                while let Some(&n) = chars.peek() {
                    let _ = chars.next();
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        if c.is_control() {
            // 换行/制表压成空格（保持单行，避免污染日志行解析）
            if matches!(c, '\n' | '\r' | '\t') && !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(c);
        if out.chars().count() >= MAX {
            out.push_str("…（已截断）");
            break;
        }
    }
    out
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
