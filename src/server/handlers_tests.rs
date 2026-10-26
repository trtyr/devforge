//! （2026-10-26 测试分离：自 src/server/handlers.rs 内联平移，纯移动零改动。）
use super::*;

use crate::config::installer::FakeInstaller;
use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::task::Task;
use crate::server::{EventBus, ServerState, UiRelay};

/// 最小引擎：渲染闸门的用例不需要引擎行为。
struct NoopEngine;

impl RunEngine for NoopEngine {
    fn execute(
        &mut self,
        _task: &Task,
        _run: &Run,
        _control: &RunControl,
    ) -> crate::error::Result<RunOutcome> {
        Ok(RunOutcome::Completed)
    }
}

fn test_state(dir: &std::path::Path) -> SharedState {
    let bus = std::sync::Arc::new(EventBus::new());
    let ui = std::sync::Arc::new(UiRelay::new(bus.clone()));
    ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| Box::new(NoopEngine) as Box<dyn RunEngine + Send>),
        bus,
        ui,
        std::sync::Arc::new(FakeInstaller),
        dir.join("profiles"),
        dir.join("plans"),
        dir.join("sessions"),
    )
}

#[test]
fn the_in_flight_count_goes_back_to_zero_when_the_render_body_panics() {
    // 错实现形态：守卫缺失（在途计数只在线程体末尾手工递减）→ 发完成通知时 panic 展开
    //   → 计数永久大于 0 → 之后的 start 永远在闸门上等 → 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let st = test_state(dir.path());
    st.render_gate.begin_render();
    assert!(st.render_gate.is_in_flight(), "登记后应有渲染在途");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_body(st.clone(), |_| panic!("发完成通知时炸了"));
    }));
    assert!(result.is_err(), "发完成通知抛异常时线程体应展开");
    assert!(
        !st.render_gate.is_in_flight(),
        "在途计数必须归零（否则之后的 start 永远在等）"
    );
}

#[tokio::test]
async fn a_finished_render_leaves_no_in_flight_count_behind() {
    // 错实现形态：渲染结束后计数不归零（守卫没接上 / 归零那一行被删）→ 之后的 start 永远在等
    //   → 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let st = test_state(dir.path());
    let resp = match put_manifest(State(st.clone()), Json(ConfigManifest::builtin())).await {
        Ok(r) => r,
        Err(_) => panic!("内置清单应通过校验（不该 400）"),
    };
    assert_eq!(resp.0, StatusCode::ACCEPTED, "PUT 清单应立即 202（渲染在后台）");
    for _ in 0..200 {
        if !st.render_gate.is_in_flight() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        !st.render_gate.is_in_flight(),
        "渲染结束后在途计数必须归零（R-27）"
    );
}
