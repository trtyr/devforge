//! P012 M0 · T0.2：契约边界终校验（引擎层越界拦截）。

#[allow(unused_imports)]
use super::*;

/// git 仓夹具（可指定基线文件集合）：tempdir + init + 基线 commit；返回（目录，基线 sha）。
///
/// 既有 `git_repo_with_base()` 是本函数的薄包装（基线仍只含 `a.rs`），签名与行为不变。
/// 「已跟踪 `.pi/**` 仍判死」的 ` M` 形态需要基线提交里已含 `.pi/goals/keep.md`。
///
/// 自审 minor（50）：隔离宿主 git 配置——旧口径跑开发机/CI 的全局设置（core.hooksPath /
/// commit.gpgsign / init.defaultBranch / template），失败时错误信息容易被误读成代码问题。
fn git_repo_with_base_files(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
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
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    git(&["add", "-A"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base"]);
    let base = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    (dir, base)
}

fn git_repo_with_base() -> (tempfile::TempDir, String) {
    git_repo_with_base_files(&[("a.rs", "fn a() {}\n")])
}

fn commit_all(dir: &tempfile::TempDir, msg: &str) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["add", "-A"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", msg])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
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
    assert_eq!(body["runs"][0]["status"], "failed", "越界 run 必须改判 Failed");
    // 事件可查：EventBus 回放缓冲（state.emit 的落点）里有 boundary.violation
    let replayed = f.state.bus.replay_after(0);
    assert!(
        replayed.iter().any(|(_, e)| e.contains("boundary.violation")),
        "必须有 boundary.violation 事件：{:?}",
        replayed.iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>()
    );
    // 自审 minor（239）：诊断力对齐 forbidden 分支——必须断言事件**指认了越界文件**，
    // 否则「判了 Failed 但说不出谁越界」的回归抓不到。
    let violation = replayed
        .iter()
        .map(|(_, e)| e.as_str())
        .find(|e| e.contains("boundary.violation"))
        .expect("boundary.violation 事件");
    assert!(
        violation.contains("evil.txt"),
        "boundary.violation 必须指认越界文件：{violation}"
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
        body["runs"][0]["status"], "failed",
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
        body["runs"][0]["status"], "failed",
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
        body["runs"][0]["status"], "settled",
        "契约内改动不得拦截：{body}"
    );
    // 自审 minor（240）：注释声称对照用例覆盖 boundary.ok，但旧口径全仓无任何地方断言它——
    // 「门过了」在事件流里必须看得见，否则 fail-open（不发点）与真过不可区分。
    let replayed = f.state.bus.replay_after(0);
    assert!(
        replayed.iter().any(|(_, e)| e.contains("boundary.ok")),
        "契约内改动必须落 boundary.ok 事件：{:?}",
        replayed.iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>()
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
        body["runs"][0]["status"], "failed",
        "禁改命中即使在 owned 内也必须 FAIL：{body}"
    );
    let replayed = f.state.bus.replay_after(0);
    let hit = replayed
        .iter()
        .find(|(_, e)| e.contains("boundary.violation") && e.contains("forbidden"))
        .expect("应有 forbidden 性质的 violation 事件");
    assert!(hit.1.contains("src/secret.rs"), "事件应指认禁改文件：{}", hit.1);
}

// ---------- 未跟踪的工具元数据（仓根 `.pi/**`）不进改动面：回归用例 ----------

/// 起一个带契约的 run 并等它进终态；返回 run 终态 status。
///
/// 新回归用例的公共骨架：登记项目 → 建带契约的任务 → start → 等终态 → 读终态。
/// `isolation` 传 "none" 或 "worktree"（对应契约的 isolation 枚举）。
async fn run_contract_task(
    f: &Fixture,
    repo: &std::path::Path,
    base: &str,
    isolation: &str,
) -> String {
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": repo.to_str().unwrap()})),
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
                "isolation": isolation,
            },
        })),
    )
    .await;
    send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    wait_settled(f, "p", "t").await;
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    body["runs"][0]["status"].as_str().unwrap_or("").to_string()
}

