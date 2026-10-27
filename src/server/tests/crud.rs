//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

use super::*;

/// 有界 409 重试的 start（自审 minor 133/134）：F18 后 status=settled 与串行槽真正释放
/// 之间有收尾窗口，偶发 409 会淹没断言本体；旧口径只有第三个 start 带防护，前两个
/// 直接断言 202 → 竞态发作时以误导性断言失败。100×20ms = 2s 上限，超限直接失败而非挂死。
async fn start_bounded(
    f: &Fixture,
    path: &str,
    body: Option<Value>,
) -> axum::response::Response {
    for _ in 0..100 {
        let r = send(&f.router, "POST", path, body.clone()).await;
        if r.status() != 409 {
            return r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("{path} 在 2s 内持续 409（串行槽未释放）——超出有界重试上限");
}

#[tokio::test]
async fn projects_crud_with_local_dir_creation() {
    let f = fixture();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new-project");

    // 创建（201）且本地目录落地（decisions/001 手动指定 + T004 ensure）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "devforge", "local_path": target.to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 201);
    assert!(target.is_dir(), "项目目录应创建");

    // 重复 → 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "devforge", "local_path": target.to_string_lossy()})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    // 列表与详情
    let resp = send(&f.router, "GET", "/api/v1/projects", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["projects"].as_array().unwrap().len(), 1);

    let resp = send(&f.router, "GET", "/api/v1/projects/devforge", None).await;
    assert_eq!(resp.status(), 200);
    let resp = send(&f.router, "GET", "/api/v1/projects/ghost", None).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn task_creation_validates() {
    let f = fixture();
        // 自审 major：不用机器全局 /tmp/p——锚定在本用例的临时目录，不依赖宿主状态、不遗留
    let repo_p = f.state.plans_root.parent().unwrap().join("repo-p");
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": repo_p.to_string_lossy()}))).await;

    // 未知 SOP → 400（P005：coding 已入注册表，改用 ghost）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "ghost"})),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // P005：coding 已注册 → 201
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t-coding", "sop": "coding"})),
    )
    .await;
    assert_eq!(resp.status(), 201);

    // 正常创建 → 201
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 201);

    // 重复 → 409
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t1", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 409);

    // 不存在项目下的任务 → 404
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/ghost/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn start_doc_from_zero_sop_returns_202() {
    // P008 T006：新 SOP 注册后能被 API 触发（引擎 mock，立即完成）
    let f = fixture();
        // 自审 major：不用机器全局 /tmp/p——锚定在本用例的临时目录，不依赖宿主状态、不遗留
    let repo_p = f.state.plans_root.parent().unwrap().join("repo-p");
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": repo_p.to_string_lossy()}))).await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks", Some(json!({"id": "d", "sop": "doc-from-zero"}))).await;

    let resp = start_bounded(&f, "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);

    // 自审 minor（242）：用共享的 wait_settled（它只看**最新**那条 run）；
    // 旧口径在本文件内联了第二套「只看 runs[0]」的等落定范式，正是 helper 注释警告的陷阱。
    wait_settled(&f, "p", "d").await;
}

#[tokio::test]
async fn start_change_sop_returns_202() {
    // P009 接线：change 已注册，start 应受理（引擎是 mock，不等真 pi）
    let f = fixture();
        // 自审 major：不用机器全局 /tmp/p——锚定在本用例的临时目录，不依赖宿主状态、不遗留
    let repo_p = f.state.plans_root.parent().unwrap().join("repo-p");
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": repo_p.to_string_lossy()}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "c", "sop": "change"})),
    )
    .await;
    let resp = start_bounded(&f, "/api/v1/projects/p/tasks/c/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["sop"], "change");
    wait_settled(&f, "p", "c").await;
    let runs = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/c/runs", None).await,
    )
    .await;
    assert_eq!(runs["runs"][0]["status"], "settled");
}

#[tokio::test]
async fn start_change_resumes_from_blackboard_and_fresh_clears_it() {
    // task-6：change 的 start 续跑/fresh 与 010 同一套语义（按 SOP 分文件）
    let f = fixture();
        // 自审 major：不用机器全局 /tmp/p——锚定在本用例的临时目录，不依赖宿主状态、不遗留
    let repo_p = f.state.plans_root.parent().unwrap().join("repo-p");
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": repo_p.to_string_lossy()}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "c", "sop": "change"})),
    )
    .await;

    let key = crate::runstate::StateKey::new("p", "c", "change");
    let mut st = crate::runstate::RunState::new(
        &key,
        json!({"change": {"phase": "coding", "baseline_sha": "abc", "roundtrips": 1}}),
    );
    crate::runstate::save(&f.state.plans_root, &key, &mut st).unwrap();

    let resp = start_bounded(&f, "/api/v1/projects/p/tasks/c/start", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], true, "有断点必须续跑：{body}");
    wait_settled(&f, "p", "c").await;

    let resp = start_bounded(
        &f,
        "/api/v1/projects/p/tasks/c/start",
        Some(json!({"fresh": true})),
    )
    .await;
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "fresh 必须冷启动：{body}");
    assert!(
        !matches!(
            crate::runstate::load(&f.state.plans_root, &key),
            crate::runstate::Loaded::Found(_)
        ),
        "fresh 后 change 断点必须清空"
    );
}

