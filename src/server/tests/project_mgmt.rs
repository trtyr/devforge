//! A6 项目管理面单测：list/get 回显（local_path/plantree_path/archived）、
//! 归档拦截 start、删除零任务闸。

#[allow(unused_imports)]
use super::*;

/// list/get 回显 local_path + plantree_path + archived（P019 卡片定稿字段）。
#[tokio::test]
async fn project_views_carry_paths_and_archived() {
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "card", "local_path": "/tmp/a6-card"})),
    )
    .await;

    let body = body_json(send(&f.router, "GET", "/api/v1/projects", None).await).await;
    let p = body["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "card")
        .unwrap();
    assert_eq!(p["local_path"], "/tmp/a6-card", "local_path 显式回显");
    assert_eq!(p["archived"], false);
    assert!(
        p["plantree_path"].as_str().unwrap().contains("plantree/card"),
        "plantree 路径与台账根同源：{}",
        p["plantree_path"]
    );

    let body = body_json(send(&f.router, "GET", "/api/v1/projects/card", None).await).await;
    assert_eq!(body["local_path"], "/tmp/a6-card");
    assert!(body["plantree_path"].as_str().unwrap().contains("plantree/card"));
}

/// 归档：端点置位 → list 带 archived 标记 → start 409；取消归档后 start 放行
/// （放行到任务不存在 404 即为过了项目闸）。
#[tokio::test]
async fn archive_gate_blocks_start_until_unarchived() {
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "arch", "local_path": "/tmp/a6-arch"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/arch/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;

    // 归档（缺省 true）
    let resp = send(&f.router, "POST", "/api/v1/projects/arch/archive", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(send(&f.router, "GET", "/api/v1/projects/arch", None).await).await;
    assert_eq!(body["archived"], true);

    // 归档中 start → 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/arch/tasks/t/start",
        None,
    )
    .await;
    assert_eq!(resp.status(), 409, "归档项目的 start 必须被拒：{}", resp.status());

    // 取消归档 → start 过项目闸（任务准入照旧——此处走到 run 起了即算过闸，
    // 用 running 断言太重；404=任务闸也过，放行证明成立）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/arch/archive",
        Some(json!({"archived": false})),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/arch/tasks/t/start",
        None,
    )
    .await;
    assert_ne!(resp.status(), 409, "取消归档后不再撞项目闸");

    // 写审计：归档动作落 auth_events（后续 start 也写审计——用 any 不用 last）
    let store = f.state.store.lock().unwrap();
    assert!(store
        .auth_events
        .iter()
        .any(|e| e.kind == "write_op" && e.detail.contains("archive_project")));
}

/// 删除：零任务项目可删；有任务 409 拒绝；删除动作落审计。
#[tokio::test]
async fn delete_project_gates_on_tasks() {
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "empty", "local_path": "/tmp/a6-empty"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "busy", "local_path": "/tmp/a6-busy"})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/busy/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;

    // 有任务 → 409
    let resp = send(&f.router, "DELETE", "/api/v1/projects/busy", None).await;
    assert_eq!(resp.status(), 409, "有任务的项目拒绝删除");
    // 零任务 → 删除成功 + 列表消失
    let resp = send(&f.router, "DELETE", "/api/v1/projects/empty", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(send(&f.router, "GET", "/api/v1/projects", None).await).await;
    assert!(body["projects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["name"] != "empty"));
    // 不存在的再删 → 404
    let resp = send(&f.router, "DELETE", "/api/v1/projects/empty", None).await;
    assert_eq!(resp.status(), 404);

    // 删除审计
    let store = f.state.store.lock().unwrap();
    let last = store.auth_events.last().unwrap();
    assert!(last.detail.contains("delete_project"), "{}", last.detail);
}
