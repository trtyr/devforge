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

/// 自审 critical 回归：worktree 隔离下主仓 HEAD 纹丝不动（agent 的改动全部在
/// task/{id} 分支上，worktree 收尾即删）——终校验必须对候选分支求 diff，否则
/// 改动集恒为空集、越界 run 判 boundary.ok（fail-open 假绿）。
#[tokio::test]
async fn worktree_isolation_boundary_checks_the_candidate_branch_not_head() {
    let (g, base) = git_repo_with_base();
    // 模拟 worktree 流程产物：候选越界改动落在 task/t 分支上，主仓 HEAD 停在 base
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(g.path())
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
    let wt = tempfile::tempdir().unwrap();
    git(&["branch", "task/t"]);
    git(&[
        "worktree",
        "add",
        wt.path().to_str().unwrap(),
        "task/t",
    ]);
    std::fs::write(wt.path().join("evil.txt"), "out of contract\n").unwrap();
    let wgit = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(wt.path())
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
    wgit(&["add", "-A"]);
    wgit(&["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "candidate"]);
    let _ = std::fs::remove_dir_all(wt.path());

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
                "isolation": "worktree",
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
        body["runs"][0]["status"], "Failed",
        "分支上的越界改动必须被抓到（对 HEAD 求 diff 是盲区）：{body}"
    );
    let replayed = f.state.bus.replay_after(0);
    assert!(
        replayed.iter().any(|(_, e)| e.contains("boundary.violation")),
        "必须有 boundary.violation 事件：{:?}",
        replayed.iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>()
    );
}

/// 自审 critical 配套：worktree 隔离但候选分支不存在（收尾异常）→ 系统错误改判
/// Failed，不能 fail-open 成 boundary.ok。
#[tokio::test]
async fn worktree_isolation_without_candidate_branch_fails_closed() {
    let (g, base) = git_repo_with_base();
    // 不建 task/t 分支：主仓 HEAD == base，旧实现对 HEAD 求 diff = 空集 = 假绿
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
                "isolation": "worktree",
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
        body["runs"][0]["status"], "Failed",
        "找不到候选分支必须拒放行（门不能 fail-open）：{body}"
    );
    let replayed = f.state.bus.replay_after(0);
    assert!(
        !replayed.iter().any(|(_, e)| e.contains("boundary.ok")),
        "分支缺失不得发 boundary.ok（fail-open 假绿）：{:?}",
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

#[tokio::test]
async fn forbidden_paths_fail_even_inside_owned() {
    // P012 挂账补全：forbidden_paths 绝对禁令——文件在 owned（src/**）内、
    // 但命中 forbidden（src/secret.rs）→ 必 FAIL（禁改优先于覆盖）
    let (g, base) = git_repo_with_base();
    std::fs::create_dir_all(g.path().join("src")).unwrap();
    std::fs::write(g.path().join("src/secret.rs"), "touched forbidden\n").unwrap();
    commit_all(&g, "touch-forbidden");

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
                "forbidden_paths": ["src/secret.rs"],
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
        body["runs"][0]["status"], "Failed",
        "禁改命中即使在 owned 内也必须 FAIL：{body}"
    );
    let replayed = f.state.bus.replay_after(0);
    let hit = replayed
        .iter()
        .find(|(_, e)| e.contains("boundary.violation") && e.contains("forbidden"))
        .expect("应有 forbidden 性质的 violation 事件");
    assert!(hit.1.contains("src/secret.rs"), "事件应指认禁改文件：{}", hit.1);
}