#[tokio::test]
async fn start_reports_resumed_and_fresh_clears_only_that_sop() {
    // plan 010 T003：start 默认续跑、fresh 只清该 SOP 的断点
    let f = fixture();
        // 自审 major：不用机器全局 /tmp/p——锚定在本用例的临时目录，不依赖宿主状态、不遗留
    let repo_p = f.state.plans_root.parent().unwrap().join("repo-p");
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": repo_p.to_string_lossy()}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "d", "sop": "doc-from-zero"})),
    )
    .await;

    // 1) 无断点 → 冷启动
    let resp = start_bounded(&f, "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "无断点必须冷启动：{body}");
    wait_settled(&f, "p", "d").await;

    // 2) 种断点（doc-from-zero + 另一个 SOP）→ 默认续跑，且只认自己那份
    let key = crate::runstate::StateKey::new("p", "d", "doc-from-zero");
    let mut st = crate::runstate::RunState::new(&key, json!({"done": ["unit-a"]}));
    crate::runstate::save(&f.state.plans_root, &key, &mut st).unwrap();
    let other = crate::runstate::StateKey::new("p", "d", "planning");
    let mut st2 = crate::runstate::RunState::new(&other, json!({"done": ["x"]}));
    crate::runstate::save(&f.state.plans_root, &other, &mut st2).unwrap();

    let resp = start_bounded(&f, "/api/v1/projects/p/tasks/d/start", None).await;
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], true, "有断点默认续跑：{body}");
    assert_eq!(body["sop"], "doc-from-zero");
    wait_settled(&f, "p", "d").await;

    // 3) fresh → 回冷启动且断点被清；另一个 SOP 的断点不受影响
    // F18：status=Settled 与串行槽真正释放之间有收尾窗口（end_run 终态写等），
    // 偶发 409 会淹没断言本体 → 409 有界重试，其余状态照常断言
    // 自审 major：无界 loop 在服务端持续 409 时永久挂起（注释声称「有界」实为无界）。
    // 100 次 × 20ms = 2s 上限，超限直接失败而非挂死 CI。
    let mut resp = None;
    for _ in 0..100 {
        let r = send(
            &f.router,
            "POST",
            "/api/v1/projects/p/tasks/d/start",
            Some(json!({"fresh": true})),
        )
        .await;
        if r.status() != 409 {
            resp = Some(r);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let resp = resp.expect("fresh start 在 2s 内持续 409（串行槽未释放）——超出有界重试上限");
    assert_eq!(resp.status(), 202);
    let body = body_json(resp).await;
    assert_eq!(body["resumed"], false, "fresh 必须冷启动：{body}");
    assert!(
        body["note"].as_str().unwrap_or("").contains("fresh"),
        "fresh 原因要回显：{body}"
    );
    assert!(
        !matches!(
            crate::runstate::load(&f.state.plans_root, &key),
            crate::runstate::Loaded::Found(_)
        ),
        "fresh 后该 SOP 的 done 必须为空"
    );
    assert!(
        matches!(
            crate::runstate::load(&f.state.plans_root, &other),
            crate::runstate::Loaded::Found(_)
        ),
        "不得动其他 SOP 的断点"
    );
}


