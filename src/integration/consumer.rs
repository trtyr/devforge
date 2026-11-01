//! 自动集成消费者（autonomy-gaps gap-E）：settled 候选自动走完 enqueue → integrate → 验证 → 终态。
//!
//! 触发形态：独立常驻任务，由 `src/server/scheduler.rs` 的 `scheduler_loop` 拉起（周期 30s）。
//! 安全边界：**总开关缺省关**（`<plans_root>/integrations/settings.json` 的
//! `auto_integrate_enabled`）——自主合并需运营者显式授权。
//!
//! 一轮 drain 的口径（每项目每轮**最多消费 1 个候选**，切短集成窗口）：
//! 1. 开关关 → **零动作**返回（连队列都不读：不得产生任何集成 worktree 目录）；
//! 2. rearm：`NeedsRebase` → `Pending`（与调度 tick 同一口径，见 [`super::rearm_needs_rebase`]）；
//! 3. 取首个 `Pending` 候选（仓库路径未知的跳过并留痕——不留痕就静默卡住队列）；
//! 4. 已集成判定：`git merge-base --is-ancestor <候选 sha> <目标 ref>` 命中 → 直接置
//!    `Integrated`（CAS 已成功但队列未落盘的场景，不再重复 merge）；
//! 5. 跑 [`Integrator::integrate_next`]（merge → 验证 → CAS）。
//!
//! 基础设施失败（System）重试口径（前置裁决④）：**第一次保留 `Pending` + 记一次重试**，
//! 第二次仍 System 才落 `NeedsRepair` + `validation_infra` 产物 + 清掉计数键
//! （避免每次基础设施抖动都写一份假质量记录）。**两条 System 路径走同一处置**：
//! 组合验证失败（`validate`）与已集成判定失败（`merge_base`，如目标 ref 不存在）——
//! 后者曾经直接冒出 `drain_project`（绕过侧车计数：候选永停 `Pending`、永不转修复队列）。
//! 计数存在侧车 `{plans_root}/integrations/{project}/retries.json`（键 `<task_id>:<frozen_sha>`）——
//! 不进 `queue.json`（旧 schema 不变，回滚安全）。
//!
//! 已知局限（登记不修）：本模块是**长窗口写方**（分钟级 load→integrate→save），
//! 期间经 API / tick 并发入队的候选可能被旧快照覆盖；错峰只挡 run 派发、挡不住入队。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

use super::settings::{IntegrationSettings, TARGET_REF};
use super::{Candidate, CandidateStatus, Integrator, IntegrationOutcome, MergeQueue};

/// 一轮 drain 的结构化结果（server 层据此发事件）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrainReport {
    /// 开关态（false = 本轮零动作）。
    pub enabled: bool,
    /// 本轮 rearm 的候选数（`NeedsRebase` → `Pending`）。
    pub rearmed: usize,
    /// 本轮消费结果（None = 没消费：开关关 / 队列无可消费候选）。
    pub consumed: Option<Consumed>,
}

/// 本轮消费结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Consumed {
    /// 已集成：目标 ref 经 CAS 前进到 merged_sha。
    Integrated {
        /// 候选来源任务。
        task_id: String,
        /// 合并后的 commit。
        merged_sha: String,
    },
    /// 候选已在目标 ref 历史里（merge-base 命中）：直接置 Integrated，未产生新 commit。
    AlreadyIntegrated {
        /// 候选来源任务。
        task_id: String,
        /// 判定命中的候选 commit。
        integrated_sha: String,
    },
    /// 修复队列：质量失败（断言红 / 编译失败 / 合并冲突）。
    NeedsRepair {
        /// 候选来源任务。
        task_id: String,
        /// 失败摘要（进 `NeedsRepair.reason`）。
        reason: String,
        /// 结构化失败产物路径（修复候选可引用）。
        artifact: PathBuf,
    },
    /// 基础设施失败：候选仍 `Pending`，本轮记一次重试。
    InfraRetry {
        /// 候选来源任务。
        task_id: String,
        /// 失败摘要。
        reason: String,
        /// 累计基础设施失败次数（1 = 还有一次重试机会）。
        infra_failures: u32,
    },
    /// 重排：目标在集成期间被推进（CAS expect 不符）。
    NeedsRebase {
        /// 候选来源任务。
        task_id: String,
        /// 目标当前指向。
        target_moved_to: String,
    },
}

