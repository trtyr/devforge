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

fn fixture() -> (
    tempfile::TempDir,
    Arc<ServerState>,
    axum::Router,
    Arc<EventBus>,
) {
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
        bus.clone(),
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
    (dir, state, router, bus)
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
    let (_dir, _state, router, _bus) = fixture();

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
    let (_dir, state, router, _bus) = fixture();
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

#[tokio::test]
async fn critical_issue_creation_raises_error_notification() {
    let (_dir, state, router, _bus) = fixture();
    let before = state.store.lock().unwrap().notifications.len();

    // critical：A2 error 通知（运营者必须马上知道）
    let resp = send(
        &router,
        "POST",
        "/api/v1/projects/devforge/issues",
        Some(issue_body("critical")),
    )
    .await;
    assert_eq!(resp.status(), 201);
    {
        let store = state.store.lock().unwrap();
        assert_eq!(store.notifications.len(), before + 1, "critical 必须发通知");
        let n = store.notifications.last().unwrap();
        assert_eq!(n.kind, "issue");
        assert_eq!(n.level, "error");
    }

    // 非 critical：不发通知（通知只给「必须马上知道」的）
    let resp = send(
        &router,
        "POST",
        "/api/v1/projects/devforge/issues",
        Some(issue_body("minor")),
    )
    .await;
    assert_eq!(resp.status(), 201);
    assert_eq!(
        state.store.lock().unwrap().notifications.len(),
        before + 1,
        "minor 不发通知"
    );
}

#[tokio::test]
async fn sse_broadcasts_issue_created_and_transitioned() {
    let (_dir, _state, router, bus) = fixture();
    let mut rx = bus.subscribe();

    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("major"))).await;
    assert_eq!(resp.status(), 201);
    let key = body_json(resp).await["key"].as_str().unwrap().to_string();

    // issue.created 先到
    let (_, raw) = rx.recv().await.unwrap();
    let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(ev["type"], "issue.created");
    assert_eq!(ev["key"], key);

    // 流转 → issue.transitioned
    send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "triaged", "version": 1})),
    )
    .await;
    let (_, raw) = rx.recv().await.unwrap();
    let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(ev["type"], "issue.transitioned");
    assert_eq!(ev["key"], key);
}

#[tokio::test]
async fn status_endpoint_counts_open_issues_by_severity() {
    let (_dir, _state, router, _bus) = fixture();
    // 两条 open：major + critical
    for sev in ["major", "critical"] {
        let resp = send(
            &router,
            "POST",
            "/api/v1/projects/devforge/issues",
            Some(issue_body(sev)),
        )
        .await;
        assert_eq!(resp.status(), 201);
    }
    let body = body_json(send(&router, "GET", "/api/v1/scheduler/status", None).await).await;
    assert_eq!(body["issues"]["open"], 2);
    assert_eq!(body["issues"]["by_severity"]["major"], 1);
    assert_eq!(body["issues"]["by_severity"]["critical"], 1);

    // triaged 一条 → open 减一（计数只看 open 态）
    send(
        &router,
        "PATCH",
        "/api/v1/issues/DEVFORGE-1",
        Some(serde_json::json!({"status": "triaged", "version": 1})),
    )
    .await;
    let body = body_json(send(&router, "GET", "/api/v1/scheduler/status", None).await).await;
    assert_eq!(body["issues"]["open"], 1);
}

#[tokio::test]
async fn run_outcome_linkage_drives_issue_states() {
    let (_dir, state, router, bus) = fixture();
    let mut rx = bus.subscribe();

    // 建 issue → Triaged → convert（Queued，task = devforge-1-fix）
    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("major"))).await;
    let key = body_json(resp).await["key"].as_str().unwrap().to_string();
    send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "triaged", "version": 1})),
    )
    .await;
    send(
        &router,
        "POST",
        &format!("/api/v1/issues/{key}/convert"),
        Some(serde_json::json!({})),
    )
    .await;

    // run Settled 联动 → Resolved + issue.resolved 事件
    crate::server::issue_run_outcome_linkage(&state, "devforge", "devforge-1-fix", "run-42", true, "");
    let body = body_json(send(&router, "GET", &format!("/api/v1/issues/{key}"), None).await).await;
    assert_eq!(body["status"], "resolved");
    let mut got_resolved = false;
    for _ in 0..4 {
        let (_, raw) = rx.recv().await.unwrap();
        let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
        if ev["type"] == "issue.resolved" {
            assert_eq!(ev["key"], key);
            assert_eq!(ev["run"], "run-42");
            got_resolved = true;
            break;
        }
    }
    assert!(got_resolved, "Settled 联动应发 issue.resolved");

    // 幂等：再 settled 一次（已 Resolved 非 Queued）→ 不再发事件
    crate::server::issue_run_outcome_linkage(&state, "devforge", "devforge-1-fix", "run-43", true, "");
    assert!(
        rx.try_recv().is_err(),
        "幂等不动不应发事件（防重放刷屏）"
    );

    // 回炉链：重开 → Triaged → 转化 → Failed 联动 → Open（不误标 Resolved）
    let v = body["version"].as_u64().unwrap();
    send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "open", "version": v})),
    )
    .await;
    let v = body_json(send(&router, "GET", &format!("/api/v1/issues/{key}"), None).await).await
        ["version"]
        .as_u64()
        .unwrap();
    send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "triaged", "version": v})),
    )
    .await;
    send(
        &router,
        "POST",
        &format!("/api/v1/issues/{key}/convert"),
        Some(serde_json::json!({"task_id": "devforge-1-fix-2"})),
    )
    .await;
    // 取消/失败同语义：都回 Open，绝不误标 Resolved
    crate::server::issue_run_outcome_linkage(
        &state,
        "devforge",
        "devforge-1-fix-2",
        "run-44",
        false,
        "取消或失败：回炉等重新处理",
    );
    let body = body_json(send(&router, "GET", &format!("/api/v1/issues/{key}"), None).await).await;
    assert_eq!(body["status"], "open", "取消/失败回 Open，不误标 Resolved");
    assert!(body["timeline"].as_array().unwrap().iter().any(|t| {
        t["action"] == "reopened" && t["detail"].as_str().unwrap().contains("run-44")
    }));
}

