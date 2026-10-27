//! 事件总线：单源序号 + 环形留档 + SSE 广播（可观测域，ARC-R2 自 server/mod.rs 迁入）。

use std::sync::Mutex;

/// 单一事件源：序号、环形缓冲、广播三合一。
/// 支撑 SSE 断线续传（Last-Event-ID）：emit 时分配单调序号并留档，
/// 重连客户端带 `Last-Event-ID` 即可回放错过的事件（缓冲窗口内恰一次）。
pub struct EventBus {
    seq: std::sync::atomic::AtomicU64,
    log: Mutex<std::collections::VecDeque<(u64, String)>>,
    tx: tokio::sync::broadcast::Sender<(u64, String)>,
}

impl EventBus {
    const CAP: usize = 1024;

    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(Self::CAP);
        Self {
            seq: std::sync::atomic::AtomicU64::new(0),
            log: Mutex::new(std::collections::VecDeque::with_capacity(Self::CAP)),
            tx,
        }
    }

    /// 分配序号、留档、广播（无订阅者时广播失败被忽略——留档仍可供回放）。
    pub fn emit(&self, value: serde_json::Value) {
        let data = value.to_string();
        // 自审 minor（77/78）：序号分配与留档入队必须在**同一临界区**——旧口径
        // fetch_add 在锁外、入队在锁内，两个线程可以拿到 id 5/6 却按 6、5 的顺序入队，
        // replay_after 于是返回乱序事件，SSE 重连会重复投递已收过的（违背「恰一次」）。
        let id = {
            use std::sync::atomic::Ordering;
            let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
            let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
            log.push_back((id, data.clone()));
            let mut dropped = 0usize;
            while log.len() > Self::CAP {
                log.pop_front();
                dropped += 1;
            }
            if dropped > 0 {
                // 自审 minor（158）：窗口外的丢弃要有信号，回放方才能区分
                // 「本来就没事件」与「事件已被挤出窗口」。
                tracing::warn!(
                    component = "observability",
                    event = "event.buffer_evicted",
                    dropped,
                    cap = Self::CAP,
                    "事件留档窗口已满，最旧事件被挤出（回放会缺失这段）"
                );
            }
            id
        };
        // 自审 minor（157）：广播失败要可见（无订阅者是常态 → debug；发送真错才 warn）
        match self.tx.send((id, data)) {
            Ok(n) => {
                if n == 0 {
                    tracing::trace!(component = "observability", id, "事件无订阅者（仅留档）");
                }
            }
            Err(e) => tracing::warn!(
                component = "observability",
                event = "event.broadcast_failed",
                id,
                error = %e,
                "事件广播失败（留档仍在，但实时订阅方收不到）"
            ),
        }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(u64, String)> {
        self.tx.subscribe()
    }

    /// 回放 `last_id` 之后（不含）的全部留档事件。
    pub fn replay_after(&self, last_id: u64) -> Vec<(u64, String)> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(id, _)| *id > last_id)
            .cloned()
            .collect()
    }

    /// 已分配的最大序号（含仍留在窗口内的与已被挤出的）。
    pub fn last_id(&self) -> u64 {
        self.seq
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
