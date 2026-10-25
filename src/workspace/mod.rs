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
mod tests {
    use super::*;

    /// git 仓夹具：init + 首个 commit；返回（目录，基线 sha）。
    fn repo_with_base() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        git(dir.path(), &["add", "-A"]).unwrap();
        git(dir.path(), &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base",
        ])
        .unwrap();
        let base = git_out(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        (dir, base.trim().to_string())
    }

    fn spec(dir: &Path, base: &str) -> WorkspaceSpec {
        WorkspaceSpec {
            repo: dir.to_path_buf(),
            task_id: "af-batch-a".into(),
            base_rev: base.to_string(),
        }
    }

    #[test]
    fn create_makes_worktree_on_task_branch_at_base() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        assert!(ws.path.exists(), "worktree 目录应存在");
        assert_eq!(ws.branch, "task/af-batch-a");
        // 分支与基点对齐：worktree HEAD == base
        let head = Workspace::freeze(&ws).unwrap();
        assert_eq!(head, base, "worktree 应 pin 在基点");
        // spawn 视角的 cwd 就在 worktree 里，不在 main
        assert_ne!(ws.path, dir.path());
    }

    #[test]
    fn remove_drops_worktree_but_keeps_branch() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        let branch = ws.branch.clone();
        ws.remove(dir.path()).unwrap();
        assert!(!ws.path.exists(), "worktree 目录应被移除");
        // 分支保留（候选归因要它）
        let branches = git_out(dir.path(), &["branch", "--list", &branch]).unwrap();
        assert!(branches.contains(&branch), "分支应保留：{branches}");
        // 注册表摘除
        assert!(scan_orphans(dir.path()).unwrap().is_empty());
    }

    /// 自审 critical 回归：`remove()` 保留分支，同一 task 的第二次 run **必须能重建**
    /// （旧实现无条件 `-b` → git 报 branch already exists，第二次 run 必输）。
    #[test]
    fn recreate_after_remove_succeeds_when_branch_still_at_base() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        ws.remove(dir.path()).unwrap();
        let again = Workspace::create(&spec(dir.path(), &base)).expect("分支仍在基点，应能复用重建");
        assert!(again.path.exists());
        assert_eq!(Workspace::freeze(&again).unwrap(), base);
    }

    /// 分支上已有候选提交（≠基点）时，重建必须**明确拒绝并指路**，
    /// 不得静默切基点（否则本轮 diff 归因错位）。
    #[test]
    fn recreate_with_advanced_branch_is_rejected_with_actionable_message() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        std::fs::write(ws.path.join("candidate.txt"), "x\n").unwrap();
        git(&ws.path, &["add", "-A"]).unwrap();
        git(
            &ws.path,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "candidate",
            ],
        )
        .unwrap();
        ws.remove(dir.path()).unwrap();
        let err = Workspace::create(&spec(dir.path(), &base)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("已存在"), "要指明分支已存在：{msg}");
        assert!(msg.contains("git branch -D"), "要给出清理命令：{msg}");
    }

    #[test]
    fn duplicate_create_is_rejected() {
        let (dir, base) = repo_with_base();
        Workspace::create(&spec(dir.path(), &base)).unwrap();
        let err = Workspace::create(&spec(dir.path(), &base)).unwrap_err();
        assert!(err.to_string().contains("已存在"), "{err}");
    }

    #[test]
    fn missing_directory_is_detected_as_orphan() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        // 模拟手删：物理删目录（worktree 元数据还在 main 的 .git/worktrees 里）
        std::fs::remove_dir_all(&ws.path).unwrap();
        let orphans = scan_orphans(dir.path()).unwrap();
        assert_eq!(orphans.len(), 1, "{orphans:?}");
        assert_eq!(orphans[0].task_id, "af-batch-a");
        remove_orphan(dir.path(), &orphans[0]).unwrap();
        assert!(scan_orphans(dir.path()).unwrap().is_empty(), "清理后无孤儿");
    }

    #[test]
    fn metadata_loss_is_detected_as_orphan() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        // 模拟 worktree 元数据被删（目录在但 git 不认了）
        git(dir.path(), &["worktree", "remove", "--force", &ws.path.to_string_lossy()])
            .unwrap();
        std::fs::create_dir_all(&ws.path).unwrap();
        std::fs::write(ws.path.join("residue.txt"), "残骸").unwrap();
        let orphans = scan_orphans(dir.path()).unwrap();
        assert_eq!(orphans.len(), 1, "{orphans:?}");
        remove_orphan(dir.path(), &orphans[0]).unwrap();
        assert!(!ws.path.join("residue.txt").exists(), "残骸应被物理清除");
    }

    #[test]
    fn freeze_tracks_new_commits() {
        let (dir, base) = repo_with_base();
        let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
        std::fs::write(ws.path.join("changed.rs"), "fn changed() {}\n").unwrap();
        git(&ws.path, &["add", "-A"]).unwrap();
        git(&ws.path, &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "step 1",
        ])
        .unwrap();
        let frozen = Workspace::freeze(&ws).unwrap();
        assert_ne!(frozen, base, "冻结 commit 应随新提交前进");
        // base..frozen 的 diff 归因：改动只有 changed.rs
        let diff = git_out(&ws.path, &["diff", "--name-only", &format!("{base}..{frozen}")]);
        assert!(diff.unwrap().contains("changed.rs"));
    }

    #[test]
    fn ensure_reuses_existing_worktree_and_rejects_base_drift() {
        // T1.4 resume 对齐：worktree 在 → 复用；base 漂移 → 拒绝
        let (dir, base) = repo_with_base();
        let s = spec(dir.path(), &base);
        let ws = Workspace::ensure(&s).unwrap();
        // resume：同 spec 再 ensure → 同一 worktree 复用
        let ws2 = Workspace::ensure(&s).unwrap();
        assert_eq!(ws.path, ws2.path);
        // base 漂移：换 base_rev 再 ensure → 拒绝
        let mut drifted = spec(dir.path(), &base);
        drifted.base_rev = "HEAD~0".into(); // 值不同即漂移（同 sha 也会被字符串不等拦住）
        let err = Workspace::ensure(&drifted).unwrap_err();
        assert!(err.to_string().contains("漂移"), "{err}");
    }

    #[test]
    fn two_worktrees_run_concurrently_without_stepping_on_each_other() {
        // M1 验收：两 worktree 并发各跑一任务不互踩——同仓不同 task，各改各的文件
        let (dir, base) = repo_with_base();
        let mut spec_a = spec(dir.path(), &base);
        spec_a.task_id = "task-a".into();
        let mut spec_b = spec(dir.path(), &base);
        spec_b.task_id = "task-b".into();
        let wa = Workspace::create(&spec_a).unwrap();
        let wb = Workspace::create(&spec_b).unwrap();
        assert_ne!(wa.path, wb.path);
        // 各自改动各自的文件并提交
        std::fs::write(wa.path.join("from-a.rs"), "// a\n").unwrap();
        std::fs::write(wb.path.join("from-b.rs"), "// b\n").unwrap();
        for (w, msg) in [(&wa, "task-a step"), (&wb, "task-b step")] {
            git(&w.path, &["add", "-A"]).unwrap();
            git(&w.path, &[
                "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", msg,
            ])
            .unwrap();
        }
        // 互不踩踏：a 看不到 b 的文件，反之亦然
        assert!(!wa.path.join("from-b.rs").exists());
        assert!(!wb.path.join("from-a.rs").exists());
        assert!(wa.path.join("from-a.rs").exists());
        assert!(wb.path.join("from-b.rs").exists());
        // main 工作树不被直改
        assert!(!dir.path().join("from-a.rs").exists());
        assert!(!dir.path().join("from-b.rs").exists());
    }
}