/// 基础设施失败计数（侧车 `retries.json` 的值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RetryEntry {
    /// 累计基础设施失败次数。
    #[serde(default)]
    pub infra_failures: u32,
}

/// 消费一个项目的队列（每轮最多消费 1 个候选）。
///
/// `repos` = 任务 id → 目标仓路径（server 层从 store 快照取：任务工作区 = 项目登记路径）。
/// `validate` = 验证门（生产 = [`super::gate`]；测试 = 注入闭包，可确定性造绿/红/System）。
pub fn drain_project<'a, F>(
    settings: &IntegrationSettings,
    project: &str,
    repos: &'a HashMap<String, PathBuf>,
    plans_root: &'a Path,
    validate: F,
) -> Result<DrainReport>
where
    F: Fn(&Path) -> Result<()> + 'a,
{
    // ① 开关关 → 零动作（不读队列、不碰 git：不得产生任何集成 worktree 目录）。
    if !settings.auto_integrate_enabled {
        return Ok(DrainReport {
            enabled: false,
            rearmed: 0,
            consumed: None,
        });
    }

    // ② rearm：NeedsRebase → Pending（与 tick 同一口径）。
    let mut queue = MergeQueue::load(plans_root, project)?;
    let rearmed = queue
        .candidates
        .iter()
        .filter(|c| matches!(c.status, CandidateStatus::NeedsRebase { .. }))
        .count();
    super::rearm_needs_rebase(project, &mut queue);

    // ③ 取首个可消费的 Pending 候选。
    let Some((idx, cand, repo)) = next_consumable(&queue, repos) else {
        if rearmed > 0 {
            queue.save()?;
        }
        return Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: None,
        });
    };

    // ⑤ 集成器（merge → 验证 → CAS；已集成判定与它共用同一目标 ref 口径）。
    let mut integrator = Integrator {
        repo,
        target_ref: TARGET_REF.to_string(),
        plans_root,
        validate: Box::new(validate),
    };

    // ④ 已集成判定（merge-base 命中 = CAS 已成功但队列未落盘）。
    match already_integrated(repo, &cand.frozen_sha) {
        Ok(true) => {
            queue.candidates[idx].status = CandidateStatus::Integrated {
                integrated_sha: cand.frozen_sha.clone(),
            };
            queue.save()?;
            tracing::info!(
                component = "integration",
                event = "integration.already_integrated",
                project = %project,
                task_id = %cand.task_id,
                integrated_sha = %cand.frozen_sha,
                "候选已在目标 ref 历史里（未重复合并）"
            );
            return Ok(DrainReport {
                enabled: true,
                rearmed,
                consumed: Some(Consumed::AlreadyIntegrated {
                    task_id: cand.task_id.clone(),
                    integrated_sha: cand.frozen_sha.clone(),
                }),
            });
        }
        Ok(false) => {}
        // 目标 ref 不可读 = 基础设施故障（前置裁决④）：与 `integrate_next` 的 System
        // **同一条归因路径**——第一次保留 Pending + 记一次重试，第二次仍 System 才落
        // NeedsRepair（否则该路径会绕过侧车计数：候选永停 Pending，每轮只打一条失败日志）。
        Err(e) => {
            let ctx = InfraContext {
                project,
                plans_root,
                idx,
                cand: &cand,
                err: &e,
                stage: "merge_base",
                rearmed,
                integrator: &integrator,
            };
            return handle_infra_failure(ctx, &mut queue);
        }
    }

    // ⑥ 集成：merge → 验证 → CAS。
    match integrator.integrate_next(&mut queue) {
        Ok(IntegrationOutcome::Integrated { task_id, merged_sha }) => Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: Some(Consumed::Integrated {
                task_id,
                merged_sha,
            }),
        }),
        Ok(IntegrationOutcome::NeedsRepair {
            task_id,
            reason,
            artifact,
        }) => Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: Some(Consumed::NeedsRepair {
                task_id,
                reason,
                artifact,
            }),
        }),
        Ok(IntegrationOutcome::Requeued {
            task_id,
            target_moved_to,
        }) => Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: Some(Consumed::NeedsRebase {
                task_id,
                target_moved_to,
            }),
        }),
        Ok(IntegrationOutcome::Idle) => Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: None,
        }),
        // 基础设施失败：不动队列 + 记一次重试（重试耗尽才落 NeedsRepair）。
        Err(e) if matches!(e.root_cause(), DevforgeError::System(_)) => {
            let ctx = InfraContext {
                project,
                plans_root,
                idx,
                cand: &cand,
                err: &e,
                stage: "validate",
                rearmed,
                integrator: &integrator,
            };
            handle_infra_failure(ctx, &mut queue)
        }
        Err(e) => Err(e),
    }
}

