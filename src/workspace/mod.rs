//! worktree 工作区生命周期（P012 M1 · T1.1）。
//!
//! 设计文档：`docs/plantree/plans/012-parallel-dev-base/topics/02-workspace-lifecycle.md`。
//! 目标：mutation 任务在独立 git worktree 内全生命周期运行——main 工作树不再被直改，
//! 并行任务各占一个 worktree 互不踩踏（并发而不互踩的物理前提）。
//!
//! 路径约定：worktree 建在**仓外兄弟目录** `{repo_parent}/{repo_name}-worktrees/{task_id}`
//! （git worktree 的 .git 指针按绝对路径记录——仓内嵌套会让容器挂载语义复杂化，P012 研究
//! 期结论）。注册表落 `{repo}/.git/devforge-workspaces/{task_id}.json`——孤儿扫描的依据。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

/// 工作区规格：从哪个仓、给哪个任务、pin 在哪个基点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    /// 主仓路径（现有工作树，含 `.git`）。
    pub repo: PathBuf,
    /// 任务 id（分支名 `task/{id}` 的来源；项目内唯一）。
    pub task_id: String,
    /// 基点：`@{base_rev}` 的 rev（sha / 分支名 / `HEAD`）。
    pub base_rev: String,
}

/// 一个已创建的 worktree 工作区。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// worktree 绝对路径（spawn cwd 的来源）。
    pub path: PathBuf,
    /// 任务分支（`task/{task_id}`）。
    pub branch: String,
    /// 创建时的基点 rev（原样记录，resume 对齐用）。
    pub base_rev: String,
}

/// 注册表条目（落 `{repo}/.git/devforge-workspaces/{task_id}.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceRecord {
    task_id: String,
    path: PathBuf,
    branch: String,
    base_rev: String,
    created_at_ms: u64,
}

impl Workspace {
    /// 取得 worktree：不存在则创建（T1.1）；已存在则校验注册表记录后复用（T1.4 resume 对齐）。
    /// 复用时 base_rev 不一致 = 基点漂移 → 拒绝并提示清理重建。
    pub fn ensure(spec: &WorkspaceSpec) -> Result<Workspace> {
        let path = Self::target_path(spec);
        if path.exists() {
            let rec = record_read(&spec.repo, &spec.task_id)
                .ok_or_else(|| {
                    DevforgeError::Store(format!(
                        "worktree 目录已存在但无注册记录：{}（手动清理后重跑）",
                        path.display()
                    ))
                })?;
            if rec.base_rev != spec.base_rev {
                return Err(DevforgeError::Store(format!(
                    "worktree 基点漂移：记录 {} vs 契约 {}（清理 {} 后重跑）",
                    rec.base_rev,
                    spec.base_rev,
                    path.display()
                )));
            }
            return Ok(Workspace {
                path,
                branch: rec.branch,
                base_rev: rec.base_rev,
            });
        }
        Self::create(spec)
    }

    /// worktree 目标路径（仓外兄弟目录约定）。
    pub fn target_path(spec: &WorkspaceSpec) -> PathBuf {
        let repo_name = spec
            .repo
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        spec.repo
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{repo_name}-worktrees"))
            .join(&spec.task_id)
    }

    /// 自审 WA-01：task_id 路径安全校验（防御纵深——不能依赖 git refname 白名单
    /// 「恰好挡住」穿越；含分隔符的 task_id 还会造出无法注册的死 worktree）。
    /// allowlist：字母数字与 `._-`，禁 `..`；并在 target_path 内自断言前缀。
    fn validate_task_id(spec: &WorkspaceSpec) -> Result<()> {
        let id = &spec.task_id;
        let ok = !id.is_empty()
            && id != "."
            && id != ".."
            && !id.contains("..")
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !ok {
            return Err(DevforgeError::Store(format!(
                "任务 id `{id}` 含非法字符（只允许字母数字与 . _ -，且不得为 . / .. 或含 ..）"
            )));
        }
        let expected_prefix = spec
            .repo
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!(
                "{}-worktrees",
                spec.repo
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            ));
        let target = Self::target_path(spec);
        if !target.starts_with(&expected_prefix) {
            return Err(DevforgeError::Store(format!(
                "worktree 目标路径越界：{} 不在 {} 下",
                target.display(),
                expected_prefix.display()
            )));
        }
        Ok(())
    }

    /// 创建 worktree：`git worktree add -b task/{id} <path> <base_rev>`。
    ///
    /// worktree 审计（2026-10-25 自审 critical）：`remove()` **保留**任务分支（候选归因要它），
    /// 而旧实现无条件 `-b` 重建同名分支 → 同一 task 的第二次 run 必报
    /// "branch already exists"。现在分两种情形：
    /// - 分支不存在 → 照旧 `-b`；
    /// - 分支已存在且**仍停在基点**（上一轮没产生提交 / 崩溃回退）→ 直接挂载复用；
    /// - 分支已存在但已有提交（上一轮候选）→ 明确拒绝并告诉怎么清理，
    ///   不静默切换基点（否则本轮的 diff 归因会错位）。
    pub fn create(spec: &WorkspaceSpec) -> Result<Workspace> {
        Self::validate_task_id(spec)?;
        let path = Self::target_path(spec);
        if path.exists() {
            return Err(DevforgeError::Store(format!(
                "worktree 目标已存在：{}（先清孤儿或换任务 id）",
                path.display()
            )));
        }
        let branch = format!("task/{}", spec.task_id);
        let existing_tip = git_out(&spec.repo, &["rev-parse", "--verify", "--quiet", &branch])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let path_str = path.to_string_lossy().to_string();
        match existing_tip {
            Some(tip) if tip != spec.base_rev => {
                return Err(DevforgeError::Store(format!(
                    "任务分支 `{branch}` 已存在且指向 {tip}（上一轮候选保留，不等于本轮基点 {}）——
                     同一 task 重建工作区请先 `git branch -D {branch}`，或换 task id",
                    spec.base_rev
                )));
            }
            Some(_) => {
                // 分支仍在基点上 = 可安全复用（不重建分支，避免丢引用）
                git(&spec.repo, &["worktree", "add", &path_str, &branch])?;
            }
            None => {
                git(
                    &spec.repo,
                    &["worktree", "add", "-b", &branch, &path_str, &spec.base_rev],
                )?;
            }
        }
        let ws = Workspace {
            path,
            branch,
            base_rev: spec.base_rev.clone(),
        };
        record_write(&spec.repo, &WorkspaceRecord {
            task_id: spec.task_id.clone(),
            path: ws.path.clone(),
            branch: ws.branch.clone(),
            base_rev: ws.base_rev.clone(),
            created_at_ms: crate::domain::now_ms(),
        })?;
        Ok(ws)
    }

    /// 冻结候选：返回 worktree 当前的 HEAD sha（候选 = 冻结 commit；diff 归因的锚点）。
    pub fn freeze(&self) -> Result<String> {
        let out = git_out(&self.path, &["rev-parse", "HEAD"])?;
        Ok(out.trim().to_string())
    }

    /// 移除 worktree（`--force`：任务可能有未提交改动，清理语义不因脏而失败）。
    /// 分支 `task/{id}` **保留**——候选归因与审计要它。
    pub fn remove(&self, repo: &Path) -> Result<()> {
        git(repo, &["worktree", "remove", "--force", &self.path.to_string_lossy()])?;
        record_remove(repo, &self_branch_task_id(&self.branch));
        Ok(())
    }
}

