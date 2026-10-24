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
        use std::sync::atomic::Ordering;
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let data = value.to_string();
        {
            let mut log = self.log.lock().expect("event log 锁");
            log.push_back((id, data.clone()));
            while log.len() > Self::CAP {
                log.pop_front();
            }
        }
        let _ = self.tx.send((id, data));
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(u64, String)> {
        self.tx.subscribe()
    }

    /// 回放 `last_id` 之后（不含）的全部留档事件。
    pub fn replay_after(&self, last_id: u64) -> Vec<(u64, String)> {
        self.log
            .lock()
            .expect("event log 锁")
            .iter()
            .filter(|(id, _)| *id > last_id)
            .cloned()
            .collect()
    }

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