/// 基础设施失败处置的入参（打包成结构体：散参数读不动，也撑爆参数上限）。
struct InfraContext<'a, 'q> {
    /// 项目名（队列与侧车按项目分目录）。
    project: &'a str,
    /// 台账根。
    plans_root: &'a Path,
    /// 候选在队列里的下标。
    idx: usize,
    /// 候选。
    cand: &'a Candidate,
    /// 失败原因。
    err: &'a DevforgeError,
    /// 失败环节（`validate` = 组合验证；`merge_base` = 已集成判定）——写进产物，
    /// 免得「validation_infra」这个 kind 把两个环节混成一样。
    stage: &'static str,
    /// 本轮 rearm 的候选数（>0 时队列状态变了，落盘要带上）。
    rearmed: usize,
    /// 集成器（写产物用同一口径）。
    integrator: &'a Integrator<'q>,
}

/// 基础设施失败处置：第一次保留 `Pending` 并记一次重试；第二次仍 System 才落修复队列。
fn handle_infra_failure(
    ctx: InfraContext<'_, '_>,
    queue: &mut MergeQueue,
) -> Result<DrainReport> {
    let InfraContext {
        project,
        plans_root,
        idx,
        cand,
        err,
        stage,
        rearmed,
        integrator,
    } = ctx;
    let key = retry_key(cand);
    let mut retries = load_retries(plans_root, project);
    let failures = retries.get(&key).map_or(0, |e| e.infra_failures) + 1;
    let reason = format!("集成基础设施失败：{err}");

    if failures == 1 {
        // 第一次：候选保持 Pending（本轮不改队列状态），只记一次重试。
        retries.insert(key, RetryEntry { infra_failures: failures });
        save_retries(plans_root, project, &retries)?;
        if rearmed > 0 {
            queue.save()?;
        }
        tracing::warn!(
            component = "integration",
            event = "integration.infra.retry",
            project = %project,
            task_id = %cand.task_id,
            infra_failures = failures,
            "集成基础设施失败（候选保持 Pending，重试一次仍失败才落修复队列）"
        );
        return Ok(DrainReport {
            enabled: true,
            rearmed,
            consumed: Some(Consumed::InfraRetry {
                task_id: cand.task_id.clone(),
                reason,
                infra_failures: failures,
            }),
        });
    }

    // 重试耗尽：落修复队列 + 结构化产物（沿用既有 NeedsRepair 产物口径）+ 清计数键。
    let artifact = integrator.write_artifact(
        project,
        cand,
        &serde_json::json!({
            "kind": "validation_infra",
            "attribution": "system",
            "stage": stage,
            "base_rev": cand.base_rev,
            "candidate": cand.frozen_sha,
            "error": reason,
            "infra_failures": failures,
        }),
    )?;
    queue.candidates[idx]
        .evidence_paths
        .push(artifact.to_string_lossy().to_string());
    queue.candidates[idx].status = CandidateStatus::NeedsRepair {
        reason: reason.clone(),
    };
    queue.save()?;
    retries.remove(&key);
    save_retries(plans_root, project, &retries)?;
    tracing::error!(
        component = "integration",
        event = "integration.infra.exhausted",
        project = %project,
        task_id = %cand.task_id,
        infra_failures = failures,
        "集成基础设施失败重试耗尽（候选转入修复队列）"
    );
    Ok(DrainReport {
        enabled: true,
        rearmed,
        consumed: Some(Consumed::NeedsRepair {
            task_id: cand.task_id.clone(),
            reason,
            artifact,
        }),
    })
}

