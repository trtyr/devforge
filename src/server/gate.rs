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
    /// 合并重渲请求（2026-10-27）：渲染在途时新到的提交不得另起一线（多线程非原子
    /// 写同一 profiles_root），只置此标记，由在跑那一轮结束时重渲一次。
    rerender_pending: std::sync::atomic::AtomicBool,
    /// 渲染完成通知（值 = 已完成代数；每次完成自增 → 订阅者的 `changed()` 会醒）。
    notify: tokio::sync::watch::Sender<u64>,
}

impl RenderGate {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(0u64);
        Self {
            in_flight: Mutex::new(0),
            rerender_pending: std::sync::atomic::AtomicBool::new(false),
            notify: tx,
        }
    }

    /// 尝试独占一轮渲染：true = 调用方负责起渲染线程（已登记在途）；
    /// false = 已有轮在跑，本次提交已合并进它（结束后自动重渲最新清单）。
    ///
    /// 与 [`Self::finish_round_or_continue`] 同锁串行：两边不可能同时判错
    /// （漏渲 = 提交了但没人渲，或双渲 = 两个线程写同一 profiles_root）。
    pub fn try_begin_or_coalesce(&self) -> bool {
        let mut n = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        if *n > 0 {
            self.rerender_pending
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return false;
        }
        *n += 1;
        true
    }

    /// 一轮渲染结束后的决策：true = 还有合并请求 → 再渲一轮（**保持在途不归零**）；
    /// false = 收工，归零并广播。持锁判 dirty 与归零是原子的——
    /// 否则「判完 dirty 到 guard 归零」之间到达的提交会永久丢掉（配置不生效、无报错）。
    pub fn finish_round_or_continue(&self) -> bool {
        let mut n = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        if self
            .rerender_pending
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return true;
        }
        *n = n.saturating_sub(1);
        drop(n);
        self.notify.send_modify(|v| *v += 1);
        false
    }

    /// 登记一次在途渲染（`PUT /config/manifest` 受理后调用）。
    pub fn begin_render(&self) {
        let mut n = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
    }

    /// 结束一次渲染（成功/失败都算）并广播。
    pub fn end_render(&self) {
        {
            let mut n = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
            *n = n.saturating_sub(1);
        }
        self.notify.send_modify(|v| *v += 1);
    }

    /// 是否有渲染在途。
    pub fn is_in_flight(&self) -> bool {
        *self.in_flight.lock().unwrap_or_else(|e| e.into_inner()) > 0
    }

    /// 等「所需 profile 渲染就绪」或「渲染结束」。
    ///
    /// 放行条件 = 「**无渲染在途**」**或**「**所需 profile 全部就绪**」二者之一成立
    /// （010 F6 靶 · R-2：并发两轮渲染时，先结束的那一轮**不**放行）。
    ///
    /// 自审 minor（115）：「就绪」不再是「settings.json 存在」——那个文件在装包**之前**就写了，
    /// 闸门看到它就会放行一个半成品 profile。现在的判据是渲染完成标记
    /// （[`crate::config::render::RENDER_READY_MARKER`]），它由渲染器**最后一步**写。
    ///
    /// 返回即代表「可以开跑」：所需 profile 已就绪（后续门卫复查会过），
    /// 或渲染已结束（缺 profile 则由门卫明确失败，不再盲重启）。
    pub async fn wait_for_profiles(&self, profiles_root: &Path, required: &[&str]) {
        if required.is_empty() {
            return;
        }
        let mut rx = self.notify.subscribe();
        // 2026-10-27 自审 major：等待必须有上界——渲染线程挂死则 in_flight 永不归零，
        // 所有 start 与调度派工永久阻塞在闸门上。超时后放行给门卫：缺 profile 由门卫
        // 明确失败、不盲重启（010 F6 契约不变）。上界 15min（单包安装超时 600s 的富余量）。
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
        loop {
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    component = "server",
                    event = "render_gate.timeout",
                    required = ?required,
                    "等待渲染闸门超时（15min）——放行给门卫做明确失败判定，不再无限等"
                );
                return;
            }
            if !self.is_in_flight() {
                return; // 渲染不在途 → 直接开跑
            }
            if required.iter().all(|p| {
                crate::config::render::rendered_fingerprint(&profiles_root.join(p)).is_some()
            }) {
                return; // 所需 profile 已按本版清单渲完 → 不等整轮渲染
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
