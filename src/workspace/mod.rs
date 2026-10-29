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
        Self::ensure_with_candidate(spec, None)
    }

    /// [`Self::ensure`] 的冻结候选形态：额外带上一轮冻结下来的候选 sha。
    ///
    /// 候选只影响「分支已存在且 tip ≠ 基点」这一种情形（此时可收养而不是拒绝）；
    /// 其余语义与旧入口完全一致。候选原文由调用方交下来，形状门在本层（最后一道门）做。
    pub fn ensure_with_candidate(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
    ) -> Result<Workspace> {
        Self::ensure_inner(spec, frozen_candidate, false)
    }

    /// [`Self::ensure_with_candidate`] 的**重试形态**：run 因基础设施故障中途死亡后
    /// 自动重试（attempt >= 2）时重建工作区。
    ///
    /// 与首次语义唯一的差别：任务分支的 tip 既不是基点、又**没有可用的冻结候选**
    /// （`frozen-commit.txt` 不存在，或内容不是合法 40-hex）时，判定为「上一轮死在
    /// coding 中途、freeze 从未发生」——那批中间产物按定义未经 testing 验证、不可信，
    /// 正确语义是**重派生**：删掉这条未冻结遗留分支、在基点重建，并先落一条 WARN 留痕
    /// （带完整 tip sha）。
    ///
    /// **有可用冻结候选时不删**：仍走 F2 收养（候选提交只靠该分支可达，删了它候选就成
    /// 孤儿）。attempt <= 1 的首次语义一律不受影响（见 [`Self::ensure_with_candidate`]）。
    ///
    /// 调用方负责判定「是否处于重试上下文」——本层不读 attempt（attempt 的真源是
    /// 线程本地的 `crate::observability::attempt()`，只有引擎层知道当前是不是重试）。
    pub fn ensure_with_candidate_on_retry(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
    ) -> Result<Workspace> {
        Self::ensure_inner(spec, frozen_candidate, true)
    }

    /// ensure 的实现点（`ensure` / `ensure_with_candidate` / `ensure_with_candidate_on_retry`
    /// 三条入口共用同一份实现，只有 `allow_retry_rebuild` 不同）。
    fn ensure_inner(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
        allow_retry_rebuild: bool,
    ) -> Result<Workspace> {
        Self::validate_task_id(spec)?;
        let path = Self::target_path(spec);
        // 基点先冻结成 sha：移动引用（HEAD/main）的漂移只有 sha 对 sha 才检得出。
        let base_sha = resolve_base(&spec.repo, &spec.base_rev)?;
        if path.exists() {
            let rec = record_read(&spec.repo, &spec.task_id)
                .ok_or_else(|| {
                    DevforgeError::Store(format!(
                        "worktree 目录已存在但无注册记录：{}（手动清理后重跑）",
                        path.display()
                    ))
                })?;
            if rec.base_rev != base_sha {
                return Err(DevforgeError::Store(format!(
                    "worktree 基点漂移：记录 {} vs 契约 {}（清理 {} 后重跑）",
                    rec.base_rev,
                    base_sha,
                    path.display()
                )));
            }
            return Ok(Workspace {
                path,
                branch: rec.branch,
                base_rev: rec.base_rev,
            });
        }
        Self::create_inner(spec, frozen_candidate, allow_retry_rebuild)
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
    /// "branch already exists"。现在分四种情形：
    /// - 分支不存在 → 照旧 `-b`；
    /// - 分支已存在且**仍停在基点**（上一轮没产生提交 / 崩溃回退）→ 直接挂载复用；
    /// - 分支已存在且**停在上一轮冻结的候选**（tip == 传入的冻结候选）→ **收养**：
    ///   同样直接挂载，分支引用不动，worktree HEAD 即候选 tip（断点续跑靠它拿回上一轮成果）；
    /// - 分支已存在但**既非基点也非冻结候选**（外来指针 / 上一轮失败退出留下的未冻结候选）
    ///   → 明确拒绝，提示人工核验分支当前 sha 后自行决定清理方式，不静默切换基点
    ///   （否则本轮的 diff 归因会错位）。
    ///
    /// **唯一的例外**（重试形态入口 [`Self::create_with_candidate_on_retry`]）：在**明确的
    /// 重试上下文**（run 因基础设施故障中途死亡后自动重试，attempt >= 2）里，且**没有可用的
    /// 冻结候选**（`frozen-commit.txt` 不存在，或内容不是合法 40-hex）时，分支上的东西按定义
    /// 是「上一轮死在 coding 中途、freeze 从未发生」的未验证产物 → 自动删掉该分支并在基点
    /// 重建（重派生），删之前先落一条带 tip sha 的 WARN 留痕。
    ///
    /// 这个例外**只在「无冻结候选」时开**：有可用候选时分支上可能留着唯一一份候选提交
    /// （候选提交只靠该分支可达），删了它候选就变孤儿——那种情形仍然拒绝并指向人工核验。
    pub fn create(spec: &WorkspaceSpec) -> Result<Workspace> {
        Self::create_with_candidate(spec, None)
    }

    /// [`Self::create`] 的冻结候选形态：额外带上一轮冻结下来的候选 sha。
    ///
    /// 命中时走**收养**：不新建分支、不改写分支引用，直接把现有分支挂到目标路径，
    /// worktree HEAD 即候选 tip。注册表记的仍是**解析后的基点**（不是候选 tip）——
    /// 否则下一次 ensure 会把合法基点误判成漂移。
    pub fn create_with_candidate(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
    ) -> Result<Workspace> {
        Self::create_inner(spec, frozen_candidate, false)
    }

    /// [`Self::create_with_candidate`] 的**重试形态**（语义见
    /// [`Self::ensure_with_candidate_on_retry`]）：无冻结候选的脏分支在重试上下文里
    /// 会被删掉并在基点重建；其余语义与既有入口一致。
    pub fn create_with_candidate_on_retry(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
    ) -> Result<Workspace> {
        Self::create_inner(spec, frozen_candidate, true)
    }

    /// create 的实现点（三条 create 入口共用；`allow_retry_rebuild` 是唯一差别）。
    ///
    /// 2026-10-27 自审 majors（WA 域）：① base_rev 先冻结成 commit sha（移动 ref
    /// 漂移要 sha 对 sha，字符串对字符串时 HEAD/main 永不漂移）；② 注册表写失败
    /// 必须回滚——无记录残骸脱离孤儿扫描索引，同一 task 再 ensure 永久报错。
    fn create_inner(
        spec: &WorkspaceSpec,
        frozen_candidate: Option<&str>,
        allow_retry_rebuild: bool,
    ) -> Result<Workspace> {
        Self::validate_task_id(spec)?;
        let base_sha = resolve_base(&spec.repo, &spec.base_rev)?;
        Self::create_at_resolved(spec, base_sha, frozen_candidate, allow_retry_rebuild)
    }

    fn create_at_resolved(
        spec: &WorkspaceSpec,
        base_sha: String,
        frozen_candidate: Option<&str>,
        allow_retry_rebuild: bool,
    ) -> Result<Workspace> {
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
        // 形状门只在本层一处（调用方只交原文）：不合法就不参与匹配，也不升级成错误。
        let candidate = normalize_candidate(frozen_candidate);
        if frozen_candidate.is_some() && candidate.is_none() {
            tracing::warn!(
                component = "workspace",
                event = "workspace.candidate_shape_invalid",
                candidate = ?frozen_candidate,
                "冻结候选不是 40 位十六进制 sha——不参与收养匹配（分支 tip 非基点即拒绝）"
            );
        }
        let path_str = path.to_string_lossy().to_string();
        let mut created_branch = false;
        let mut adopted_tip: Option<String> = None;
        match existing_tip {
            Some(tip) => {
                // 分支已存在：合法起点只有两个——契约基点，或上一轮冻结下来的候选。
                // 顺序有意：基点优先（候选恰好等于基点时也走「复用」，语义不变）。
                let at_base = tip == base_sha;
                let at_candidate = !at_base
                    && candidate
                        .as_deref()
                        .is_some_and(|c| tip.eq_ignore_ascii_case(c));
                if at_base || at_candidate {
                    // 两种情形都不新建分支、不改写引用：把现有分支直接挂到目标路径。
                    git(&spec.repo, &["worktree", "add", &path_str, &branch])?;
                    adopted_tip = at_candidate.then(|| tip.clone());
                } else if allow_retry_rebuild && candidate.is_none() {
                    // 重试上下文 + **无可用冻结候选** → 上一轮死在 coding 中途、freeze 从未
                    // 发生：这批中间产物按定义未经 testing 验证、不可信 → 重派生
                    // （删分支 + 在基点重建）。有可用候选时**不删**（候选提交只靠该分支可达，
                    // 删了就成孤儿）→ 仍走下面的拒绝路径。
                    Self::rebuild_unfrozen_branch(spec, &branch, &tip, &base_sha, &path_str)?;
                    created_branch = true;
                } else {
                    return Err(DevforgeError::Store(format!(
                        "任务分支 `{branch}` 已存在且指向 {tip}：非基点也非冻结候选（契约基点 {base_sha}）——
                         同一 task 重建工作区前请先人工核验 `{branch}` 的当前 sha，再自行决定清理方式；
                         分支上可能留着上一轮候选提交，删分支会让候选变孤儿",
                    )));
                }
            }
            None => {
                git(
                    &spec.repo,
                    &["worktree", "add", "-b", &branch, &path_str, &base_sha],
                )?;
                created_branch = true;
            }
        }
        if let Some(tip) = &adopted_tip {
            tracing::info!(
                component = "workspace",
                event = "workspace.adopted",
                branch = %branch,
                path = %path_str,
                frozen = %tip,
                "worktree 收养冻结候选（分支未重建、引用未改写，HEAD = 候选 tip）"
            );
        }
        let ws = Workspace {
            path,
            branch,
            base_rev: base_sha,
        };
        if let Err(e) = record_write(&spec.repo, &WorkspaceRecord {
            task_id: spec.task_id.clone(),
            path: ws.path.clone(),
            branch: ws.branch.clone(),
            base_rev: ws.base_rev.clone(),
            created_at_ms: crate::domain::now_ms(),
        }) {
            // WA-04 回滚：注册表没写进去，worktree 留在盘上就是「无记录残骸」——
            // 孤儿扫描以注册表为唯一索引，永远看不见它，同一 task 再 ensure 永久报错。
            let _ = git(&spec.repo, &["worktree", "remove", "--force", &path_str]);
            if created_branch {
                let _ = git(&spec.repo, &["branch", "-D", &ws.branch]);
            }
            return Err(DevforgeError::Store(format!(
                "写 workspace 注册表失败（worktree 与分支已回滚）：{e}"
            )));
        }
        Ok(ws)
    }

    /// 删掉未冻结的遗留分支并在基点重建（**仅限重试上下文 + 无可用冻结候选**）。
    ///
    /// 顺序有意：**先落 WARN 留痕（带完整 tip sha），再删分支**——删完 tip 提交可能不可达
    /// （不建备份 ref：「未冻结产物不可信」与「其实还在仓里」语义相反），事后取证只能靠
    /// 这条日志里的 sha（`git gc --prune` 之前仍可 `git cat-file`）。
    ///
    /// `git worktree prune` 是 best-effort 兜底：上一轮被 watchdog 强杀时 worktree 元数据
    /// 可能没清（`WsCleanup::drop` 没跑完），`git branch -D` 会以「used by worktree」失败；
    /// prune 只清失效元数据，不动有效 worktree（目录还在的那种情形走的是复用分支，
    /// 压根到不了这里）。
    fn rebuild_unfrozen_branch(
        spec: &WorkspaceSpec,
        branch: &str,
        tip: &str,
        base_sha: &str,
        path_str: &str,
    ) -> Result<()> {
        tracing::warn!(
            component = "workspace",
            event = "workspace.retry_rebuilt_unfrozen",
            branch = %branch,
            tip = %tip,
            base = %base_sha,
            attempt = crate::observability::attempt(),
            "未冻结遗留分支已自动重建（上一轮死在 coding 中途，产物未经 testing 验证——已删除并在基点重建）"
        );
        if let Err(e) = git(&spec.repo, &["worktree", "prune"]) {
            tracing::warn!(
                component = "workspace",
                "worktree prune 失败（继续尝试删除未冻结遗留分支）：{e}"
            );
        }
        git(&spec.repo, &["branch", "-D", branch])?;
        git(
            &spec.repo,
            &["worktree", "add", "-b", branch, path_str, base_sha],
        )?;
        Ok(())
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

/// 冻结候选的形状门：trim 之后必须**恰好 40 位 ASCII 十六进制**（commit sha）。
///
/// 合法 → `Some(小写 sha)`；不合法（空串 / 纯空白 / `cafebabe` / 40 位 sha 后跟别的非空行）
/// → `None`（不参与收养匹配，也不升级成错误——脏内容只是「不是候选」）。
///
/// 形状门只放这一处：调用方（change_engine）只负责把原文交下来，判定由本层做，
/// 两处各判一次迟早判定漂移。大小写无关：git 吐出的 sha 是小写，冻结文件是人写的。
fn normalize_candidate(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.len() != 40 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(text.to_ascii_lowercase())
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
        // WA-02：物理删除失败必须上抛并保留注册表记录——
        // 吞掉后照样摘记录，残骸从此脱离扫描视野，同一 task 被永久砖化。
        std::fs::remove_dir_all(&orphan.path).map_err(|e| {
            DevforgeError::Store(format!(
                "删除孤儿 worktree 残骸 {} 失败（注册表记录保留，残骸不脱离扫描视野）：{e}",
                orphan.path.display()
            ))
        })?;
    }
    if let Err(e) = git(repo, &["worktree", "prune"]) {
        // prune 失败不阻塞摘记录（它只是 git 元数据清理，注册表才是扫描索引），但要留痕。
        tracing::warn!(component = "workspace", "worktree prune 失败（继续摘除注册表记录）：{e}");
    }
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

/// WA-03 基点冻结：把契约给的 base_rev（可能是 HEAD/main 这类移动引用）解析成
/// commit sha。漂移检测必须是 sha 对 sha——字符串对字符串时移动 ref 永不漂移，
/// 「契约显示新基点、worktree 还停在旧基点」的错位不可检。
/// rev 解析失败 = 契约错误（Config，不自动重启），不是环境故障。
fn resolve_base(repo: &Path, base_rev: &str) -> Result<String> {
    // 自审 minor（193）：外部串直接当 rev 参数——`--` 前缀会被 git 当**选项**解析
    // （`--git-dir=`/`--output=` 一类）。合法性由 rev-parse 判，但选项面必须先在门口挡掉。
    if base_rev.starts_with('-') || base_rev.contains('\0') {
        return Err(DevforgeError::Config(format!(
            "契约基点 `{base_rev}` 形状非法（不得以 - 开头或含 NUL）——拒绝当 git 选项解析"
        )));
    }
    git_out(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base_rev}^{{commit}}"),
        ],
    )
    .map(|s| s.trim().to_string())
    .map_err(|_| {
        DevforgeError::Config(format!(
            "契约基点 `{base_rev}` 无法解析为 commit（仓库 {}）——检查拼写或先 fetch",
            repo.display()
        ))
    })
}

#[cfg(test)]
mod tests;
