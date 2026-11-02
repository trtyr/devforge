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
    // 自审 minor（20）：外部传入的基线串直接作为 git 参数 = 选项注入面（`--output=/x`
    // 之类可让本命令写文件/改变输出）。先做形状校验，再用 `--` 端选项分隔符收口。
    let ok = (7..=40).contains(&baseline.len()) && baseline.bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(DevforgeError::Config(format!(
            "diff 基线形状非法：`{baseline}`（必须是 7..=40 位十六进制 commit，不能含选项前缀）"
        )));
    }
    let out = Command::new("git")
        .args(["diff", "--numstat", baseline, "--"])
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
        // R9-P85：「不算产品改动」统一权威 = testing 门 is_exempt_path（段级
        // tests/docs/evidence + 源码形态不豁免，R4-P49 裁决）——旧纯根前缀丢弃
        // 三宗罪：①docs/ 下源码（docs/app.rb）从统计消失；②src/evidence/data.json
        // （段级豁免位）反而进统计；③与 testing 门同词不同答。
        if crate::change::testing_gate::is_exempt_path(path) {
            continue;
        }
        stats.files += 1;
        stats.insertions += std::fs::read_to_string(repo.join(path))
            .map(|t| t.lines().count())
            .unwrap_or(0);
    }
    Ok(stats)
}

/// R9-P81：验收条款对账归一化——跨 agent 手抄的条款文本不做逐字节比较。
/// 计划 agent 写「**给定 x=1** → 返回 2」、test agent 手抄「给定 x=1 → 返回 2」，
/// 格式标记（粗体/反引号）与空白差异不该等价「这条没验」（缺一条即 Quality 判死）。
/// 归一范围：剥格式标记（`**粗体**/\`代码\`/__下划线__`）+ 空白折叠；
/// **不做标点全半角归一**（中文计划可能有意区分全半角）；对称函数，两侧同处理。
fn normalize_criterion(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for ch in s.trim().chars() {
        if matches!(ch, '`' | '*' | '_') {
            continue; // 格式标记剥离
        }
        let ws = ch.is_whitespace();
        if ws && last_ws {
            continue; // 空白折叠
        }
        out.push(ch);
        last_ws = ws;
    }
    out.trim().to_string()
}

/// 汇总验收对账（`criteria` 全部、`done` 已通过）。
///
/// R9-P81：比较走 [`normalize_criterion`] 归一化（格式标记/空白差异不判死），
/// 内容真不同才缺条。
pub fn summarize_acceptance(criteria: &[String], done: &[String]) -> AcceptanceSummary {
    let missing: Vec<String> = criteria
        .iter()
        .filter(|c| !done.iter().any(|d| normalize_criterion(d) == normalize_criterion(c)))
        .cloned()
        .collect();
    AcceptanceSummary {
        total: criteria.len(),
        done: criteria.len() - missing.len(),
        missing,
    }
}

/// 写报告到目标仓（目录不存在先建）。
pub fn write_report(change_dir: &Path, report: &ChangeReport) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(change_dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", change_dir.display())))?;
    let path = change_dir.join("report.md");
    std::fs::write(&path, report.render())
        .map_err(|e| DevforgeError::System(format!("写报告 {} 失败：{e}", path.display())))?;
    Ok(path)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
