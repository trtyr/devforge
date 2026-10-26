//! 变更报告（P009 T012）：**引擎写，不派 agent**。
//!
//! 报告只放可复算的东西：墙钟、diff 统计、审查轮次、往返次数、用量（会话账汇总）、验收对账结果。
//! 对账没过时报告里必须一眼看出来——报告不能报喜不报忧。

use std::path::Path;
use std::process::Command;

use crate::error::{DevforgeError, Result};

/// diff 统计（对基线）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// 验收对账结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceSummary {
    pub total: usize,
    pub done: usize,
    pub missing: Vec<String>,
}

impl AcceptanceSummary {
    pub fn passed(&self) -> bool {
        self.missing.is_empty() && self.total > 0
    }
}

/// 报告内容（全部由引擎采集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeReport {
    pub change_id: String,
    pub run_id: String,
    pub baseline_sha: String,
    pub wall_secs: u64,
    pub diff: DiffStats,
    pub review_rounds: usize,
    pub roundtrips: usize,
    pub usage: String,
    pub acceptance: AcceptanceSummary,
}

impl ChangeReport {
    /// 渲染成 markdown（写 `docs/plantree/changes/<id>/report.md`）。
    pub fn render(&self) -> String {
        let verdict = if self.acceptance.passed() {
            "通过".to_string()
        } else if self.acceptance.total == 0 {
            "无验收标准（异常）".to_string()
        } else {
            format!(
                "未通过：{} / {} 条对账通过，缺 {} 条",
                self.acceptance.done,
                self.acceptance.total,
                self.acceptance.missing.len()
            )
        };
        let mut out = format!(
            "# 变更报告 · {}\n\n\
- run: {}\n\
- 基线: `{}`\n\
- 墙钟: {}s\n\
- diff: {} 个文件（+{} / -{}）\n\
- 审查轮次: {}\n\
- plan↔coding 往返: {}\n\
- 用量: {}\n\
- 验收对账: {}\n",
            self.change_id,
            self.run_id,
            self.baseline_sha,
            self.wall_secs,
            self.diff.files,
            self.diff.insertions,
            self.diff.deletions,
            self.review_rounds,
            self.roundtrips,
            self.usage,
            verdict
        );
        if !self.acceptance.missing.is_empty() {
            out.push_str("\n## 未对账的验收标准\n\n");
            for m in &self.acceptance.missing {
                out.push_str(&format!("- {m}\n"));
            }
        }
        out
    }
}

/// `git diff --numstat <baseline>` → 统计（只看工作区相对基线的改动）。
pub fn diff_stats(repo: &Path, baseline: &str) -> Result<DiffStats> {
    let out = Command::new("git")
        .args(["diff", "--numstat", baseline])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git diff 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git diff --numstat {baseline} 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let mut stats = DiffStats::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split('\t');
        let add = parts.next().unwrap_or("");
        let del = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        if path.is_empty() {
            continue;
        }
        stats.files += 1;
        // 二进制文件是 "-"，不计增删行
        stats.insertions += add.parse::<usize>().unwrap_or(0);
        stats.deletions += del.parse::<usize>().unwrap_or(0);
    }
    // 自审 major：numstat 不含未跟踪文件——coding 产出的新文件曾让报告写成
    // 「diff: 0 个文件（+0 / -0）」。untracked（豁免目录外）逐个计入，行数按实读计。
    let out = Command::new("git")
        .args(["-c", "core.quotePath=false", "status", "--porcelain", "-uall"])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git status 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git status 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some(rest) = line.strip_prefix("?? ") else {
            continue;
        };
        let path = rest.trim();
        if path.starts_with("tests/") || path.starts_with("docs/") || path.starts_with("evidence/") {
            continue;
        }
        stats.files += 1;
        stats.insertions += std::fs::read_to_string(repo.join(path))
            .map(|t| t.lines().count())
            .unwrap_or(0);
    }
    Ok(stats)
}

/// 汇总验收对账（`criteria` 全部、`done` 已通过）。
pub fn summarize_acceptance(criteria: &[String], done: &[String]) -> AcceptanceSummary {
    let missing: Vec<String> = criteria
        .iter()
        .filter(|c| !done.iter().any(|d| d == *c))
        .cloned()
        .collect();
    AcceptanceSummary {
        total: criteria.len(),
        done: criteria.len() - missing.len(),
        missing,
    }
}

/// 写报告到目标仓（目录不存在先建）。
pub fn write_report(repo: &Path, change_id: &str, report: &ChangeReport) -> Result<std::path::PathBuf> {
    let dir = repo.join("docs/plantree/changes").join(change_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", dir.display())))?;
    let path = dir.join("report.md");
    std::fs::write(&path, report.render())
        .map_err(|e| DevforgeError::System(format!("写报告 {} 失败：{e}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
