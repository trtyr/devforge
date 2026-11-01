//! P020 issues 端点集成测试：CRUD / 状态流转（乐观锁 409 / 非法转移 400）/
//! convert 生成 task 带 issue_ref / A1 审计落账。

#[allow(unused_imports)]
use super::*;

use crate::config::installer::FakeInstaller;
use crate::config::ConfigManifest;
use crate::domain::store::Store;
use crate::domain::SopRegistry;
use crate::observability::events::EventBus;
use crate::server::{build_router, ServerState, UiRelay};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, Arc<ServerState>, axum::Router) {
    let dir = tempfile::tempdir().unwrap();
    let bus = Arc::new(EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let st = ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("issues 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // 项目必须先存在（issue refs 锚不可悬空）
    {
        let mut store = st.store.lock().unwrap();
        store
            .create_project(crate::domain::Project::new(
                "devforge",
                dir.path().join("repo"),
                1,
            ))
            .unwrap();
    }
    let state: Arc<ServerState> = st;
    let router = build_router(state.clone());
    (dir, state, router)
}

fn issue_body(severity: &str) -> serde_json::Value {
    serde_json::json!({
        "title": "测试阶段结论可疑",
        "body": "47 号 run 的回归日志指针缺失",
        "severity": severity,
        "category": "bug",
        "run_refs": ["run-47"],
        "evidence": ["regression.log:12"]
    })
}

#[tokio::test]
async fn issue_crud_and_transition_full_chain() {
    let (_dir, _state, router) = fixture();

    // 创建：201 + key = DEVFORGE-1（项目前缀序）
    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("major"))).await;
    assert_eq!(resp.status(), 201);
    let body = body_json(resp).await;
    assert_eq!(body["key"], "DEVFORGE-1");
    assert_eq!(body["status"], "open");
    assert_eq!(body["source"], "operator");
    assert_eq!(body["version"], 1);

    // 不存在项目 = 404
    let resp = send(&router, "POST", "/api/v1/projects/ghost/issues", Some(issue_body("info"))).await;
    assert_eq!(resp.status(), 404);

    // 详情
    let resp = send(&router, "GET", "/api/v1/issues/DEVFORGE-1", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["timeline"].as_array().unwrap().len(), 1, "创建即有 created 条目");

    // 列表过滤（severity=major 命中；info 空）
    let resp = send(&router, "GET", "/api/v1/projects/devforge/issues?severity=major", None).await;
    assert_eq!(body_json(resp).await["total"], 1);
    let resp = send(&router, "GET", "/api/v1/projects/devforge/issues?severity=info", None).await;
    assert_eq!(body_json(resp).await["total"], 0);

    // 非法转移：Open → Resolved = 400（跳级）
    let resp = send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "resolved", "version": 1})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // 版本冲突 = 409
    let resp = send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "triaged", "version": 999})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    // 合法流转 Open → Triaged → Queued（手动 Queued → Resolved 仍 400）
    let resp = send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "triaged", "version": 1})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let resp = send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "queued", "version": 2})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let resp = send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "resolved", "version": 3})),
    )
    .await;
    assert_eq!(resp.status(), 400, "Queued → Resolved 只能由 run 联动");
}

#[tokio::test]
async fn issue_convert_creates_task_with_ref_and_audits() {
    let (_dir, state, router) = fixture();
    let audits_before = state.store.lock().unwrap().auth_events.len();

    // 建 issue → Triaged
    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("major"))).await;
    let issue = body_json(resp).await;
    let key = issue["key"].as_str().unwrap().to_string();
    send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "triaged", "version": 1})),
    )
    .await;

    // 转化：缺省 task id = devforge-1-fix（key 小写 + -fix）
    let resp = send(
        &router,
        "POST",
        &format!("/api/v1/issues/{key}/convert"),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(resp.status(), 201);
    let task = body_json(resp).await;
    assert_eq!(task["id"], "devforge-1-fix");
    assert_eq!(task["issue_ref"], "DEVFORGE-1", "task 带 issue 引用");

    // issue 进 Queued + task_ref 回填
    let body = body_json(send(&router, "GET", &format!("/api/v1/issues/{key}"), None).await).await;
    assert_eq!(body["status"], "queued");
    assert_eq!(body["task_ref"], "devforge-1-fix");

    // Open 的 issue 直接转化 = 400（先 Triaged）
    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("info"))).await;
    let key2 = body_json(resp).await["key"].as_str().unwrap().to_string();
    let resp = send(
        &router,
        "POST",
        &format!("/api/v1/issues/{key2}/convert"),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // A1 审计落账（create/patch/convert 三类写操作都留痕）
    let audits_after = state.store.lock().unwrap().auth_events.len();
    assert!(audits_after > audits_before, "写操作必须留审计");
}
