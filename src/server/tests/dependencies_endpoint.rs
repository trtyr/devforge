//! A4 依赖端点单测：Fake 探测器注入（路由构建前独占期换入）。

#[allow(unused_imports)]
use super::*;

use crate::config::installer::FakeInstaller;
use crate::config::ConfigManifest;
use crate::domain::SopRegistry;
use crate::observability::events::EventBus;
use crate::server::dependencies::{DependencyProbe, ProbeOutcome};
use crate::server::{build_router, ServerState, UiRelay};
use std::collections::HashMap;
use std::sync::Arc;

struct FakeProbe {
    outcomes: HashMap<String, ProbeOutcome>,
}

impl DependencyProbe for FakeProbe {
    fn probe(&self, cmd: &str, _args: &[&str]) -> ProbeOutcome {
        self.outcomes.get(cmd).cloned().unwrap_or(ProbeOutcome {
            alive: false,
            version: None,
            error: Some("未预置".into()),
        })
    }
}

/// 端点照出全清单：alive 依赖带版本、dead 带 probe_error、无写操作字段。
#[tokio::test]
async fn dependencies_endpoint_lists_with_probe_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut st = ServerState::new(
        crate::domain::store::Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("dependencies 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    let shared = Arc::get_mut(&mut st).expect("路由构建前应独占");
    shared.dependency_probe = Arc::new(FakeProbe {
        outcomes: HashMap::from([
            (
                "pi".to_string(),
                ProbeOutcome { alive: true, version: Some("1.4.0".into()), error: None },
            ),
            (
                "git".to_string(),
                ProbeOutcome { alive: false, version: None, error: Some("exit 1".into()) },
            ),
        ]),
    });
    let state: Arc<ServerState> = st;
    let router = build_router(state.clone());

    let resp = send(&router, "GET", "/api/v1/dependencies", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let deps = body["dependencies"].as_array().unwrap();
    assert_eq!(deps.len(), 3, "静态清单全量（pi/codegraph/git）");

    let pi = deps.iter().find(|d| d["id"] == "pi").unwrap();
    assert_eq!(pi["alive"], true);
    assert_eq!(pi["version"], "1.4.0");
    assert!(pi["probe_error"].is_null());

    let git = deps.iter().find(|d| d["id"] == "git").unwrap();
    assert_eq!(git["alive"], false);
    assert_eq!(git["probe_error"], "exit 1");
    assert!(git["version"].is_null());

    // 只读面：响应不含写操作字段（下载/更新是后续任务化的写端点）
    let raw = serde_json::to_string(&body).unwrap();
    assert!(!raw.contains("uninstall"), "无卸载（P019 定稿）");
    assert!(!raw.contains("auto_fix"), "无 AI 自动修复（P019 定稿）");
}
