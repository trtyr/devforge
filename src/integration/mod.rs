//! 集成事务（P012 M3）：候选串行集成 + 组合验证 + CAS 原子更新 + 冲突产物结构化。
//!
//! 设计文档：`docs/plantree/plans/012-parallel-dev-base/topics/04-integration-queue.md`
//! （流程图/CAS 语义/证据重建规则/实施落点速记）。核心不变量：**迟到者不能覆盖新目标**
//! ——目标 ref 更新走 `git update-ref <ref> <new> <expect>` 三参 CAS，expect 不符即重排。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

/// 集成候选：worktree freeze 的产物（M1 的 frozen-commit.txt 即来源）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// 来源任务 id（分支 `task/{id}` 的所有者）。
    pub task_id: String,
    /// 冻结 commit（worktree HEAD，diff 归因锚点）。
    pub frozen_sha: String,
    /// 候选的基点（worktree 创建时的 base_rev，重排/rebase 参考）。
    pub base_rev: String,
    /// 契约边界（组合验证后的 owned_paths 复核用）。
    pub owned_paths: Vec<String>,
    /// 队列内状态（Pending → Integrated / NeedsRepair / NeedsRebase）。
    pub status: CandidateStatus,
    /// 证据 bundle 路径（M3 记录引用；bundle 重建在 M5 观测期完善）。
    #[serde(default)]
    pub evidence_paths: Vec<String>,
}

/// 候选状态机：Pending → Integrated / NeedsRepair / NeedsRebase。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CandidateStatus {
    /// 排队中。
    Pending,
    /// 已集成（CAS 成功，目标 ref 含本候选）。
    Integrated { integrated_sha: String },
    /// 修复队列：组合验证失败或合并冲突（修复 = 新候选带新证据走完整流程）。
    NeedsRepair { reason: String },
    /// 重排：目标在集成期间被别的候选推进（CAS expect 不符）→ rebase 到新目标后重验。
    NeedsRebase { target_moved_to: String },
}

/// merge 队列（文件态：`{plans_root}/integrations/{project}/queue.json`，重启存活）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueue {
    /// 项目名（队列按项目隔离——不同项目不同目标仓）。
    pub project: String,
    pub candidates: Vec<Candidate>,
    #[serde(skip)]
    path: PathBuf,
}

impl MergeQueue {
    /// 从 plans_root 加载项目队列（无文件 = 空队列）。
    pub fn load(plans_root: &Path, project: &str) -> Result<Self> {
        let path = Self::queue_path(plans_root, project);
        let candidates = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Vec<Candidate>>(&bytes)
                .ctx("队列文件反序列化（删除损坏文件可重置）")?,
            Err(_) => Vec::new(),
        };
        Ok(Self {
            project: project.to_string(),
            candidates,
            path,
        })
    }

    pub fn queue_path(plans_root: &Path, project: &str) -> PathBuf {
        plans_root
            .join("integrations")
            .join(project)
            .join("queue.json")
    }

    /// 入队（追加到尾部——串行集成的公平语义）。
    pub fn enqueue(&mut self, cand: Candidate) {
        self.candidates.push(cand);
    }

    /// 落盘（自审 major：原子写——半截 queue.json 会让下一次 load 判损坏静默清空）。
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ctx("建队列目录")?;
        }
        let bytes = serde_json::to_vec_pretty(&self.candidates).ctx("序列化队列")?;
        let tmp = self.path.with_extension(format!(
            "json.{}.tmp",
            std::process::id()
        ));
        (|| -> Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).ctx("建队列临时文件")?;
            f.write_all(&bytes).ctx("写队列临时文件")?;
            f.sync_all().ctx("刷队列临时文件")?;
            std::fs::rename(&tmp, &self.path).ctx("原子替换队列文件")?;
            Ok(())
        })()
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    /// 读-改-写互斥（自审 major）：与 scheduler tick / API 入队并发时，无锁的
    /// load→save 会互相覆写（丢候选/状态回退成 Pending 造成重复集成）。
    /// 锁文件紧邻 queue.json，复用引擎的 with_file_lock。
    ///
    /// 已知局限（登记不修）：集成器 integrate_next 持 &mut 队列跑完整合并（分钟级），
    /// 不适合持锁——其每次状态转移的 save 仍以自己快照为准，期间经 API 并发入队的
    /// 候选会被覆盖。快速写方（API/tick）已全部走本方法，长窗口由集成器独占的
    /// 运行语义兜底。
    pub fn mutate<T>(
        plans_root: &Path,
        project: &str,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let path = Self::queue_path(plans_root, project);
        crate::engine::ledger::SpawnLedger::with_file_lock(&path, || {
            let mut q = Self::load(plans_root, project)?;
            let out = f(&mut q)?;
            q.save()?;
            Ok(out)
        })
    }

    /// 第一个 Pending 候选的下标。
    fn next_pending(&self) -> Option<usize> {
        self.candidates.iter().position(|c| matches!(c.status, CandidateStatus::Pending))
    }
}

