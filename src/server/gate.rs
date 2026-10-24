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
mod gate_tests {
    use super::RenderGate;

    // ---------- 010 F6 靶：渲染闸门 ----------

    #[tokio::test]
    async fn the_gate_waits_for_the_required_profiles_and_ignores_the_rest() {
        // 错实现形态：闸门不等所需 profile（start 立刻开跑 → 门卫判未渲染 → 盲重启）
        // 或等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent", "plan-review"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiter.is_finished(), "所需 profile 未就绪时闸门必须等");
        // 所需 profile 就绪（其它 profile 仍缺、渲染仍在途）→ 放行
        for p in ["plan-agent", "plan-review"] {
            let d = dir.path().join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("所需 profile 就绪后闸门应放行")
            .unwrap();
        assert!(
            gate.is_in_flight(),
            "放行不改变渲染在途状态（闸门不等整轮渲染）"
        );
    }

    #[tokio::test]
    async fn the_gate_releases_when_the_render_finishes_even_if_the_profile_is_missing() {
        // 错实现形态：渲染结束也不放行（start 永远挂着）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!waiter.is_finished(), "渲染仍在途且 profile 未就绪 → 继续等");
        gate.end_render();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("渲染结束后闸门应放行（成败由门卫复查决定）")
            .unwrap();
    }

    #[tokio::test]
    async fn the_gate_keeps_waiting_while_another_render_is_still_in_flight() {
        // 错实现形态：任意一轮渲染结束就放行（`rx.changed()` 分支直接 return）→ 并发两轮渲染时
        //   第一轮结束就把 run 放出去（所需 profile 还没落盘）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render(); // 第 1 轮渲染在途
        gate.begin_render(); // 第 2 轮渲染在途
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiter.is_finished(), "两轮渲染都在途时闸门必须等");
        // 第 1 轮结束：仍有渲染在途、所需 profile 未就绪 → 不得放行
        gate.end_render();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !waiter.is_finished(),
            "仍有渲染在途时不得放行（所需 profile 还没落盘）"
        );
        // 所需 profile 就绪 → 放行（放行条件的另一半）
        let d = dir.path().join("plan-agent");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("所需 profile 就绪后闸门应放行")
            .unwrap();
    }

    #[tokio::test]
    async fn the_gate_does_not_wait_when_no_render_is_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let gate = RenderGate::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            gate.wait_for_profiles(dir.path(), &["plan-agent"]),
        )
        .await
        .expect("无渲染在途时不应等待");
    }
}