/// 事件流里有没有含 `marker` 的事件。
fn has_event(f: &Fixture, marker: &str) -> bool {
    f.state
        .bus
        .replay_after(0)
        .iter()
        .any(|(_, e)| e.contains(marker))
}

/// 取事件流里第一条含 `marker` 的事件并解析成 JSON（找不到 → None）。
fn find_event(f: &Fixture, marker: &str) -> Option<serde_json::Value> {
    f.state
        .bus
        .replay_after(0)
        .iter()
        .map(|(_, e)| e.as_str())
        .find(|e| e.contains(marker))
        .and_then(|e| serde_json::from_str(e).ok())
}

/// 回归（SR1 run 9 实证缺陷）：未跟踪的工具元数据（仓根 `.pi/**`）不是产品改动——终校验
/// 放行，不判死。
///
/// 错实现形态：改动面收集不区分 porcelain 状态码（`??` 与已跟踪一视同仁）→ 未跟踪的
/// `.pi/goals/x.md` 进改动集 → 不在 owned_paths（`src/**`）内 → run 判 failed +
/// boundary.violation 指认 `.pi/goals/x.md`（SR1 run 9 的「29 个文件超出契约 owned_paths」
/// 误杀同款）。
#[tokio::test]
async fn untracked_pi_tool_metadata_is_not_a_boundary_violation() {
    let (g, base) = git_repo_with_base();
    // pi 在自己 cwd 写的状态目录（未跟踪形态）：工具元数据，不是产品输出
    std::fs::create_dir_all(g.path().join(".pi/goals")).unwrap();
    std::fs::write(g.path().join(".pi/goals/x.md"), "# goal\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "settled",
        "未跟踪的工具元数据不得判死（SR1 run 9 误杀）：{status}"
    );
    assert!(
        !has_event(&f, "boundary.violation"),
        "不得有 boundary.violation 事件"
    );
    // 被忽略的工具元数据不计入改动面条数（boundary.ok.files 是对外可观测的改动面计数）
    let ok = find_event(&f, "boundary.ok").expect("必须落 boundary.ok 事件");
    assert_eq!(
        ok["files"].as_u64(),
        Some(0),
        "只含工具元数据的 run，改动面条数应为 0：{ok}"
    );
}

