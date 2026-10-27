//! P012-T004 回归：输入边界入口收口（S-01 入口半边 + base_rev 真实性）。

#[allow(unused_imports)]
use super::*;

#[tokio::test]
async fn create_project_rejects_path_traversal_name() {
    let f = fixture();
    for bad in ["../evil", "a/b", "..", "", "."] {
        let resp = send(
            &f.router,
            "POST",
            "/api/v1/projects",
            Some(json!({"name": bad, "local_path": "/tmp/ws-p012-name"})),
        )
        .await;
        assert_eq!(resp.status(), 400, "项目名 {bad:?} 应被入口拒收");
    }
}

#[tokio::test]
async fn create_task_rejects_path_traversal_id() {
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-p012-id"})),
    )
    .await;
    for bad in ["a/../b", "batch/a", "..", ""] {
        let resp = send(
            &f.router,
            "POST",
            "/api/v1/projects/p/tasks",
            Some(json!({"id": bad, "sop": "change"})),
        )
        .await;
        assert_eq!(resp.status(), 400, "任务 id {bad:?} 应被入口拒收");
    }
}

#[tokio::test]
async fn create_task_rejects_unverifiable_base_rev() {
    let f = fixture();
    // local_path 用空目录（validate_local_path 合法形态之一）：非 git 仓，
    // 任何 base_rev 的 cat-file 探测必失败 → 拒收（避免 run 跑完才发现契约不可用）。
    let ws = f._dir.path().join("ws-empty");
    std::fs::create_dir_all(&ws).unwrap();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": ws.to_string_lossy()})),
    )
    .await;
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({
            "id": "t1",
            "sop": "change",
            "contract": {
                "owned_paths": ["src/"],
                "base_rev": "deadbeef-not-a-real-rev",
                "isolation": "worktree"
            }
        })),
    )
    .await;
    assert_eq!(resp.status(), 400, "假 base_rev 应在入口被拒（S-02 修法②）");
}

#[tokio::test]
async fn create_task_without_base_rev_still_works() {
    let f = fixture();
    let ws = f._dir.path().join("ws-empty2");
    std::fs::create_dir_all(&ws).unwrap();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": ws.to_string_lossy()})),
    )
    .await;
    // 无 base_rev（Option::None）：不做 git 探测，正常创建
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({
            "id": "t1",
            "sop": "change",
            "contract": {"owned_paths": ["src/"], "isolation": "worktree"}
        })),
    )
    .await;
    assert_eq!(resp.status(), 201, "无 base_rev 的契约不受校验影响");
}
