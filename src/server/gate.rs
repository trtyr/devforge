//! 渲染闸门（ARC-R2 自 server/mod.rs 迁入）：清单重渲染在途时，`start` 端点等
//! 「本 run 所需 profile 的 `settings.json` 落盘」或「渲染结束」再开跑。
//!
//! - 事实源：闸门与门卫读**同一张** per-SOP profile 表（`BuiltinRpcEngine::profiles_for_sop`，
//!   见 `crate::engine::builtin`），清单只有一份，不会漂移；
//! - 等待是 **awaitable** 的（watch 通知 + 短间隔复查），**不占** tokio worker
//!   （不得用 `std::thread::sleep` 轮询）；
//! - 闸门只等「所需 profile 的就绪」，**不等整轮渲染**（等整轮会把 `start` 变成分钟级阻塞点）。

use std::path::Path;
use std::sync::Mutex;

/// 渲染闸门（用户裁决 A）。
pub struct RenderGate {
    /// 在途渲染数（>0 = 有渲染在途）。
    in_flight: Mutex<usize>,
    /// 渲染完成通知（值 = 已完成代数；每次完成自增 → 订阅者的 `changed()` 会醒）。
    notify: tokio::sync::watch::Sender<u64>,
}

impl RenderGate {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(0u64);
        Self {
            in_flight: Mutex::new(0),
            notify: tx,
        }
    }

    /// 登记一次在途渲染（`PUT /config/manifest` 受理后调用）。
    pub fn begin_render(&self) {
        let mut n = self.in_flight.lock().expect("render gate 锁");
        *n += 1;
    }

    /// 结束一次渲染（成功/失败都算）并广播。
    pub fn end_render(&self) {
        {
            let mut n = self.in_flight.lock().expect("render gate 锁");
            *n = n.saturating_sub(1);
        }
        self.notify.send_modify(|v| *v += 1);
    }

    /// 是否有渲染在途。
    pub fn is_in_flight(&self) -> bool {
        *self.in_flight.lock().expect("render gate 锁") > 0
    }

    /// 等「所需 profile 的 `settings.json` 落盘」或「渲染结束」。
    ///
    /// 放行条件 = 「**无渲染在途**」**或**「**所需 profile 全部就绪**」二者之一成立
    /// （010 F6 靶 · R-2：并发两轮渲染时，先结束的那一轮**不**放行）。
    ///
    /// 返回即代表「可以开跑」：所需 profile 已就绪（后续门卫复查会过），
    /// 或渲染已结束（缺 profile 则由门卫明确失败，不再盲重启）。
    pub async fn wait_for_profiles(&self, profiles_root: &Path, required: &[&str]) {
        if required.is_empty() {
            return;
        }
        let mut rx = self.notify.subscribe();
        loop {
            if !self.is_in_flight() {
                return; // 渲染不在途 → 直接开跑
            }
            if required
                .iter()
                .all(|p| profiles_root.join(p).join("settings.json").is_file())
            {
                return; // 所需 profile 已就绪 → 不等整轮渲染
            }
            // 等「渲染结束」信号，或短间隔后重查就绪状态
            // （awaitable：不占 tokio worker；不用 std::thread::sleep 轮询）
            //
            // 收到通知**不直接放行**（R-2）：并发两轮渲染时，任意一轮结束就放行 = 闸门形同虚设
            // （所需 profile 还没落盘就把 run 放出去）→ 回环重判上面那两个放行条件。
            tokio::select! {
                _ = rx.changed() => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }
}

impl Default for RenderGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod gate_tests;