/// 集成结果（结构化返回，事件与 API 由调用方组装）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum IntegrationOutcome {
    /// 队列无 Pending 候选。
    Idle,
    /// 集成成功：目标 ref 已 CAS 前进到 merged_sha。
    Integrated {
        task_id: String,
        merged_sha: String,
    },
    /// 重排：目标被推进（expect 不符），候选回 NeedsRebase。
    Requeued {
        task_id: String,
        target_moved_to: String,
    },
    /// 修复队列：合并冲突或组合验证失败。
    NeedsRepair {
        task_id: String,
        reason: String,
        /// 冲突/失败产物路径（结构化 JSON，修复 = 新候选带新证据）。
        artifact: PathBuf,
    },
}

/// 集成执行器：绑定目标仓 / 目标 ref / 组合验证管线的串行消费者。
pub struct Integrator<'a> {
    /// 目标仓（main 工作树——集成在专用 worktree 里做，不动它）。
    pub repo: &'a Path,
    /// 目标 ref（如 `refs/heads/main`——CAS 的守卫对象）。
    pub target_ref: String,
    /// 队列与产物文件的根。
    pub plans_root: &'a Path,
    /// 组合验证管线（注入式：生产 = cargo test 全量 + doc-lint + owned_paths 复核；
    /// 测试 = 闭包注入，让「验证失败归因」可以确定性测）。
    pub validate: ValidateFn<'a>,
}

/// 组合验证管线签名（注入式）。
pub type ValidateFn<'a> = Box<dyn Fn(&Path) -> Result<()> + 'a>;

