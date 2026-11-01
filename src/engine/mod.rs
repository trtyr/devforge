//! RPC 编排引擎（P001 · decisions/005）：pi --mode rpc 长驻子进程 + JSONL 控制。
//!
//! - [`protocol`]：JSONL 记录编解码
//! - [`process`]：spawn 命令构造（身份标记注入，decisions/007）
//! - [`ledger`]：spawn 台账 + 孤儿对账（Q005：直接杀）
//! - [`client`]：生命周期客户端（保活 agent_settled / 唤醒 prompt / 终止关 stdin）

pub mod client;
pub mod builtin;
pub mod ledger;
pub mod process;
pub mod protocol;
pub mod policy;

#[cfg(test)]
mod builtin_tests;

pub use client::{RpcClient, WaitOutcome};
pub use ledger::{Reconciliation, SpawnLedger, LedgerEntry};
pub use process::{build_spawn_command, SpawnSpec};
pub use protocol::{decode_line, CommandRecord, EventRecord, Inbound, ResponseRecord};

/// 会话目录布局单一事实源（R5-P53）：run 的会话存档根 =
/// `<sessions_root>/<run_id>/`，角色子目录（plan/reviews/coding/testing/doc-fix…）
/// 由 spawn 侧按 role 创建。读取方（transcript 端点等）经此函数取根后递归遍历，
/// 不得自拼三层布局——三处各拼一份正是 transcript 恒空的结构性根因。
pub fn session_root_for(sessions_root: &std::path::Path, run_id: &str) -> std::path::PathBuf {
    sessions_root.join(run_id)
}
