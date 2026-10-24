//! RPC 编排引擎（P001 · decisions/005）：pi --mode rpc 长驻子进程 + JSONL 控制。
//!
//! - [`protocol`]：JSONL 记录编解码
//! - [`process`]：spawn 命令构造（身份标记注入，decisions/007）
//! - [`ledger`]：spawn 台账 + 孤儿对账（Q005：直接杀）
//! - [`client`]：生命周期客户端（保活 agent_settled / 唤醒 prompt / 终止关 stdin）

pub mod client;
pub mod builtin;
pub mod doc_wiring;
pub mod ledger;
pub mod process;
pub mod protocol;
pub mod retry;

#[cfg(test)]
mod builtin_tests;

pub use client::{RpcClient, WaitOutcome};
pub use ledger::{Reconciliation, SpawnLedger, LedgerEntry};
pub use process::{build_spawn_command, SpawnSpec};
pub use protocol::{decode_line, encode_line, CommandRecord, EventRecord, Inbound, ResponseRecord};
