//! 发现核验器（P013 M2 · T2.1，确定性零 LLM）。
//!
//! RepoAudit 教训：无 validator 时 FP +245%。两道验证：
//! ① path:line 存在性（发现引用的位置必须真实存在）；
//! ② 行号顺序合法性（同一发现内的声明/引用行号关系不倒置——解析式，零 LLM）。
//! 幽灵发现丢弃并计数（「先验证再交付」抗噪核心）。

use super::finding::Finding;
use std::collections::BTreeMap;
use std::path::Path;

/// 核验结果：放行的发现 + 被丢弃的幽灵（带原因，供报告统计）。
#[derive(Debug, Default, PartialEq)]
pub struct VerifyReport {
    pub kept: Vec<Finding>,
    /// (finding 摘要, 丢弃原因)
    pub dropped: Vec<(String, String)>,
}

/// 对一批发现跑核验：`workspace` = 仓库根（存在性判定的基准）。
pub fn verify(workspace: &Path, findings: &[Finding]) -> VerifyReport {
    // 行号缓存：path → 行数（同一文件的多次发现只读一次盘）
    let mut line_counts: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut report = VerifyReport::default();
    for f in findings {
        let lines = line_counts
            .entry(f.path.clone())
            .or_insert_with(|| {
                std::fs::read_to_string(workspace.join(&f.path))
                    .map(|s| s.lines().count())
                    .ok()
            });
        let Some(total) = lines else {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!("幽灵发现：文件 {} 不存在", f.path),
            ));
            continue;
        };
        if *total == 0 || f.line as usize > *total {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!(
                    "行号越界：{} 共 {total} 行，发现指认第 {} 行",
                    f.path, f.line
                ),
            ));
            continue;
        }
        // ② 顺序合法性（evidence 里的引用行号不得全部大于文件行数——
        //    解析 evidence 中的 `path:line` 引用并查存在性）
        if let Some(bad) = evidence_refs_out_of_file(workspace, &f.evidence) {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!("证据引用越界：{bad}"),
            ));
            continue;
        }
        report.kept.push(f.clone());
    }
    report
}

/// 证据文本中的 `xx.rs:NN` 引用核验：引用的文件必须存在且行号在界内。
/// 返回第一条越界引用的描述（无越界 = None）。
fn evidence_refs_out_of_file(workspace: &Path, evidence: &str) -> Option<String> {
    let bytes = evidence.as_bytes();
    let mut i = 0usize;
    while let Some(pos) = evidence[i..].find(':') {
        let abs = i + pos;
        // 向前找路径段（字母数字/_/./- 组成的非空段），向后找行号
        let head = &evidence[..abs];
        let path_start = head
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-' || *c == '/'))
            .map(|(idx, _)| idx + 1)
            .unwrap_or(0);
        let path = &evidence[path_start..abs];
        let tail = &evidence[abs + 1..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        i = abs + 1;
        if path.is_empty() || digits.is_empty() {
            continue;
        }
        // 启发式过滤：路径段要像文件（含扩展名或目录分隔），行号 ≤ 6 位防时间戳误吞
        if !(path.contains('.') || path.contains('/')) || digits.len() > 6 {
            continue;
        }
        let full = workspace.join(path);
        let line_no: usize = match digits.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        match std::fs::read_to_string(&full) {
            Ok(content) => {
                let total = content.lines().count();
                if line_no == 0 || line_no > total {
                    return Some(format!(
                        "{path}:{line_no}（该文件共 {total} 行）"
                    ));
                }
            }
            Err(_) => {
                return Some(format!("{path}:{line_no}（文件不存在）"));
            }
        }
        let _ = bytes;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::finding::{Severity, View};

    fn finding(path: &str, line: u32) -> Finding {
        Finding {
            path: path.into(),
            line,
            view: View::Correctness,
            severity: Severity::Major,
            claim: "测试发现".into(),
            evidence: format!("{path}:{line} 处存在问题"),
            batch: "unit-001".into(),
        }
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/real.rs"), "line1\nline2\nline3\n").unwrap();
        tmp
    }

    #[test]
    fn ghost_file_findings_are_dropped() {
        let r = fixture();
        let findings = vec![finding("src/ghost.rs", 5), finding("src/real.rs", 2)];
        let v = verify(r.path(), &findings);
        assert_eq!(v.kept.len(), 1);
        assert_eq!(v.kept[0].path, "src/real.rs");
        assert_eq!(v.dropped.len(), 1);
        assert!(v.dropped[0].1.contains("不存在"), "{}", v.dropped[0].1);
    }

    #[test]
    fn out_of_range_line_is_dropped() {
        let r = fixture();
        let findings = vec![finding("src/real.rs", 99)];
        let v = verify(r.path(), &findings);
        assert!(v.kept.is_empty());
        assert!(v.dropped[0].1.contains("行号越界"), "{}", v.dropped[0].1);
    }

    #[test]
    fn valid_findings_pass_with_ghost_counting() {
        let r = fixture();
        let findings = vec![
            finding("src/real.rs", 1),
            finding("src/real.rs", 3),
            finding("src/nope.rs", 1),
        ];
        let v = verify(r.path(), &findings);
        assert_eq!(v.kept.len(), 2);
        assert_eq!(v.dropped.len(), 1);
    }

    #[test]
    fn evidence_ref_to_nonexistent_file_is_dropped() {
        let r = fixture();
        let mut f = finding("src/real.rs", 2);
        f.evidence = "参照 src/ghost.rs:1 的实现".into();
        let v = verify(r.path(), &[f]);
        assert!(v.kept.is_empty(), "证据引用幽灵文件应整条丢弃");
        assert!(v.dropped[0].1.contains("文件不存在"), "{}", v.dropped[0].1);
    }
}
