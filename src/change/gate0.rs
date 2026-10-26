//! 门 0（P009）：基线、文档档位、change-id 的确定性部分。
//!
//! 口径：
//! - **基线（D001）**：目标仓无 `.git` → `git init` + 全量提交（提交信息固定前缀
//!   `devforge: baseline`），得到可 diff / 可回滚的锚点；已有 `.git` → 只记 HEAD，**不动历史**。
//! - **doc_level（D002）**：`none`（默认）/ `essential` / `complete`——文档完整度是调用方的决定，
//!   引擎不自动探测目录里有没有文档。
//! - **change-id（D008）**：`change-<date>-<slug>`，同一任务重复触发必须幂等。

use std::path::Path;
use std::process::Command;

use crate::error::{DevforgeError, Result};

/// 基线锚点：`sha` 是 diff/回滚的对照点，`created_repo` 标记是不是本次新建的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    pub sha: String,
    pub created_repo: bool,
}

/// 文档档位：全仓唯一定义在 [`crate::docgen::audit::DocLevel`]（全仓审计 #4 去重）——
/// 旧实现是本文件另立一份同名不同值域的枚举（None/Essential/Complete，无 Detailed），
/// 跨模块传递无编译期区分。现在这里只做重导出，保持 `change::gate0::DocLevel` 路径可用；
/// change SOP 的配置面解析用 [`DocLevel::parse_strict`]（未知值 = Config 错误）。
pub use crate::docgen::audit::DocLevel;

