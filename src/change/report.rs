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
mod tests {
    use super::*;

    fn sample(acceptance: AcceptanceSummary) -> ChangeReport {
        ChangeReport {
            change_id: "change-20261020-x".into(),
            run_id: "run-1".into(),
            baseline_sha: "abc123".into(),
            wall_secs: 42,
            diff: DiffStats {
                files: 3,
                insertions: 10,
                deletions: 2,
            },
            review_rounds: 4,
            roundtrips: 1,
            usage: "1,234 tokens（in 1000 / out 234），成本 $0.0100".into(),
            acceptance,
        }
    }

    fn all_ok() -> AcceptanceSummary {
        AcceptanceSummary {
            total: 2,
            done: 2,
            missing: vec![],
        }
    }

    #[test]
    fn render_carries_every_required_field() {
        let text = sample(all_ok()).render();
        for anchor in [
            "change-20261020-x",
            "run-1",
            "abc123",
            "42s",
            "+10 / -2",
            "审查轮次: 4",
            "往返: 1",
            "tokens",
            "验收对账: 通过",
        ] {
            assert!(text.contains(anchor), "报告缺字段：{anchor}\n{text}");
        }
    }

    #[test]
    fn failed_reconciliation_is_visible_in_report() {
        let text = sample(AcceptanceSummary {
            total: 2,
            done: 1,
            missing: vec!["空输入报错".into()],
        })
        .render();
        assert!(text.contains("未通过"), "{text}");
        assert!(text.contains("缺 1 条"), "{text}");
        assert!(text.contains("## 未对账的验收标准"), "{text}");
        assert!(text.contains("空输入报错"), "{text}");
    }

    #[test]
    fn empty_criteria_is_reported_as_anomaly() {
        let text = sample(AcceptanceSummary {
            total: 0,
            done: 0,
            missing: vec![],
        })
        .render();
        assert!(text.contains("无验收标准"), "{text}");
    }

    #[test]
    fn summarize_counts_missing() {
        let criteria = vec!["a".to_string(), "b".to_string()];
        let done = vec!["a".to_string()];
        let s = summarize_acceptance(&criteria, &done);
        assert_eq!(s.total, 2);
        assert_eq!(s.done, 1);
        assert_eq!(s.missing, vec!["b".to_string()]);
        assert!(!s.passed());
    }

    #[test]
    fn diff_stats_reads_numstat() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for args in [vec!["init", "-q"], vec!["add", "-A"]] {
            let _ = Command::new("git").args(&args).current_dir(repo).output();
        }
        // 基线提交
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "add", "-A"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        let sha = String::from_utf8_lossy(
            &Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // 改动一个文件 + 新增一个（新增文件先入索引：coding 按步提交后 diff 才看得到它）
        std::fs::write(repo.join("a.txt"), "a\nb\nc\n").unwrap();
        std::fs::write(repo.join("b.txt"), "x\n").unwrap();
        let out = Command::new("git")
            .args(["add", "b.txt"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success());

        let stats = diff_stats(repo, &sha).unwrap();
        assert_eq!(stats.files, 2, "{stats:?}");
        assert_eq!(stats.insertions, 3, "{stats:?}");
        assert_eq!(stats.deletions, 0, "{stats:?}");
    }

    #[test]
    fn write_report_creates_directory_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_report(tmp.path(), "change-20261020-x", &sample(all_ok())).unwrap();
        assert!(path.is_file());
        assert!(path.ends_with("docs/plantree/changes/change-20261020-x/report.md"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("变更报告"));
    }
}
