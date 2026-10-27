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
    // 自审 minor（243）：用真 git 仓（旧口径只建空 `.git` 目录，实际钉住的是
    // 「存在 .git 即放行」的弱规则，真仓从未被验证过）。
    let st = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(repo.path())
        .status()
        .expect("git 应可用");
    assert!(st.success(), "git init 应成功");
    assert!(repo.path().join(".git").is_dir());
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

/// 自审 major 回归：审计报告端点的路径穿越守卫 safe_segment 必须有负向覆盖——
/// project/task 含穿越段时 400，删掉守卫本用例变红。
#[tokio::test]
async fn audit_report_rejects_path_traversal_segments() {
    let f = fixture();
    // 注：裸 "a/b" 在路由层就拆成两段匹配不到 {name}（404），到不了 handler；
    // 真正能打到 safe_segment 的是 ".." 与 URL 编码的 %2F。
    for evil in ["..", "a%2Fb"] {
        let url = format!("/api/v1/projects/{evil}/tasks/t/audit-report");
        let resp = send(&f.router, "GET", &url, None).await;
        assert_eq!(
            resp.status(),
            400,
            "穿越段 {evil} 必须被 safe_segment 拦下（400），实际 {}",
            resp.status()
        );
    }
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
    // 自审 minor（244）：旧断言是析取式（`!/ || !plans`）——只要文案不是同时含 '/' 与
    // "plans" 就能通过，其余任何绝对路径泄露全部放行。现在：禁止任何**绝对路径 token**。
    assert!(
        !msg.split_whitespace().any(|w| w.starts_with('/')),
        "404 文案不得回显绝对路径：{msg}"
    );
    assert!(
        !msg.contains(&f.state.plans_root.display().to_string()),
        "404 文案不得回显 plans_root：{msg}"
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

/// 自审 minor（245）：模块文档宣称「token 常量时间比较」，但单测只验 401/404 状态码——
/// 回退到旧的按字节早退比较仍全绿。计时侧信道没法在单测里测，所以钉**实现路径**：
/// token 校验必须调 ct_eq，且不得出现早退式 `==` 比较。
#[test]
fn token_comparison_stays_constant_time() {
    let src = include_str!("../handlers.rs");
    let at = src.find("fn token_matches").expect("token_matches 应存在");
    let end = (at + 400).min(src.len());
    let mut end = end;
    while end < src.len() && !src.is_char_boundary(end) {
        end += 1;
    }
    let body = &src[at..end];
    assert!(body.contains("ct_eq("), "token 校验必须走常量时间比较：{body}");
    assert!(
        !body.contains("== expected") && !body.contains("g =="),
        "不得回退到按字节早退比较：{body}"
    );
    // 缺失 token 不得短路成「相等」
    assert!(body.contains("None => false"), "缺失 token 必须判不匹配：{body}");
}

/// 自审 critical：GET /config/manifest 不得回传明文 api_key；PUT 见脱敏占位 = 保持
/// 原值（GET→改→PUT 往返不丢凭据）。
#[tokio::test]
async fn get_manifest_masks_api_key_and_roundtrip_keeps_it() {
    let f = fixture();
    let mut m = serde_json::to_value(ConfigManifest::builtin()).unwrap();
    m["api_key"] = json!("sk-secret-123");
    let resp = send(&f.router, "PUT", "/api/v1/config/manifest", Some(m)).await;
    assert_eq!(resp.status(), 202);

    let body = body_json(send(&f.router, "GET", "/api/v1/config/manifest", None).await).await;
    assert_eq!(
        body["api_key"], "__redacted__",
        "GET 必须回脱敏占位：{body}"
    );
    assert!(
        !body.to_string().contains("sk-secret-123"),
        "明文 Key 不得出现在响应里"
    );

    // 往返：把 GET 到的 body 原样 PUT 回去 → 服务端仍持有真值
    let resp = send(&f.router, "PUT", "/api/v1/config/manifest", Some(body)).await;
    assert_eq!(resp.status(), 202, "占位往返必须放行");
    assert_eq!(
        f.state.manifest.lock().expect("manifest 锁").api_key,
        Some("sk-secret-123".into()),
        "占位 PUT 后真值必须保留"
    );
}

/// 自审 critical：agent 名 / skill 名会被渲染器 join 进 profiles_root 并落明文 Key
/// 的 auth.json——路径段不安全必须 400 拒收，不得走到渲染。
#[tokio::test]
async fn put_manifest_rejects_unsafe_agent_name_and_skill() {
    let f = fixture();
    let mut m = serde_json::to_value(ConfigManifest::builtin()).unwrap();
    let name0 = m["agents"][0]["name"].as_str().unwrap().to_string();
    m["agents"][0]["name"] = json!("../../evil");
    let resp = send(&f.router, "PUT", "/api/v1/config/manifest", Some(m.clone())).await;
    assert_eq!(resp.status(), 400, "穿越 agent 名应 400");

    m["agents"][0]["name"] = json!(name0);
    m["agents"][0]["skills"] = json!(["legit", "../escape"]);
    let resp = send(&f.router, "PUT", "/api/v1/config/manifest", Some(m)).await;
    assert_eq!(resp.status(), 400, "穿越 skill 名应 400");
}

/// 自审 critical：集成队列的 project 段直接 join 进 plans_root/integrations/，
/// 不过 safe_segment = 路径穿越 + 任意目录创建/覆写。
#[tokio::test]
async fn integration_queue_rejects_path_traversal_project() {
    let f = fixture();
    // %2e%2e = ".."，axum Path 解码后交给 handler 的是穿越段
    let resp = send(
        &f.router,
        "GET",
        "/api/v1/projects/%2e%2e%2fevil/integration/queue",
        None,
    )
    .await;
    assert_eq!(resp.status(), 400, "穿越 project 应 400");

    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/%2e%2e%2fevil/integration/queue",
        Some(json!({"task_id": "t", "frozen_sha": "abc", "base_rev": "def"})),
    )
    .await;
    assert_eq!(resp.status(), 400, "入队同样要拒");
    // 不得在 plans_root 下留下穿越目录
    assert!(!f.state.plans_root.join("evil").exists(), "穿越目录不得被创建");
}