/// 确保基线存在，并把 sha 写到 `out_dir/baseline.txt`。
///
/// - 已有 `.git`：取 `HEAD`；HEAD 不可解析（空仓）时按「新建基线」处理。
/// - 无 `.git`：`git init` → `add -A` → 提交 `devforge: baseline`。
/// - **`baseline.txt` 已存在且非空 → 直接复用记录的 sha（基线 = 变更起点的不可变锚点）**：
///   同一 change 续跑时 HEAD 已前进（按步提交），重记会把基线顶成当前 HEAD，
///   导致按步提交的门禁看不到已提交的步（t26 崩溃恢复实测的自锁路径）。
pub fn ensure_baseline(repo: &Path, out_dir: &Path) -> Result<Baseline> {
    if !repo.is_dir() {
        return Err(DevforgeError::Config(format!(
            "目标仓不存在：{}",
            repo.display()
        )));
    }
    let record = out_dir.join(BASELINE_FILE);
    match std::fs::read_to_string(&record) {
        Ok(text) => {
            let sha = text.trim().to_string();
            if !sha.is_empty() {
                // 自审 major：锚点内容先过形状门（commit id = 7-64 位 hex）——
                // 旧口径原样当 diff 锚点，垃圾内容潜伏到远处门禁层才炸。
                let well_formed = sha.len() >= 7
                    && sha.len() <= 64
                    && sha.chars().all(|c| c.is_ascii_hexdigit());
                if !well_formed {
                    return Err(DevforgeError::System(format!(
                        "基线锚点 {} 内容不是合法 commit id（{sha:?}）——不静默重锚（t26 自锁防线）；人工修复或删除该文件",
                        record.display()
                    )));
                }
                // 锚点还活着吗：cat-file 快验存在性——死锚点（被 GC/rebase 吹掉）
                // 现在就炸，不留给远处门禁；也不静默重锚（重记=打开自锁路径）。
                let out = Command::new("git")
                    .args(["cat-file", "-e", &format!("{sha}^{{commit}}")])
                    .current_dir(repo)
                    .output()
                    .map_err(|e| DevforgeError::System(format!("执行 git cat-file 失败：{e}")))?;
                if !out.status.success() {
                    return Err(DevforgeError::System(format!(
                        "基线锚点 {sha} 在目标仓已不存在（被 GC/rebase 吹掉）——不静默重锚；确认后删除 {} 可重建基线",
                        record.display()
                    )));
                }
                tracing::info!(
                    component = "change-gate0",
                    event = "baseline.reused",
                    sha = %sha,
                    "复用已记录的基线锚点"
                );
                return Ok(Baseline {
                    sha,
                    created_repo: false,
                });
            }
            // 空文件：原子写后 crash 不会再产生；多半是人为残留——显式留痕后重记。
            tracing::warn!(
                component = "change-gate0",
                path = %record.display(),
                "基线锚点文件为空，重记基线"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // 自审 major：读失败（权限等）不再被当「无记录」静默重锚——
        // 基线是自锁防线，读不动就该炸，不能悄悄换锚。
        Err(e) => {
            return Err(DevforgeError::System(format!(
                "读基线锚点 {} 失败：{e}——不静默重锚，先修复文件权限",
                record.display()
            )));
        }
    }
    let git_dir = repo.join(".git");
    if git_dir.exists() {
        match head_sha(repo)? {
            Some(sha) => {
                write_baseline(out_dir, &sha)?;
                tracing::info!(
                    component = "change-gate0",
                    event = "baseline.recorded",
                    sha = %sha,
                    "记录当前 HEAD 为基线"
                );
                return Ok(Baseline {
                    sha,
                    created_repo: false,
                });
            }
            None => {
                // 有 .git 但没有提交：补一个基线提交（否则无法 diff）
                let sha = create_baseline_commit(repo)?;
                write_baseline(out_dir, &sha)?;
                tracing::info!(
                    component = "change-gate0",
                    event = "baseline.created",
                    sha = %sha,
                    "空仓：新建基线提交"
                );
                return Ok(Baseline {
                    sha,
                    created_repo: false,
                });
            }
        }
    }

    run_git(repo, &["init", "-q"])?;
    let sha = create_baseline_commit(repo)?;
    write_baseline(out_dir, &sha)?;
    tracing::info!(
        component = "change-gate0",
        event = "baseline.repo_initialized",
        sha = %sha,
        "无 .git：git init 并新建基线提交"
    );
    Ok(Baseline {
        sha,
        created_repo: true,
    })
}

fn create_baseline_commit(repo: &Path) -> Result<String> {
    run_git(repo, &["add", "-A"])?;
    // 固定身份，避免依赖机器上的全局 git 配置（CI / 新机也能跑）
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=devforge",
            "-c",
            "user.email=devforge@localhost",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "devforge: baseline",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git commit 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "基线提交失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    head_sha(repo)?.ok_or_else(|| DevforgeError::System("基线提交后仍取不到 HEAD".into()))
}

/// 取 HEAD sha。Ok(None) = 空仓（unborn HEAD，有资格走补基线提交路径）；
/// 其它 git 失败 = 基础设施错误（自审 major：旧口径用 Option 抹掉失败原因，
/// 非 unborn 的 git 故障被静默当「空仓」处理，还顺手建了基线提交）。
fn head_sha(repo: &Path) -> Result<Option<String>> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git rev-parse 失败：{e}")))?;
    if out.status.success() {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return Ok((!sha.is_empty()).then_some(sha));
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("unknown revision")
        || stderr.contains("ambiguous argument")
        || stderr.contains("Failed to resolve")
    {
        return Ok(None);
    }
    Err(DevforgeError::System(format!(
        "git rev-parse HEAD 失败：{}",
        stderr.trim()
    )))
}

fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git {args:?} 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// 基线锚点文件名（相对计划目录）。
const BASELINE_FILE: &str = "baseline.txt";

fn write_baseline(out_dir: &Path, sha: &str) -> Result<()> {
    std::fs::create_dir_all(out_dir).map_err(|e| {
        DevforgeError::System(format!("建目录 {} 失败：{e}", out_dir.display()))
    })?;
    let path = out_dir.join(BASELINE_FILE);
    // 自审 major：原子写（唯一临时名 + rename）——truncate+write 崩溃留下空文件，
    // 下次读空走「无记录」重记，恰好打开本模块声明要防的 t26 自锁路径。
    let tmp = out_dir.join(format!("{BASELINE_FILE}.{}.tmp", std::process::id()));
    let body = format!("{sha}\n");
    (|| -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| DevforgeError::System(format!("建临时文件失败：{e}")))?;
        f.write_all(body.as_bytes())
            .map_err(|e| DevforgeError::System(format!("写临时文件失败：{e}")))?;
        f.sync_all()
            .map_err(|e| DevforgeError::System(format!("刷临时文件失败：{e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| DevforgeError::System(format!("原子替换 {} 失败：{e}", path.display())))?;
        Ok(())
    })()
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
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
}
