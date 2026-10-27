//! 运行域公共件（P004）：RunControl（干预控制面）/ RunEngine（引擎契约）/ RunOutcome。
//!
//! 历史注：原 Runner 串行执行循环已删（2026-10-27 自审 major：生产编排全部在
//! server/change_engine，本文件 run_task 是死掉的弱化副本，仅自测在调——
//! 死代码不与生产双权威并存）。生命周期守卫语义由 Store 层承担，未变。

use crate::domain::run::Run;
use crate::domain::task::Task;
use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

/// 单 run 的干预控制面（P8-ISSUE-01 修复：取消端点 + 可选超时的载体）。
/// 零硬限制哲学：deadline 默认 None（不限时）；超时/取消均为显式外部干预。
/// 超时机制重设计（006 decisions/002）：墙钟预算（显式 opt-in）与失活收割
/// （会话无进展，显式 opt-in）分离；stalled 标记由 watchdog 在失活收割时置位，
/// 让 map_intervention 给出明确的 Failed 原因而非底层 RPC 噪音。
pub struct RunControl {
    /// 取消标志（cancel 端点置位）。**私有**——自审 minor（19）：旧口径 `pub` 原子字段
    /// 与访问器并存，调用方得以绕过封装直接 store/load（语义变更点就不可收口）。
    cancel: AtomicBool,
    /// 绝对截止时间（epoch ms）；None = 不限时。
    deadline_ms: Option<u64>,
    /// 失活标志：watchdog 判定「会话无进展超过闲置阈值」并收割时置位。
    stalled: AtomicBool,
}

impl Default for RunControl {
    fn default() -> Self {
        Self::new()
    }
}

impl RunControl {
    pub fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            deadline_ms: None,
            stalled: AtomicBool::new(false),
        }
    }

    pub fn with_deadline(mut self, deadline_ms: u64) -> Self {
        self.deadline_ms = Some(deadline_ms);
        self
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn timed_out(&self, now_ms: u64) -> bool {
        matches!(self.deadline_ms, Some(dl) if now_ms >= dl)
    }

    /// watchdog 失活收割时置位。
    pub fn mark_stalled(&self) {
        self.stalled.store(true, Ordering::SeqCst);
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled.load(Ordering::SeqCst)
    }
}

/// 单 run 执行引擎抽象。P001 的 RPC 引擎（spawn pi 子进程跑 SOP）实现本 trait。
pub trait RunEngine {
    /// 执行一次 run。Err = 技术故障（如子进程无法启动）；Ok(Failed) = 剧本内失败。
    /// control：取消/超时干预面（引擎应周期性检查；阻塞读类引擎配合 watchdog 杀进程）。
    fn execute(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Completed,
    Failed { reason: String },
}
