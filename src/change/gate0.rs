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
    // 自审 minor（67）：旧口径只看 `repo/.git` 是否存在——在已有 git 仓的**子目录**里
    // 会静默 `git init` 建出嵌套仓，并把该子目录下全部内容提交进去（改掉用户的 VCS 拓扑）。
    if let Some(outer) = enclosing_repo(repo) {
        return Err(DevforgeError::Config(format!(
            "目标仓 {} 位于另一个 git 仓 {} 之内——拒绝建嵌套仓（请在仓库根发起变更）",
            repo.display(),
            outer.display()
        )));
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
    // 自审 minor（68）：整份 `add -A` 会把 .env/密钥类文件固化进新 git 历史。不阻断
    // （会打断正常流程），但必须在提交前大声告警并列出命中文件。
    if let Ok(staged) = run_git_capture(repo, &["diff", "--cached", "--name-only"]) {
        let hits: Vec<&str> = staged
            .lines()
            .map(str::trim)
            .filter(|l| looks_like_secret(l))
            .collect();
        if !hits.is_empty() {
            tracing::warn!(
                component = "change-gate0",
                files = ?hits,
                "基线提交包含疑似密钥/环境文件——确认无误后继续（这些内容会进入 git 历史）"
            );
        }
    }
    // 固定身份，避免依赖机器上的全局 git 配置（CI / 新机也能跑）
    // 自审 minor（69）：加 `--no-verify`——目标仓或全局 core.hooksPath 的 pre-commit/
    // commit-msg 钩子会在引擎的基线提交上执行（可能改内容/阻塞/带副作用）。
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=devforge",
            "-c",
            "user.email=devforge@localhost",
            "commit",
            "-q",
            "--allow-empty",
            "--no-verify",
            "-m",
            "devforge: baseline",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| {
            DevforgeError::System(format!("执行 git commit 失败（仓 {}）：{e}", repo.display()))
        })?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "基线提交失败（仓 {}）：{}",
            repo.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    head_sha(repo)?
        .ok_or_else(|| DevforgeError::System(format!("基线提交后仍取不到 HEAD（仓 {}）", repo.display())))
}

/// 向上探测：`repo` 的**任一祖先**（不含自身）如果是 git 仓，返回那个仓根。
fn enclosing_repo(repo: &Path) -> Option<std::path::PathBuf> {
    let mut cur = repo.canonicalize().ok()?;
    loop {
        let parent = cur.parent()?.to_path_buf();
        if parent.join(".git").exists() {
            return Some(parent);
        }
        cur = parent;
    }
}

/// 疑似密钥/环境文件（自审 minor 68 的告警名单）。
fn looks_like_secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.contains("id_rsa")
        || name.contains("credentials")
        || name.contains("secrets")
        || name.ends_with(".keystore")
}

/// 取命令 stdout（失败返回 Err，调用方按需忽略）。
fn run_git_capture(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git {args:?} 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git {args:?} 失败（仓 {}）：{}",
            repo.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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
        .map_err(|e| {
            DevforgeError::System(format!("执行 git {args:?} 失败（仓 {}）：{e}", repo.display()))
        })?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git {args:?} 失败（仓 {}）：{}",
            repo.display(),
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
#[path = "gate0_tests.rs"]
mod tests;