#[tokio::test]
async fn agent_submit_issue_lands_open_with_evidence_and_event() {
    let (_dir, state, router, bus) = fixture();
    let mut rx = bus.subscribe();
    // P020-k5：引擎 issue_sink 终点——agent 上报落 Open，自动附 run/task 证据，不发事件外的副作用
    crate::server::submit_agent_issue(
        &state,
        crate::domain::issue::AgentIssueReport {
            project: "devforge".into(),
            task_id: "t1".into(),
            run_id: "run-9".into(),
            title: "编码黑板上报".into(),
            body: "证据：src/x.rs:1".into(),
            severity: crate::domain::IssueSeverity::Major,
            category: crate::domain::IssueCategory::Flow,
        },
    );
    let body = body_json(send(&router, "GET", "/api/v1/issues/DEVFORGE-1", None).await).await;
    assert_eq!(body["source"], "agent", "agent 上报 source=Agent");
    assert_eq!(body["status"], "open", "落 Open 待分诊");
    assert_eq!(body["severity"], "major");
    assert_eq!(body["run_refs"][0], "run-9");
    assert_eq!(body["evidence"][0], "t1/run-9", "证据自动附 task/run");
    let (_, raw) = rx.recv().await.unwrap();
    let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(ev["type"], "issue.created");
    assert_eq!(ev["source"], "agent");
    assert_eq!(ev["key"], "DEVFORGE-1");
}

#[tokio::test]
async fn auto_report_run_failure_creates_and_dedupes() {
    let (_dir, state, router, bus) = fixture();
    let mut rx = bus.subscribe();

    // 终判失败上报：新建 System/Flow/Major + issue.created 事件
    crate::server::auto_report_run_failure(&state, "devforge", "t1", "run-1", "修复耗尽");
    let body = body_json(send(&router, "GET", "/api/v1/issues/DEVFORGE-1", None).await).await;
    assert_eq!(body["source"], "system");
    assert_eq!(body["category"], "flow");
    assert_eq!(body["severity"], "major");
    assert_eq!(body["run_refs"][0], "run-1");
    let (_, raw) = rx.recv().await.unwrap();
    let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(ev["type"], "issue.created");
    assert_eq!(ev["source"], "system");

    // 去重闸：再报不新建，只追加 timeline（created + auto_report = 2 条）
    crate::server::auto_report_run_failure(&state, "devforge", "t2", "run-2", "又红");
    let body = body_json(send(&router, "GET", "/api/v1/issues/DEVFORGE-1", None).await).await;
    let timeline = body["timeline"].as_array().unwrap();
    assert_eq!(timeline.len(), 2, "只追加 timeline 不新建");
    assert_eq!(timeline[1]["action"], "auto_report");
    assert!(timeline[1]["detail"].as_str().unwrap().contains("run-2"));
    let list = body_json(
        send(&router, "GET", "/api/v1/projects/devforge/issues", None).await,
    )
    .await;
    assert_eq!(list["total"], 1, "去重后仍只有一条未关 issue");
}

#[tokio::test]
async fn patch_issue_edits_fields_with_optimistic_lock() {
    let (_dir, _state, router, bus) = fixture();
    let mut rx = bus.subscribe();
    let resp = send(&router, "POST", "/api/v1/projects/devforge/issues", Some(issue_body("minor"))).await;
    let key = body_json(resp).await["key"].as_str().unwrap().to_string();

    // 字段编辑半边：severity/category 一次改 + issue.edited 事件
    let resp = send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"severity": "critical", "category": "flow", "version": 1})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["severity"], "critical");
    assert_eq!(body["category"], "flow");
    assert_eq!(body["version"], 2);
    assert_eq!(body["status"], "open", "字段编辑不动状态");
    let raw = {
        // 创建事件先入流——过滤等 issue.edited
        let mut raw = String::new();
        for _ in 0..4 {
            let (_, r) = rx.recv().await.unwrap();
            let ev: serde_json::Value = serde_json::from_str(&r).unwrap();
            if ev["type"] == "issue.edited" {
                raw = r;
                break;
            }
        }
        raw
    };
    let ev: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(ev["type"], "issue.edited");

    // 旧 version 再改 = 409；流转 + 字段同传链式推进
    let resp = send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"severity": "info", "version": 1})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    let resp = send(
        &router,
        "PATCH",
        &format!("/api/v1/issues/{key}"),
        Some(serde_json::json!({"status": "triaged", "title": "改后标题", "version": 2})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "triaged");
    assert_eq!(body["title"], "改后标题");
    assert_eq!(body["version"], 4, "流转 + 编辑各 bump 一次");
}
