//! P012 M0 · T0.2：契约边界终校验（引擎层越界拦截）。

#[allow(unused_imports)]
use super::*;

/// git 仓夹具：tempdir + init + 基线 commit；返回（目录，基线 sha）。
fn git_repo_with_base() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base"]);
    let base = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    (dir, base)
}

fn commit_all(dir: &tempfile::TempDir, msg: &str) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["add", "-A"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", msg])
        .output()
        .unwrap();
    assert!(out.status.success());
}

#[tokio::test]
async fn boundary_violation_fails_an_otherwise_successful_run() {
    // 错实现形态：终校验缺失 → MockEngine 成功 + 预埋越界 commit → run 判 Completed（假绿）
    let (g, base) = git_repo_with_base();
    // 越界改动：base 之后 commit 一个契约外文件（MockEngine 不动工作区，预埋即 diff 差异）
    std::fs::write(g.path().join("evil.txt"), "out of contract\n").unwrap();
    commit_all(&g, "violation");

    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": g.path().to_str().unwrap()})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({
            "id": "t",
            "sop": "doc-from-zero",
            "contract": {
                "owned_paths": ["src/**"],
                "forbidden_paths": [],
                "base_rev": base,
                "isolation": "none",
            },
        })),
    )
    .await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    wait_settled(&f, "p", "t").await;

    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(body["runs"][0]["status"], "Failed", "越界 run 必须改判 Failed");
    // 事件可查：EventBus 回放缓冲（state.emit 的落点）里有 boundary.violation
    let replayed = f.state.bus.replay_after(0);
    assert!(
        replayed.iter().any(|(_, e)| e.contains("boundary.violation")),
        "必须有 boundary.violation 事件：{:?}",
        replayed.iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn project_registration_cleans_orphan_worktrees() {
    // P012 M1 · T1.3：重启后首个 POST projects 即接管清理钩子——预埋孤儿 → 注册即清
    let (g, base) = git_repo_with_base();
    // 预埋孤儿：创建 worktree 后手删目录（注册表记录还在）
    let spec = crate::workspace::WorkspaceSpec {
        repo: g.path().to_path_buf(),
        task_id: "orphan-task".into(),
        base_rev: base,
    };
    let ws = crate::workspace::Workspace::create(&spec).unwrap();
    std::fs::remove_dir_all(&ws.path).unwrap();
    assert_eq!(crate::workspace::scan_orphans(g.path()).unwrap().len(), 1);

    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": g.path().to_str().unwrap()})),
    )
    .await;
    // 注册即清：无孤儿残留 + 清理事件在总线
    assert!(crate::workspace::scan_orphans(g.path()).unwrap().is_empty());
    let replayed = f.state.bus.replay_after(0);
    assert!(
        replayed.iter().any(|(_, e)| e.contains("workspace.orphans_cleaned")),
        "应有清理事件：{:?}",
        replayed.iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn in_contract_changes_pass_the_boundary_check() {
    // 对照：改动全部落在 owned_paths 内 → run 正常 Completed（boundary.ok）
    let (g, base) = git_repo_with_base();
    // 合规改动：base 之后 commit 一个契约内文件
    std::fs::create_dir_all(g.path().join("src")).unwrap();
    std::fs::write(g.path().join("src/good.rs"), "fn good() {}\n").unwrap();
    commit_all(&g, "in-contract");

    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": g.path().to_str().unwrap()})),
    )
    .await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({
            "id": "t",
            "sop": "doc-from-zero",
            "contract": {
                "owned_paths": ["src/**"],
                "forbidden_paths": [],
                "base_rev": base,
                "isolation": "none",
            },
        })),
    )
    .await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    wait_settled(&f, "p", "t").await;

    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(
        body["runs"][0]["status"], "Settled",
        "契约内改动不得拦截：{body}"
    );
}
