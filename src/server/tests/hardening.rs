//! 输入信任与路径加固（全仓审计 #3/#5/#6）：local_path 注册校验、审计报告路径参数、
//! token 常量时间比较。

#[allow(unused_imports)]
use super::*;

/// 全仓审计 #5：非空且不是 git 仓的目录必须拒——门 0 会在此 git init + add -A 建基线，
/// 误填已有目录（如 home）即把其全部内容纳入一次提交。
#[tokio::test]
async fn create_project_rejects_non_empty_non_git_dir() {
    let f = fixture();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("important.txt"), "用户的文件").unwrap();

    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": dir.path().to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 400, "非空非 git 目录应 400 拒收");
    let body = body_json(resp).await;
    assert!(
        body["error"].as_str().unwrap_or("").contains("非空且不是 git 仓"),
        "错误文案应说明原因：{body}"
    );

    // 不留半注册项目（store 里不应出现）
    let body = body_json(send(&f.router, "GET", "/api/v1/projects", None).await).await;
    assert!(
        body["projects"].as_array().unwrap().is_empty(),
        "拒收后不得留半注册项目：{body}"
    );
}

/// 合法形态：不存在（从 0 创建）/ 空目录 / 已是 git 仓 都应放行。
#[tokio::test]
async fn create_project_accepts_empty_dir_and_git_repo() {
    let f = fixture();
    let empty = tempfile::tempdir().unwrap();
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "empty", "local_path": empty.path().to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 201, "空目录应可注册");

    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "repo", "local_path": repo.path().to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 201, "已是 git 仓应可注册");

    let fresh = tempfile::tempdir().unwrap();
    let not_yet = fresh.path().join("will-be-created");
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "fresh", "local_path": not_yet.to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 201, "不存在的路径应可注册（从 0 创建）");
}

/// 全仓审计 #3：审计报告端点先按 store 校验存在性，且 404 不回显绝对路径。
#[tokio::test]
async fn audit_report_404_does_not_leak_absolute_path() {
    let f = fixture();
    let resp = send(
        &f.router,
        "GET",
        "/api/v1/projects/nope/tasks/nope-task/audit-report",
        None,
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = body_json(resp).await;
    let msg = body["error"].as_str().unwrap_or("");
    assert!(
        !msg.contains('/') || !msg.contains("plans"),
        "404 文案不得回显绝对路径：{msg}"
    );

    // 已存在项目/任务但报告未生成 → 同样 404（文案指向历史位置而非主机路径）
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "t", "sop": "planning"}))).await;
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/audit-report", None).await,
    )
    .await;
    let msg = body["error"].as_str().unwrap_or("");
    assert!(msg.contains("审计报告尚未生成"), "{msg}");
    assert!(!msg.contains(&f.state.plans_root.display().to_string()), "不得回显 plans_root：{msg}");
}

/// 全仓审计 #6：token 校验行为不变（错误 token 401、正确 token 放行）。
#[tokio::test]
async fn token_gate_still_rejects_wrong_token() {
    let f = fixture_with_token(Some("s3cret"));
    let resp = send(&f.router, "GET", "/api/v1/projects/nope/tasks/t/audit-report", None).await;
    assert_eq!(resp.status(), 401, "无 token 应 401");

    let resp = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/projects/nope/tasks/t/audit-report")
                .header("x-devforge-token", "wrong")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "错误 token 应 401");

    let resp = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/projects/nope/tasks/t/audit-report")
                .header("x-devforge-token", "s3cret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "正确 token 应通过鉴权（404 = 项目不存在）");
}
