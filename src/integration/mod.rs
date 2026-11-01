//! 集成事务（P012 M3）：候选串行集成 + 组合验证 + CAS 原子更新 + 冲突产物结构化。
//!
//! 设计文档：`docs/plantree/plans/012-parallel-dev-base/topics/04-integration-queue.md`
//! （流程图/CAS 语义/证据重建规则/实施落点速记）。核心不变量：**迟到者不能覆盖新目标**
//! ——目标 ref 更新走 `git update-ref <ref> <new> <expect>` 三参 CAS，expect 不符即重排。
//!
//! 2026-10-31（autonomy-gaps gap-E）：队列**有了生产消费方**——三个子模块：
//! - [`settings`]：总开关与可选门（`<plans_root>/integrations/settings.json`；
//!   缺省关、读坏了 fail-closed、每次决策前重读 → 改文件即热生效）；
//! - [`gate`]：验证门（全量 `cargo test` 硬门 + 可选 clippy / doc-lint；
//!   逐步超时且超时杀进程组；Quality / System 归因二分）；
//! - [`consumer`]：自动消费（settled 候选 → enqueue → integrate → 验证 → Integrated / NeedsRepair 终态）。
//!
//! 失败归因口径（前置裁决④，**两者副作用不同**）：
//! - Quality（断言红 / 编译失败 / 合并冲突 / 候选失联）→ 写产物 + `NeedsRepair` + 产物路径进 `evidence_paths`；
//! - System（cargo 起不来 / 构建目录不可写 / 超时被杀）→ **不动队列、不置状态、不落产物**，
//!   交消费者记一次重试（否则每次基础设施抖动都写一份假质量记录）。

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
        // CH-10 纵深：project 拼路径前的域层校验（store 创建入口已拦，这里防绕过入口的调用方）
        crate::domain::store::Store::validate_path_name(project).ctx("集成队列项目名校验")?;
        let path = Self::queue_path(plans_root, project);
        let candidates = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Vec<Candidate>>(&bytes)
                .ctx("队列文件反序列化（删除损坏文件可重置）")?,
            // 2026-10-27 自审 major：只有「文件不存在」才是空队列；其余读错误必须上抛——
            // 旧口径一律当空队列，随后的 save() 会用只含新候选的队列覆盖原文件，
            // 已排队候选被静默清空。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(DevforgeError::from(e)).ctx(format!(
                    "读集成队列 {} 失败（拒绝当作空队列——后续 save 会静默清掉已排队候选）",
                    path.display()
                ))
            }
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
        self.prune_integrated();
    }

    /// 终态候选裁剪（R4-P36）：Integrated 候选超保留数时删最老（Vec 头部序 ≈
    /// 入队序）。证据三处冗余：目标 ref 历史（merge commit）、integration.* 事件流、
    /// 产物文件——主队列不再永久滞留被 tick/status/查询端点反复全量反序列化。
    /// NeedsRepair 是活单不裁（人工处理语义）。
    const MAX_INTEGRATED_KEPT: usize = 50;
    pub fn prune_integrated(&mut self) {
        let integrated: Vec<usize> = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.status, CandidateStatus::Integrated { .. }))
            .map(|(i, _)| i)
            .collect();
        if integrated.len() <= Self::MAX_INTEGRATED_KEPT {
            return;
        }
        let drop_set: std::collections::HashSet<usize> = integrated
            [..integrated.len() - Self::MAX_INTEGRATED_KEPT]
            .iter()
            .copied()
            .collect();
        let before = self.candidates.len();
        let mut i = 0usize;
        self.candidates.retain(|_| {
            let keep = !drop_set.contains(&i);
            i += 1;
            keep
        });
        tracing::info!(
            component = "integration",
            event = "queue.integrated_pruned",
            before,
            after = self.candidates.len(),
            "Integrated 候选超保留数，已裁最老（证据在目标 ref 历史/事件流/产物）"
        );
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
        crate::domain::store::Store::validate_path_name(project).ctx("集成队列项目名校验")?;
        let path = Self::queue_path(plans_root, project);
        crate::engine::ledger::with_file_lock(&path, || {
            let mut q = Self::load(plans_root, project)?;
            let out = f(&mut q)?;
            q.save()?;
            Ok(out)
        })
    }

    /// 第一个 Pending 候选的下标（测试与调用方选候选用）。
    pub fn next_pending(&self) -> Option<usize> {
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
    /// 集成**调用方选中的**候选（串行消费一次）。
    ///
    /// R4-P42：旧 `integrate_next` 内部自取队首 Pending，而消费者 [`next_consumable`]
    /// 会跳过仓库路径未知的候选——队首是孤儿时，merge 的与记账的不是同一个候选。
    /// 现在候选由调用方选中后传下标，「选谁就集成谁」。
    ///
    /// 流程：集成 worktree checkout 目标最新 → merge 候选（冲突 = NeedsRepair）→
    /// 组合验证（失败 = NeedsRepair）→ CAS 更新目标 ref（expect 不符 = NeedsRebase 重排）。
    pub fn integrate_selected(
        &mut self,
        queue: &mut MergeQueue,
        idx: usize,
    ) -> Result<IntegrationOutcome> {
        // 防漂移：选中的必须仍是 Pending（并发变更时宁可空转一轮）
        if !matches!(
            queue.candidates.get(idx).map(|c| &c.status),
            Some(CandidateStatus::Pending)
        ) {
            return Ok(IntegrationOutcome::Idle);
        }
        let cand = queue.candidates[idx].clone();
        let expect_before = self.target_sha()?;

        // 集成工作树：独立目录 checkout 目标最新（路径按项目消歧，R4-P38）
        let iw_path = Self::integration_path(self.repo, &queue.project);
        // R4-P38：清残骸失败必须留痕（残留路径可从日志重建，不静默丢弃）；
        // NotFound 是首次集成的正常路径，静默。
        if let Err(e) = std::fs::remove_dir_all(&iw_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                component = "integration",
                event = "iw_residue_clean_failed",
                path = %iw_path.display(),
                error = %e,
                "上次集成工作树残骸清理失败（残留路径见上，后续 worktree prune 会摘注册）"
            );
        }
        // 2026-10-27 自审 major：崩溃残留的 worktree 注册（目录已删但 .git/worktrees
        // 还挂着）会让 add 被永久拒绝，整个队列再也无法消费——目录清掉后同步 prune 摘注册。
        git(self.repo, &["worktree", "prune"])?;
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
            let stdout = String::from_utf8_lossy(&merge.stdout).to_string();
            // git merge 的冲突详情走 stdout（stderr 常为空），归因看合并流
            let combined = format!("{stdout}\n{stderr}");
            // 2026-10-27 自审 major（归因三分）：merge 失败 ≠ 合并冲突。
            // conflict / 候选失联 → 质量失败（NeedsRepair，artifact kind 区分）；
            // 其余 → 基础设施故障（Err(System)，队列状态不动 = 保持 Pending，可重试）。
            let kind = match classify_merge_failure(&combined) {
                MergeFailureKind::Conflict => "merge_conflict",
                MergeFailureKind::CandidateUnreachable => "candidate_unreachable",
                MergeFailureKind::Infra => {
                    drop(guard);
                    tracing::error!(
                        component = "integration",
                        event = "integration.merge.infra",
                        project = %queue.project,
                        task_id = %cand.task_id,
                        stderr = %first_line(&stderr),
                        "git merge 基础设施失败（非冲突、非候选失联），候选保持 Pending 等重试"
                    );
                    return Err(DevforgeError::System(format!(
                        "集成 git merge 基础设施失败（task {}）：{}",
                        cand.task_id,
                        first_line(&stderr)
                    )));
                }
            };
            let reason = if kind == "merge_conflict" {
                format!("合并冲突：{}", first_line(&stderr))
            } else {
                format!("候选 commit 不可达：{}", first_line(&stderr))
            };
            let _ = git(&iw_path, &["merge", "--abort"]);
            drop(guard);
            let artifact = self.write_artifact(
                &queue.project,
                &cand,
                &serde_json::json!({
                    "kind": kind,
                    "attribution": "quality",
                    "base_rev": cand.base_rev,
                    "target": expect_before,
                    "candidate": cand.frozen_sha,
                    "stdout": stdout,
                    "stderr": stderr,
                }),
            )?;
            queue.candidates[idx]
                .evidence_paths
                .push(artifact.to_string_lossy().to_string());
            queue.candidates[idx].status = CandidateStatus::NeedsRepair {
                reason: reason.clone(),
            };
            queue.save()?;
            tracing::warn!(
                component = "integration",
                event = "integration.needs_repair",
                project = %queue.project,
                task_id = %cand.task_id,
                kind = kind,
                "集成转入修复队列"
            );
            return Ok(IntegrationOutcome::NeedsRepair {
                task_id: cand.task_id,
                reason,
                artifact,
            });
        }

        // R4-P30：契约边界复核（真落地）——收口门只看工作区，集成链是合入前的最后
        // 防线：merge 已做、CAS 未做，改动集越出候选 owned_paths 就打回，不碰目标 ref。
        // 越界 = Quality 归因（候选自身问题）→ NeedsRepair + 产物；diff 取不到 = System。
        if let Some(reason) = self.boundary_violation(&iw_path, &cand)? {
            drop(guard);
            let artifact = self.write_artifact(
                &queue.project,
                &cand,
                &serde_json::json!({
                    "kind": "boundary_violation",
                    "attribution": "quality",
                    "base_rev": cand.base_rev,
                    "target": expect_before,
                    "candidate": cand.frozen_sha,
                    "error": reason,
                }),
            )?;
            queue.candidates[idx]
                .evidence_paths
                .push(artifact.to_string_lossy().to_string());
            queue.candidates[idx].status = CandidateStatus::NeedsRepair {
                reason: reason.clone(),
            };
            queue.save()?;
            tracing::warn!(
                component = "integration",
                event = "integration.needs_repair",
                project = %queue.project,
                task_id = %cand.task_id,
                kind = "boundary_violation",
                "候选改动越出契约 owned_paths，集成转入修复队列"
            );
            return Ok(IntegrationOutcome::NeedsRepair {
                task_id: cand.task_id,
                reason,
                artifact,
            });
        }

        // 组合验证（注入管线；失败产物含验证输出）
        if let Err(e) = (self.validate)(&iw_path) {
            drop(guard);
            // 归因二分（前置裁决④）：Quality = 候选自身问题（写产物 + 修复队列）；
            // System = 基础设施故障（**不写队列、不置状态、不落产物**，交消费者记一次重试）。
            // 只把 Quality 记成质量账——否则每次基础设施抖动都写一份假质量记录。
            if !matches!(e.root_cause(), DevforgeError::Quality(_)) {
                tracing::error!(
                    component = "integration",
                    event = "integration.validation.infra",
                    project = %queue.project,
                    task_id = %cand.task_id,
                    error = %e,
                    "组合验证基础设施失败（候选保持 Pending 等重试）"
                );
                return Err(DevforgeError::System(format!(
                    "集成组合验证基础设施失败（task {}）：{e}",
                    cand.task_id
                )));
            }
            let artifact = self.write_artifact(
                &queue.project,
                &cand,
                &serde_json::json!({
                    "kind": "validation_failed",
                    "attribution": "quality",
                    "base_rev": cand.base_rev,
                    "target": expect_before,
                    "candidate": cand.frozen_sha,
                    "error": e.to_string(),
                }),
            )?;
            queue.candidates[idx]
                .evidence_paths
                .push(artifact.to_string_lossy().to_string());
            queue.candidates[idx].status = CandidateStatus::NeedsRepair {
                reason: format!("组合验证失败：{e}"),
            };
            queue.save()?;
            tracing::warn!(
                component = "integration",
                event = "integration.needs_repair",
                project = %queue.project,
                task_id = %cand.task_id,
                kind = "validation_failed",
                "组合验证失败，集成转入修复队列"
            );
            return Ok(IntegrationOutcome::NeedsRepair {
                task_id: cand.task_id,
                reason: format!("组合验证失败：{e}"),
                artifact,
            });
        }

        let merged_sha = git_out(&iw_path, &["rev-parse", "HEAD"])?.trim().to_string();
        drop(guard);

        // R4-P41：CAS 前探测目标 ref 是否被工作树检出（命中只留痕不拒绝）
        self.warn_if_target_checked_out();

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
            // R4-P36：终态收敛点也裁（enqueue 之外的另一增长面）；裁后再落盘
            queue.prune_integrated();
            queue.save()?;
            tracing::info!(
                component = "integration",
                event = "integration.cas.success",
                project = %queue.project,
                task_id = %cand.task_id,
                merged_sha = %merged_sha,
                "候选已集成（目标 ref CAS 前进）"
            );
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
        tracing::warn!(
            component = "integration",
            event = "integration.cas.requeue",
            project = %queue.project,
            task_id = %cand.task_id,
            expect = %expect_before,
            moved_to = %target_moved_to,
            "目标 ref 在集成窗口内被推进（CAS expect 不符），候选转重排"
        );
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

    /// 候选改动集 vs 候选 owned_paths（R4-P30 真落地）。
    ///
    /// 改动集 = `git diff --name-only {base_rev}..{frozen_sha}`（在集成 worktree 里求——
    /// checkout 目标后两 sha 均可达）。匹配语义复用 domain 的 `path_covered_by`
    /// （与收口门 `Contract::covers` 同一套，零复制不给语义漂移留缝）。
    /// 无 base_rev（空串 = M0 无基准无法机械 diff）→ 放行，与收口门同口径；
    /// diff 失败/base 形状非法 = System（门不能 fail-open）。`Some(描述)` = 越界。
    fn boundary_violation(&self, iw: &Path, cand: &Candidate) -> Result<Option<String>> {
        use crate::domain::contract::path_covered_by;
        let base = cand.base_rev.trim();
        if base.is_empty() {
            return Ok(None); // 无基准（M0）：机械 diff 不可得，与收口门同口径放行
        }
        let sha_ok = (7..=40).contains(&base.len())
            && base.chars().all(|c| c.is_ascii_hexdigit());
        if !sha_ok {
            return Err(DevforgeError::System(format!(
                "集成边界复核拒绝盲跑：候选 base_rev 形状非法 `{base}`（须 7..=40 位十六进制）"
            )));
        }
        let out = Command::new("git")
            .arg("-C")
            .arg(iw)
            .args(["diff", "--name-only", &format!("{base}..{}", cand.frozen_sha)])
            .output()
            .ctx("git 可执行")?;
        if !out.status.success() {
            return Err(DevforgeError::System(format!(
                "集成边界复核无法取 diff（git 非零退出）：{}",
                first_line(&String::from_utf8_lossy(&out.stderr))
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let violations: Vec<&str> = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter(|f| !cand.owned_paths.iter().any(|pat| path_covered_by(f, pat)))
            .collect();
        if violations.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "越界改动：{} 个文件超出候选 owned_paths（base {base}）：{}",
            violations.len(),
            violations.join(", ")
        )))
    }

    /// R4-P41：目标 ref 正被某个工作树检出时留痕点名——改写 ref 后那些树会显示
    /// 「假脏」（索引仍停旧快照）。只留痕不拒绝：主工作树检出目标分支是常态布局，
    /// 拒绝 = 自动集成对缺省布局永不可用；运营者按日志决定是否 checkout 重置。
    fn warn_if_target_checked_out(&self) {
        let Ok(list) = Command::new("git")
            .arg("-C")
            .arg(self.repo)
            .args(["worktree", "list", "--porcelain"])
            .output()
        else {
            return; // 探测本身失败不阻断集成（CAS 自己会失败并归因）
        };
        if !list.status.success() {
            return;
        }
        let mut current_wt = String::new();
        let mut checked_out: Vec<String> = Vec::new();
        for line in String::from_utf8_lossy(&list.stdout).lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                current_wt = p.to_string();
            } else if line.strip_prefix("branch ") == Some(self.target_ref.as_str()) {
                checked_out.push(current_wt.clone());
            }
        }
        if !checked_out.is_empty() {
            tracing::warn!(
                component = "integration",
                event = "integration.target_checked_out",
                target_ref = %self.target_ref,
                worktrees = ?checked_out,
                "目标 ref 正被工作树检出（改写后这些树将显示脏）——如需干净工作树请人工 checkout 重置"
            );
        }
    }

    /// 集成工作树路径（按项目消歧——R4-P38：同机两项目 basename 相同（/srv/a/app
    /// 与 /srv/b/app）时固定名 _integration 会落到同一物理路径互相删除对方的
    /// 集成工作树；project 已过 safe_segment，拼进路径安全）。
    fn integration_path(repo: &Path, project: &str) -> PathBuf {
        let name = repo
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        repo.parent()
            .unwrap_or(Path::new("."))
            .join(format!("{name}-worktrees"))
            .join(format!("_integration-{project}"))
    }

    /// 冲突/失败产物：`{plans_root}/integrations/{project}/artifacts/{task_id}-{n}.json`。
    /// 全仓审计 #9：按项目分目录（旧实现恒 `_global`，多项目产物混放，检索与清理边界模糊）。
    ///
    /// `pub(crate)`：自动消费方（[`consumer`]）在「基础设施重试耗尽」时复用同一产物口径
    /// （同名 + 同序号规则），不另开一套命名。
    pub(crate) fn write_artifact(
        &self,
        project: &str,
        cand: &Candidate,
        body: &serde_json::Value,
    ) -> Result<PathBuf> {
        // CH-10 纵深：task_id 直接拼产物文件名，落盘前校验（防绕过 store 入口的构造）
        crate::domain::store::Store::validate_path_name(&cand.task_id).ctx("产物 task_id 校验")?;
        let dir = self
            .plans_root
            .join("integrations")
            .join(project)
            .join("artifacts");
        std::fs::create_dir_all(&dir).ctx("建产物目录")?;
        // CH-09：空闲槽序号——旧口径用「目录条目数」当序号，删除/同名候选会撞号覆盖旧证据。
        // 按本 task_id 的已有产物数递增到第一个空闲槽，永不覆盖。
        let mut n = 0u32;
        while dir.join(format!("{}-{n}.json", cand.task_id)).exists() {
            n += 1;
        }
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
        // 2026-11-01 审计批次一（EX 附加/R3 语境）：清理失败必须留痕——前置 prune
        // 能兜住大部分残骸，但与仓内「清理失败必须留痕」口径（孤儿清理同款）对齐。
        if let Err(e) = git(
            &self.repo,
            &["worktree", "remove", "--force", &self.path.to_string_lossy()],
        ) {
            tracing::warn!(
                component = "integration",
                event = "iw_cleanup_failed",
                path = %self.path.display(),
                repo = %self.repo.display(),
                error = %e,
                "worktree 清理失败（后续 prune 兑底，残留路径见上）"
            );
        }
    }
}

 /// NeedsRebase 候选放回 Pending 重验（终态不停滞）——消费循环每轮开头调用。
pub(crate) fn rearm_needs_rebase(project: &str, queue: &mut MergeQueue) {
    for c in queue.candidates.iter_mut() {
        if let CandidateStatus::NeedsRebase { target_moved_to } = &c.status {
            tracing::warn!(
                component = "integration",
                event = "integration.needs_rebase.rearm",
                project = %project,
                task = %c.task_id,
                target = %target_moved_to,
                "NeedsRebase 候选放回 Pending 重验（终态不停滞）"
            );
            c.status = CandidateStatus::Pending;
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

/// merge 失败归因（CH-11 三分）：冲突（质量）/ 候选失联（候选自身缺陷）/ 基础设施。
fn classify_merge_failure(stderr: &str) -> MergeFailureKind {
    if stderr.contains("CONFLICT") {
        MergeFailureKind::Conflict
    } else if ["not something we can merge", "unknown revision", "bad revision"]
        .iter()
        .any(|needle| stderr.contains(needle))
    {
        MergeFailureKind::CandidateUnreachable
    } else {
        MergeFailureKind::Infra
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeFailureKind {
    Conflict,
    CandidateUnreachable,
    Infra,
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
pub mod consumer;
pub mod gate;
pub mod settings;