/// 取首个可消费的 `Pending` 候选（仓库路径未知的跳过并留痕）。
fn next_consumable<'a>(
    queue: &MergeQueue,
    repos: &'a HashMap<String, PathBuf>,
) -> Option<(usize, Candidate, &'a Path)> {
    for (idx, c) in queue.candidates.iter().enumerate() {
        if !matches!(c.status, CandidateStatus::Pending) {
            continue;
        }
        match repos.get(&c.task_id) {
            Some(repo) => return Some((idx, c.clone(), repo.as_path())),
            None => {
                tracing::warn!(
                    component = "integration",
                    event = "integration.candidate.repo_unknown",
                    task_id = %c.task_id,
                    "候选缺目标仓路径（任务工作区未知）——本轮跳过（不留痕就会静默卡住队列）"
                );
            }
        }
    }
    None
}

/// 已集成判定：`git merge-base --is-ancestor <候选 sha> <目标 ref>`。
///
/// 退出码 0 = 是祖先；1 = 不是祖先；其余 = 目标 ref 不可读等错误 → System
/// （目标项目默认分支不是 master 时会走到这里——两次后才落 NeedsRepair）。
fn already_integrated(repo: &Path, frozen_sha: &str) -> Result<bool> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", frozen_sha, TARGET_REF])
        .output()
        .ctx("git 可执行")?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(DevforgeError::System(format!(
            "git merge-base 判定失败（目标 ref {TARGET_REF} 不可读）：{}",
            super::first_line(&String::from_utf8_lossy(&out.stderr))
        ))),
    }
}

/// 重试计数键（`<task_id>:<frozen_sha>`——同任务的新候选是新证据，计数不串味）。
fn retry_key(cand: &Candidate) -> String {
    format!("{}:{}", cand.task_id, cand.frozen_sha)
}

/// 侧车路径：`{plans_root}/integrations/{project}/retries.json`。
fn retry_path(plans_root: &Path, project: &str) -> PathBuf {
    plans_root.join("integrations").join(project).join("retries.json")
}

/// 读重试计数（文件不存在 = 无计数；读坏了 = 无计数 + WARN 留痕）。
fn load_retries(plans_root: &Path, project: &str) -> BTreeMap<String, RetryEntry> {
    let path = retry_path(plans_root, project);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<BTreeMap<String, RetryEntry>>(&bytes) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    component = "integration",
                    path = %path.display(),
                    error = %e,
                    "基础设施重试计数文件内容非法 → 按无计数处理"
                );
                BTreeMap::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => {
            tracing::warn!(
                component = "integration",
                path = %path.display(),
                error = %e,
                "基础设施重试计数文件读取失败 → 按无计数处理"
            );
            BTreeMap::new()
        }
    }
}

/// 写重试计数（原子写：半截文件会让下一次 load 判损坏）。
fn save_retries(
    plans_root: &Path,
    project: &str,
    retries: &BTreeMap<String, RetryEntry>,
) -> Result<()> {
    let path = retry_path(plans_root, project);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ctx("建重试计数目录")?;
    }
    let bytes = serde_json::to_vec_pretty(retries).ctx("序列化重试计数")?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    (|| -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).ctx("建重试计数临时文件")?;
        f.write_all(&bytes).ctx("写重试计数临时文件")?;
        f.sync_all().ctx("刷重试计数临时文件")?;
        std::fs::rename(&tmp, &path).ctx("原子替换重试计数文件")?;
        Ok(())
    })()
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
#[path = "consumer_tests.rs"]
mod consumer_tests;