/// 从分支名反解任务 id（`task/xxx` → `xxx`）。
fn self_branch_task_id(branch: &str) -> String {
    branch.strip_prefix("task/").unwrap_or(branch).to_string()
}

/// 孤儿 worktree：注册表里有记录、但目录已不存在（或 worktree 元数据失效）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrphanWorkspace {
    pub task_id: String,
    pub path: PathBuf,
}

/// 扫描某仓的孤儿 worktree（server 启动时与清理流程共用）。
///
/// 判据：注册表条目的 `path` 不存在 → 孤儿（目录被手删）；目录在但 `git worktree list`
/// 不含它 → 也算孤儿（worktree 元数据被手删，目录成了普通残骸）。
pub fn scan_orphans(repo: &Path) -> Result<Vec<OrphanWorkspace>> {
    let listed = git_out(repo, &["worktree", "list", "--porcelain"])?;
    let listed_paths: Vec<String> = listed
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(normalize)
        .collect();
    let mut orphans = Vec::new();
    for rec in records(repo) {
        let exists = rec.path.exists();
        let listed_hit = listed_paths.iter().any(|p| *p == normalize(&rec.path.to_string_lossy()));
        if !exists || !listed_hit {
            orphans.push(OrphanWorkspace {
                task_id: rec.task_id,
                path: rec.path,
            });
        }
    }
    Ok(orphans)
}

/// 清掉一个孤儿（注册表摘除；目录若还在则物理删除残骸）。
pub fn remove_orphan(repo: &Path, orphan: &OrphanWorkspace) -> Result<()> {
    if orphan.path.exists() {
        let _ = std::fs::remove_dir_all(&orphan.path);
    }
    let _ = git(
        repo,
        &["worktree", "prune"],
    );
    record_remove(repo, &orphan.task_id);
    Ok(())
}

// ---------- 注册表（{repo}/.git/devforge-workspaces/） ----------

fn registry_dir(repo: &Path) -> PathBuf {
    repo.join(".git").join("devforge-workspaces")
}

fn record_write(repo: &Path, rec: &WorkspaceRecord) -> Result<()> {
    let dir = registry_dir(repo);
    std::fs::create_dir_all(&dir).ctx("建 workspace 注册表目录")?;
    let path = dir.join(format!("{}.json", rec.task_id));
    std::fs::write(&path, serde_json::to_vec(rec).ctx("序列化 workspace 记录")?)
        .ctx("写 workspace 记录")
}

fn record_remove(repo: &Path, task_id: &str) {
    let _ = std::fs::remove_file(registry_dir(repo).join(format!("{task_id}.json")));
}

fn record_read(repo: &Path, task_id: &str) -> Option<WorkspaceRecord> {
    let bytes = std::fs::read(registry_dir(repo).join(format!("{task_id}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn records(repo: &Path) -> Vec<WorkspaceRecord> {
    let dir = registry_dir(repo);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            if let Ok(bytes) = std::fs::read(e.path())
                && let Ok(rec) = serde_json::from_slice::<WorkspaceRecord>(&bytes)
            {
                out.push(rec);
            }
        }
    }
    out
}

// ---------- git 调用 ----------

fn git(cwd: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ctx("git 可执行")?;
    if out.status.success() {
        Ok(())
    } else {
        Err(DevforgeError::System(format!(
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )))
    }
}

fn git_out(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ctx("git 可执行")?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(DevforgeError::System(format!(
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )))
    }
}

/// 路径归一（worktree list 的输出可能是符号链接形态；比对此先归一）。
fn normalize(p: &str) -> String {
    Path::new(p)
        .canonicalize()
        .map(|c| c.to_string_lossy().to_string())
        .unwrap_or_else(|_| p.to_string())
}

#[cfg(test)]
mod tests;
