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

/// 文档档位（调用方给，不由引擎猜）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLevel {
    /// 不碰文档（默认）。
    None,
    /// 轻量：清单 + 入口 + Runbook 级覆盖。
    Essential,
    /// 完整：整条文档链（等价 doc-from-zero complete）。
    Complete,
}

impl DocLevel {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Ok(Self::None),
            "essential" => Ok(Self::Essential),
            "complete" => Ok(Self::Complete),
            other => Err(DevforgeError::Config(format!(
                "未知 doc_level `{other}`（只认 none/essential/complete）"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Essential => "essential",
            Self::Complete => "complete",
        }
    }

    /// 该档位是否需要先跑文档阶段。
    pub fn needs_docs(&self) -> bool {
        !matches!(self, Self::None)
    }
}

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
    if let Ok(text) = std::fs::read_to_string(&record) {
        let sha = text.trim().to_string();
        if !sha.is_empty() {
            return Ok(Baseline {
                sha,
                created_repo: false,
            });
        }
    }
    let git_dir = repo.join(".git");
    if git_dir.exists() {
        match head_sha(repo) {
            Some(sha) => {
                write_baseline(out_dir, &sha)?;
                return Ok(Baseline {
                    sha,
                    created_repo: false,
                });
            }
            None => {
                // 有 .git 但没有提交：补一个基线提交（否则无法 diff）
                let sha = create_baseline_commit(repo)?;
                write_baseline(out_dir, &sha)?;
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
    head_sha(repo)
        .ok_or_else(|| DevforgeError::System("基线提交后仍取不到 HEAD".into()))
}

fn head_sha(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
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
    std::fs::write(&path, format!("{sha}\n"))
        .map_err(|e| DevforgeError::System(format!("写 {} 失败：{e}", path.display())))?;
    Ok(())
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
        assert_eq!(DocLevel::parse("").unwrap(), DocLevel::None);
        assert_eq!(DocLevel::parse("none").unwrap(), DocLevel::None);
        assert_eq!(DocLevel::parse("Essential").unwrap(), DocLevel::Essential);
        assert_eq!(DocLevel::parse("complete").unwrap(), DocLevel::Complete);
        assert!(DocLevel::parse("full").is_err());
        assert!(!DocLevel::None.needs_docs());
        assert!(DocLevel::Essential.needs_docs() && DocLevel::Complete.needs_docs());
    }
}
