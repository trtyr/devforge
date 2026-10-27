//! （2026-10-26 测试分离：自 src/change/gate0.rs 内联平移，纯移动零改动。）
use super::*;

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit_all(repo: &Path, msg: &str) {
    git(repo, &["add", "-A"]);
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            msg,
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn existing_baseline_record_is_reused_even_after_head_advances() {
    // t26 崩溃恢复实测：coding 中途带提交续跑，HEAD 已前进；
    // 基线 = 变更起点的不可变锚点，重记会把已提交的步从门禁视野里抹掉（自锁）
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let out = tmp.path().join("changes/c1");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "a\n").unwrap();

    let first = ensure_baseline(&repo, &out).unwrap();
    assert_eq!(first.sha, git(&repo, &["rev-parse", "HEAD"]));

    // 模拟 coding agent 的按步提交：HEAD 前进
    std::fs::write(repo.join("src/a.rs"), "a\nb\n").unwrap();
    commit_all(&repo, "change: c step 1");
    assert_ne!(first.sha, git(&repo, &["rev-parse", "HEAD"]));

    let second = ensure_baseline(&repo, &out).unwrap();
    assert_eq!(
        second.sha, first.sha,
        "续跑必须复用已记录的基线，不得重记当前 HEAD"
    );
    assert_eq!(
        std::fs::read_to_string(out.join("baseline.txt"))
            .unwrap()
            .trim(),
        first.sha
    );
}

#[test]
fn bare_dir_gets_init_plus_single_baseline_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/main.py"), "print(1)\n").unwrap();
    let out = tmp.path().join("changes/c1");

    let b = ensure_baseline(&repo, &out).unwrap();
    assert!(b.created_repo, "无 .git 时必须新建仓库");

    // 有且仅有一条提交，且是基线提交
    let log = git(&repo, &["log", "--oneline"]);
    assert_eq!(log.lines().count(), 1, "基线应只有一条提交：{log}");
    assert!(log.contains("devforge: baseline"), "{log}");

    // baseline.txt 等于该 sha
    let recorded = std::fs::read_to_string(out.join("baseline.txt")).unwrap();
    assert_eq!(recorded.trim(), b.sha);
    assert_eq!(b.sha, git(&repo, &["rev-parse", "HEAD"]));
}

/// 自审 major 回归：垃圾锚点（非 hex）不静默重锚也不当 diff 锚点，直接报错。
#[test]
fn corrupt_baseline_record_is_rejected_not_reanchored() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "a\n").unwrap();
    ensure_baseline(&repo, &tmp.path().join("changes/c1")).unwrap();
    // 头文件写坏
    std::fs::write(
        tmp.path().join("changes/c1/baseline.txt"),
        "not-a-sha-at-all!!\n",
    )
    .unwrap();
    let err = ensure_baseline(&repo, &tmp.path().join("changes/c1")).unwrap_err();
    assert!(err.to_string().contains("不是合法 commit id"), "{err}");
}

/// 自审 major 回归：死锚点（sha 在仓里已不存在）当场报错，不留给远处门禁，
/// 也不静默重锚。
#[test]
fn dangling_baseline_anchor_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "a\n").unwrap();
    let out = tmp.path().join("changes/c1");
    ensure_baseline(&repo, &out).unwrap();
    // 形状合法但对象不存在（40 个 hex 的 0）
    std::fs::write(out.join("baseline.txt"), format!("{}\n", "0".repeat(40))).unwrap();
    let err = ensure_baseline(&repo, &out).unwrap_err();
    assert!(err.to_string().contains("已不存在"), "{err}");
}

/// 自审 major 回归：锚点文件存在但不可读，不得当「无记录」静默重锚。
#[test]
fn unreadable_baseline_record_is_an_error_not_a_reanchor() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "a\n").unwrap();
    let out = tmp.path().join("changes/c1");
    let first = ensure_baseline(&repo, &out).unwrap();
    let record = out.join("baseline.txt");
    let mut perm = std::fs::metadata(&record).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o000);
    std::fs::set_permissions(&record, perm).unwrap();
    let err = ensure_baseline(&repo, &out).unwrap_err();
    assert!(err.to_string().contains("读基线锚点"), "{err}");
    // 恢复权限清理临时目录
    let mut perm = std::fs::metadata(&record).unwrap().permissions();
    perm.set_mode(0o644);
    std::fs::set_permissions(&record, perm).unwrap();
    let _ = first;
}

#[test]
fn existing_repo_only_records_head_and_never_moves_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q"]).unwrap();
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    commit_all(&repo, "existing work");
    let head_before = git(&repo, &["rev-parse", "HEAD"]);
    let count_before = git(&repo, &["rev-list", "--count", "HEAD"]);

    let b = ensure_baseline(&repo, &tmp.path().join("out")).unwrap();
    assert!(!b.created_repo, "已有 .git 不得重建仓库");
    assert_eq!(b.sha, head_before, "基线必须是当时的 HEAD");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), count_before, "不得新增提交");
}