/// 回归（worktree 隔离形态）：主仓工作区里的未跟踪工具元数据同样不进改动面。
///
/// 坑（计划 §6 风险「测试假绿陷阱」）：终校验 stat 的是 `task.workspace`（主仓），不是
/// worktree——worktree 在引擎收尾时已被删除。把 `.pi` 放进 worktree 目录的用例在修复前
/// 也是绿的（假绿），所以 `.pi` 必须落在**主仓**。
///
/// 错实现形态：豁免只加在 isolation=none 分支 / 只作用于已提交集合 → worktree 形态的
/// 未跟踪 `.pi` 仍进改动集 → run 判 failed（boundary.violation 指认 `.pi/goals/x.md`）。
#[tokio::test]
async fn worktree_isolation_boundary_check_ignores_untracked_pi_tool_metadata() {
    let (g, base) = git_repo_with_base();
    // 候选分支 task/t：只有契约内提交（src/good.rs）→ 已提交集合侧无越界
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
    git(&["worktree", "add", wt.path().to_str().unwrap(), "task/t"]);
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
    std::fs::create_dir_all(wt.path().join("src")).unwrap();
    std::fs::write(wt.path().join("src/good.rs"), "fn good() {}\n").unwrap();
    wgit(&["add", "-A"]);
    wgit(&["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "candidate"]);
    let _ = std::fs::remove_dir_all(wt.path());
    // 主仓工作区：pi 的未跟踪工具元数据（终校验 stat 的就是这个目录）
    std::fs::create_dir_all(g.path().join(".pi/goals")).unwrap();
    std::fs::write(g.path().join(".pi/goals/x.md"), "# goal\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "worktree").await;
    assert_eq!(
        status, "settled",
        "worktree 形态下未跟踪的工具元数据不得判死：{status}"
    );
    assert!(
        has_event(&f, "boundary.ok"),
        "worktree 形态同样要落 boundary.ok（豁免在 status 侧生效）"
    );
    assert!(
        !has_event(&f, "boundary.violation"),
        "不得有 boundary.violation 事件"
    );
}

/// 回归：豁免是**按条目**的，不是「有 `.pi` 就整批跳过」——同一次 run 里既有未跟踪的
/// `.pi/goals/x.md` 又有未跟踪的 `evil.txt` 时，必须判 failed，且 violation 清单**只含**
/// `evil.txt`。
///
/// 错实现形态：豁免写成「改动集里出现 `.pi/` 就整批放行」或「未跟踪条目一律不算改动」
/// → `evil.txt` 一并被放过 → run 判 settled（假绿）。
#[tokio::test]
async fn untracked_pi_tool_metadata_does_not_mask_other_violations() {
    let (g, base) = git_repo_with_base();
    std::fs::create_dir_all(g.path().join(".pi/goals")).unwrap();
    std::fs::write(g.path().join(".pi/goals/x.md"), "# goal\n").unwrap();
    std::fs::write(g.path().join("evil.txt"), "out of contract\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "failed",
        "契约外的未跟踪文件仍必须判死（拦截语义不退化）：{status}"
    );
    let violation = find_event(&f, "boundary.violation").expect("必须有 boundary.violation 事件");
    let listed: Vec<&str> = violation["violations"]
        .as_array()
        .expect("violations 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        listed,
        vec!["evil.txt"],
        "violation 清单必须只含 evil.txt（豁免按条目，不按批次）：{violation}"
    );
}

/// 回归（语义边界）：已跟踪的 `.pi/**` 是**显式提交行为**，仍照常受契约判定——豁免只针对
/// 未跟踪形态（porcelain 状态码 `??`）。两种形态各钉一次：
/// ① 提交形态：`.pi/goals/x.md` 被 git add + commit → 进已提交集合 → 越界；
/// ② ` M` 形态：基线提交里已含 `.pi/goals/keep.md`，工作区把它改成未提交的修改 → 越界。
///
/// 错实现形态：豁免只看路径不看状态码（`path.starts_with(".pi/")` 就放过）→ ①② 都判
/// settled（假绿，语义边界被抹平）。
#[tokio::test]
async fn tracked_pi_tool_metadata_is_still_judged_by_the_contract() {
    // ① 提交形态
    let (g, base) = git_repo_with_base();
    std::fs::create_dir_all(g.path().join(".pi/goals")).unwrap();
    std::fs::write(g.path().join(".pi/goals/x.md"), "# goal\n").unwrap();
    commit_all(&g, "commit-pi-metadata");

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "failed",
        "已提交的 .pi/** 是显式提交行为，必须判死：{status}"
    );
    let violation = find_event(&f, "boundary.violation").expect("必须有 boundary.violation 事件");
    let listed: Vec<&str> = violation["violations"]
        .as_array()
        .expect("violations 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        listed,
        vec![".pi/goals/x.md"],
        "violation 清单必须指认已提交的 .pi 文件：{violation}"
    );

    // ② ` M` 形态：基线提交里已含 .pi/goals/keep.md，工作区改成未提交的修改
    let (g2, base2) = git_repo_with_base_files(&[
        ("a.rs", "fn a() {}\n"),
        (".pi/goals/keep.md", "keep\n"),
    ]);
    std::fs::write(g2.path().join(".pi/goals/keep.md"), "changed\n").unwrap();

    let f2 = fixture();
    let status2 = run_contract_task(&f2, g2.path(), &base2, "none").await;
    assert_eq!(
        status2, "failed",
        "已跟踪 .pi 文件的未提交修改必须判死：{status2}"
    );
    let violation2 = find_event(&f2, "boundary.violation").expect("必须有 boundary.violation 事件");
    let listed2: Vec<&str> = violation2["violations"]
        .as_array()
        .expect("violations 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        listed2,
        vec![".pi/goals/keep.md"],
        "violation 清单必须指认被修改的已跟踪 .pi 文件：{violation2}"
    );
}

/// 回归（F12 通道 2）：终校验的未跟踪豁免只认仓根 `.pi/**`，没认预算熔断台账——
/// `.gitignore` 没生效的旧仓/浅克隆里，台账挂在 `git status` 里 → 进改动面 → 越界判死
/// （真症状：四阶段全过、死在 done 门一个 0 字节锁文件）。
///
/// 错实现形态：豁免条件不含台账名（只有 `path.starts_with(".pi/")`）→ run 判 failed +
/// boundary.violation 指认 `spawn-ledger.json` → 本用例必红。
#[tokio::test]
async fn untracked_spawn_ledger_is_not_a_boundary_violation() {
    let (g, base) = git_repo_with_base();
    // 未跟踪形态的仓根台账（主名 + 锁文件）：工具元数据，不是产品输出
    std::fs::write(g.path().join("spawn-ledger.json"), "{}\n").unwrap();
    std::fs::write(g.path().join("spawn-ledger.json.lock"), "\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "settled",
        "未跟踪的仓根台账不得判死（旧仓/浅克隆场景）：{status}"
    );
    assert!(
        !has_event(&f, "boundary.violation"),
        "不得有 boundary.violation 事件"
    );
    // 被豁免的工具元数据不计入改动面条数（boundary.ok.files 是对外可观测的改动面计数）
    let ok = find_event(&f, "boundary.ok").expect("必须落 boundary.ok 事件");
    assert_eq!(
        ok["files"].as_u64(),
        Some(0),
        "只含台账的 run，改动面条数应为 0：{ok}"
    );
}

/// 回归（F12 通道 2 · 按条目豁免）：台账豁免不得变成「有台账就整批放行」——
/// 同一次 run 里既有未跟踪台账又有未跟踪 `evil.txt` 时，必须判 failed，且 violation
/// 清单**只含** `evil.txt`。
///
/// 错实现形态：豁免写成「改动集里出现台账名就整批跳过」或「未跟踪条目一律不算改动」
/// → `evil.txt` 一并被放过 → run 判 settled（假绿）→ 本用例必红。
#[tokio::test]
async fn untracked_spawn_ledger_does_not_mask_other_violations() {
    let (g, base) = git_repo_with_base();
    std::fs::write(g.path().join("spawn-ledger.json"), "{}\n").unwrap();
    std::fs::write(g.path().join("spawn-ledger.json.lock"), "\n").unwrap();
    std::fs::write(g.path().join("evil.txt"), "out of contract\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "failed",
        "契约外的未跟踪文件仍必须判死（拦截语义不退化）：{status}"
    );
    let violation = find_event(&f, "boundary.violation").expect("必须有 boundary.violation 事件");
    let listed: Vec<&str> = violation["violations"]
        .as_array()
        .expect("violations 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        listed,
        vec!["evil.txt"],
        "violation 清单必须只含 evil.txt（豁免按条目，不按批次）：{violation}"
    );
}

/// 回归（F12 通道 2 · 语义边界）：已跟踪的台账是**显式提交行为**，仍照常受契约判定——
/// 豁免只针对未跟踪形态（porcelain 状态码 `??`）。基线提交里已含 `spawn-ledger.json`，
/// 工作区把它改成未提交的修改（` M` 形态）→ 仍判 failed。
///
/// 错实现形态：豁免只看路径不看状态码（`path == "spawn-ledger.json"` 就放过）→ 判 settled
/// （假绿，语义边界被抹平）→ 本用例必红。
#[tokio::test]
async fn tracked_spawn_ledger_is_still_judged_by_the_contract() {
    let (g, base) = git_repo_with_base_files(&[
        ("a.rs", "fn a() {}\n"),
        ("spawn-ledger.json", "{}\n"),
    ]);
    std::fs::write(g.path().join("spawn-ledger.json"), "{\"changed\":1}\n").unwrap();

    let f = fixture();
    let status = run_contract_task(&f, g.path(), &base, "none").await;
    assert_eq!(
        status, "failed",
        "已跟踪台账的未提交修改必须判死：{status}"
    );
    let violation = find_event(&f, "boundary.violation").expect("必须有 boundary.violation 事件");
    let listed: Vec<&str> = violation["violations"]
        .as_array()
        .expect("violations 必须是数组")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        listed,
        vec!["spawn-ledger.json"],
        "violation 清单必须指认被修改的已跟踪台账：{violation}"
    );
}