impl<'a> Integrator<'a> {
    /// 集成下一个 Pending 候选（串行消费一次）。
    ///
    /// 流程：出队 → 集成 worktree checkout 目标最新 → merge 候选（冲突 = NeedsRepair）→
    /// 组合验证（失败 = NeedsRepair）→ CAS 更新目标 ref（expect 不符 = NeedsRebase 重排）。
    pub fn integrate_next(&mut self, queue: &mut MergeQueue) -> Result<IntegrationOutcome> {
        let Some(idx) = queue.next_pending() else {
            return Ok(IntegrationOutcome::Idle);
        };
        let cand = queue.candidates[idx].clone();
        let expect_before = self.target_sha()?;

        // 集成工作树：独立目录 checkout 目标最新（M1 路径约定，`_integration` 固定名独占）
        let iw_path = Self::integration_path(self.repo);
        let _ = std::fs::remove_dir_all(&iw_path); // 上一次的残骸清掉再建
        git(self.repo, &[
            "worktree",
            "add",
            "--detach",
            &iw_path.to_string_lossy(),
            &expect_before,
        ])?;
        let guard = IwCleanup { path: iw_path.clone(), repo: self.repo.to_path_buf() };

        // merge 候选（--no-ff：候选边界在目标历史里可见）
        let merge = Command::new("git")
            .arg("-C")
            .arg(&iw_path)
            .args(["merge", "--no-ff", "--no-edit", &cand.frozen_sha])
            .output()
            .ctx("git 可执行")?;
        if !merge.status.success() {
            let stderr = String::from_utf8_lossy(&merge.stderr).to_string();
            let _ = git(&iw_path, &["merge", "--abort"]);
            drop(guard);
            let artifact = self.write_artifact(
                &queue.project,
                &cand,
                &serde_json::json!({
                    "kind": "merge_conflict",
                    "base_rev": cand.base_rev,
                    "target": expect_before,
                    "candidate": cand.frozen_sha,
                    "stderr": stderr,
                }),
            )?;
            queue.candidates[idx].status = CandidateStatus::NeedsRepair {
                reason: format!("合并冲突：{}", first_line(&stderr)),
            };
            queue.save()?;
            return Ok(IntegrationOutcome::NeedsRepair {
                task_id: cand.task_id,
                reason: format!("合并冲突：{}", first_line(&stderr)),
                artifact,
            });
        }

        // 组合验证（注入管线；失败产物含验证输出）
        if let Err(e) = (self.validate)(&iw_path) {
            drop(guard);
            let artifact = self.write_artifact(
                &queue.project,
                &cand,
                &serde_json::json!({
                    "kind": "validation_failed",
                    "base_rev": cand.base_rev,
                    "target": expect_before,
                    "candidate": cand.frozen_sha,
                    "error": e.to_string(),
                }),
            )?;
            queue.candidates[idx].status = CandidateStatus::NeedsRepair {
                reason: format!("组合验证失败：{e}"),
            };
            queue.save()?;
            return Ok(IntegrationOutcome::NeedsRepair {
                task_id: cand.task_id,
                reason: format!("组合验证失败：{e}"),
                artifact,
            });
        }

        let merged_sha = git_out(&iw_path, &["rev-parse", "HEAD"])?.trim().to_string();
        drop(guard);

        // CAS：期望旧值 = 集成开始时的目标（期间被别的候选推进 → expect 不符 → 重排）
        let cas = Command::new("git")
            .arg("-C")
            .arg(self.repo)
            .args([
                "update-ref",
                &self.target_ref,
                &merged_sha,
                &expect_before,
            ])
            .output()
            .ctx("git 可执行")?;
        if cas.status.success() {
            queue.candidates[idx].status = CandidateStatus::Integrated {
                integrated_sha: merged_sha.clone(),
            };
            queue.save()?;
            return Ok(IntegrationOutcome::Integrated {
                task_id: cand.task_id,
                merged_sha,
            });
        }
        let target_moved_to = self.target_sha()?;
        queue.candidates[idx].status = CandidateStatus::NeedsRebase {
            target_moved_to: target_moved_to.clone(),
        };
        queue.save()?;
        Ok(IntegrationOutcome::Requeued {
            task_id: cand.task_id,
            target_moved_to,
        })
    }

    /// 目标 ref 当前值。
    fn target_sha(&self) -> Result<String> {
        Ok(git_out(self.repo, &["rev-parse", &self.target_ref])?
            .trim()
            .to_string())
    }

    /// 集成工作树路径（固定名独占——集成串行）。
    fn integration_path(repo: &Path) -> PathBuf {
        let name = repo
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        repo.parent()
            .unwrap_or(Path::new("."))
            .join(format!("{name}-worktrees"))
            .join("_integration")
    }

    /// 冲突/失败产物：`{plans_root}/integrations/{project}/artifacts/{task_id}-{n}.json`。
    /// 全仓审计 #9：按项目分目录（旧实现恒 `_global`，多项目产物混放，检索与清理边界模糊）。
    fn write_artifact(
        &self,
        project: &str,
        cand: &Candidate,
        body: &serde_json::Value,
    ) -> Result<PathBuf> {
        let dir = self
            .plans_root
            .join("integrations")
            .join(project)
            .join("artifacts");
        std::fs::create_dir_all(&dir).ctx("建产物目录")?;
        let n = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok()).count())
            .unwrap_or(0);
        let path = dir.join(format!("{}-{n}.json", cand.task_id));
        std::fs::write(&path, serde_json::to_vec_pretty(body).ctx("序列化产物")?)
            .ctx("写冲突/失败产物")?;
        Ok(path)
    }
}

/// worktree 清理 guard（Ok/Err 两路都摘集成工作树）。
struct IwCleanup {
    path: PathBuf,
    repo: PathBuf,
}
impl Drop for IwCleanup {
    fn drop(&mut self) {
        let _ = git(&self.repo, &["worktree", "remove", "--force", &self.path.to_string_lossy()]);
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

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

#[cfg(test)]
mod tests;