#[test]
fn repo_with_git_but_no_commit_gets_baseline_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q"]).unwrap();
    std::fs::write(repo.join("x.txt"), "x\n").unwrap();

    let b = ensure_baseline(&repo, &tmp.path().join("out")).unwrap();
    assert!(!b.created_repo, "已有 .git（只是空仓）不算新建");
    assert_eq!(git(&repo, &["log", "--oneline"]).lines().count(), 1);
    assert_eq!(b.sha, git(&repo, &["rev-parse", "HEAD"]));
}

#[test]
fn missing_repo_is_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = ensure_baseline(&tmp.path().join("nope"), tmp.path()).unwrap_err();
    assert!(err.to_string().contains("不存在"), "{err}");
}

#[test]
fn doc_level_parses_three_tiers_and_defaults_to_none() {
    // 全仓审计 #4：change 侧用严格解析（未知值 = Config），档位定义已归一到
    // docgen::audit::DocLevel（包含 Detailed 与 None）
    assert_eq!(DocLevel::parse_strict("").unwrap(), DocLevel::None);
    assert_eq!(DocLevel::parse_strict("none").unwrap(), DocLevel::None);
    assert_eq!(DocLevel::parse_strict("Essential").unwrap(), DocLevel::Essential);
    assert_eq!(DocLevel::parse_strict("complete").unwrap(), DocLevel::Complete);
    assert_eq!(DocLevel::parse_strict("detailed").unwrap(), DocLevel::Detailed);
    assert!(DocLevel::parse_strict("full").is_err());
    assert!(!DocLevel::None.needs_docs());
    assert!(DocLevel::Essential.needs_docs() && DocLevel::Complete.needs_docs());
    // 档位走字符串往返不缺变体
    for lv in [DocLevel::None, DocLevel::Essential, DocLevel::Complete, DocLevel::Detailed] {
        assert_eq!(DocLevel::parse_strict(lv.as_str()).unwrap(), lv);
    }
}

// ── 2026-10-27 自审 gate0 批回归 ──

/// 自审 minor（67）：已有 git 仓的**子目录**里不许静默建嵌套仓（会改掉用户 VCS 拓扑）。
#[test]
fn nested_repo_inside_an_existing_repo_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    let inner = root.join("sub/dir");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("x.txt"), "x\n").unwrap();

    let out_dir = tempfile::tempdir().unwrap();
    let err = ensure_baseline(&inner, out_dir.path()).unwrap_err();
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "子目录必须报 Config（拒绝建嵌套仓）：{err}"
    );
    assert!(
        !inner.join(".git").exists(),
        "不得在子目录里留下 .git"
    );
}

/// 自审 minor（69）：基线提交必须带 --no-verify——目标仓的 pre-commit 钩子不得被执行。
#[cfg(unix)]
#[test]
fn baseline_commit_skips_repo_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    // 装一个必定失败的 pre-commit 钩子：带 --no-verify 才不会被执行
    let hooks = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&hook).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&hook, perms).unwrap();
    std::fs::write(root.join("a.txt"), "a\n").unwrap();

    let out_dir = tempfile::tempdir().unwrap();
    let base = ensure_baseline(root, out_dir.path()).expect("钩子不得阻断基线提交");
    assert!(!base.sha.is_empty());
}

/// 自审 minor（68）：疑似密钥文件进基线提交必须告警（不阻断），判定本身要覆盖常见形态。
#[test]
fn secret_like_paths_are_recognized() {
    for p in [
        "config/.env",
        ".env.local",
        "keys/server.pem",
        "certs/tls.key",
        "home/.ssh/id_rsa",
        "deploy/secrets.yaml",
    ] {
        assert!(looks_like_secret(p), "应识别为疑似密钥：{p}");
    }
    for p in ["src/main.rs", "README.md", "docs/env.md", "tests/fixtures/a.json"] {
        assert!(!looks_like_secret(p), "不该误报：{p}");
    }
}

/// 自审 minor（70）：baseline.txt 为**空文件**时的回退路径必须有用例
/// （旧口径这条分支零测试，正是 t26 自锁的那条路）。
#[test]
fn empty_baseline_record_falls_back_and_reanchors() {
    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    std::fs::write(out_dir.path().join("baseline.txt"), "   \n").unwrap();
    let base = ensure_baseline(tmp.path(), out_dir.path()).expect("空记录应重记基线");
    assert!(!base.sha.is_empty());
    let written = std::fs::read_to_string(out_dir.path().join("baseline.txt")).unwrap();
    assert_eq!(written.trim(), base.sha, "重记后文件必须是新 sha");
}

/// 自审 minor（70）：doc 档位是**四**档（None/Essential/Complete/Detailed），
/// 旧用例名说三档且测的是别处的函数。这里钉四档与 Detailed 分支。
#[test]
fn doc_level_has_four_tiers_including_detailed() {
    let reg = crate::docgen::audit::DocLevel::parse_strict("none").unwrap();
    assert_eq!(reg.as_str(), "none");
    let detailed = crate::docgen::audit::DocLevel::parse_strict("detailed").unwrap();
    assert_eq!(detailed.as_str(), "detailed");
    assert!(
        crate::docgen::audit::DocLevel::parse_strict("bogus").is_err(),
        "未知档位必须报错（strict）"
    );
}
