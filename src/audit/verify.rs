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

/// 路径穿越静态拒绝（审计修复 #7）：绝对路径或含 `..` 段的发现路径不读盘直接丢。
/// 防 `workspace.join` 对绝对路径的整体替换（`/w".join("/etc/passwd")` = `/etc/passwd`）
/// 与 `../` 爬出仓库——核验器只做存在性检查，不能被当成任意路径读盘器。
fn path_traversal(p: &str) -> bool {
    std::path::Path::new(p).is_absolute() || p.split('/').any(|seg| seg == "..")
}

/// evidence 中是否含 `path:line` 形态引用（审计修复 #4：零证据不入的判定基础）。
/// 解析启发式与 [`evidence_refs_out_of_file`] 同源：路径段（含扩展名或目录分隔）+ 纯数字行号。
fn has_evidence_ref(evidence: &str) -> bool {
    let mut i = 0usize;
    while let Some(pos) = evidence[i..].find(':') {
        let abs = i + pos;
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
        if !path.is_empty()
            && !digits.is_empty()
            && (path.contains('.') || path.contains('/'))
        {
            return true;
        }
    }
    false
}

/// 对一批发现跑核验：`workspace` = 仓库根（存在性判定的基准）。
pub fn verify(workspace: &Path, findings: &[Finding]) -> VerifyReport {
    // 行号缓存：path → 行数（同一文件的多次发现只读一次盘）
    let mut line_counts: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut report = VerifyReport::default();
    for f in findings {
        // 审计修复 #7：路径穿越先拒绝（不读盘）
        if path_traversal(&f.path) {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!("路径越界：{} 含 .. 或绝对路径，拒绝读盘", f.path),
            ));
            continue;
        }
        // 审计修复 #4：兑现「零证据不入」（协议提示词约定，M0 起推给核验器至今未实现）——
        // evidence 空或无 path:line 引用的发现直接丢弃（在读盘之前，省 I/O）。
        if f.evidence.trim().is_empty() || !has_evidence_ref(&f.evidence) {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                "零证据：evidence 为空或无 path:line 引用".to_string(),
            ));
            continue;
        }
        let lines = line_counts
            .entry(f.path.clone())
            .or_insert_with(|| read_within(workspace, &f.path).map(|s| s.lines().count()));
        let Some(total) = lines else {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!("幽灵发现：文件 {} 不存在", f.path),
            ));
            continue;
        };
        // 审计修复 #9：line 0 与越界同样丢弃（1-based 约定下 0 必非法，与 evidence 分支对齐）
        if *total == 0 || f.line == 0 || f.line as usize > *total {
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

/// 全仓审计 m11：仓内读取的符号链接**不得越界**——canonicalize 后必须仍在 workspace 下。
/// 旧实现只挡绝对路径与 `..`，仓内 `link -> /etc` 仍会被读（只取行数，无内容外泄，
/// 但核验器不该成为任意路径读盘器）。
fn read_within(workspace: &Path, rel: &str) -> Option<String> {
    let root = workspace.canonicalize().ok()?;
    let target = workspace.join(rel).canonicalize().ok()?;
    if !target.starts_with(&root) {
        tracing::warn!(
            component = "audit-engine",
            path = %rel,
            resolved = %target.display(),
            "核验路径经符号链接越出仓库，拒绝读取"
        );
        return None;
    }
    std::fs::read_to_string(&target).ok()
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
        // 审计修复 #7：证据引用路径同样拒绝穿越（不读盘，整条丢弃）
        if path_traversal(path) {
            return Some(format!("{path}:路径越界（.. 或绝对路径，拒绝读盘）"));
        }
        let line_no: usize = match digits.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        // 第三方 r2 m5：证据引用读盘也走 read_within——旧实现只拦绝对路径与 `..`，
        // 仓内符号链接指向仓外仍会打开（主 path 已是前缀狱，这里漏了）。
        match read_within(workspace, path) {
            Some(content) => {
                let total = content.lines().count();
                if line_no == 0 || line_no > total {
                    return Some(format!(
                        "{path}:{line_no}（该文件共 {total} 行）"
                    ));
                }
            }
            None => {
                return Some(format!(
                    "{path}:{line_no}（文件不存在或经符号链接越出仓库，拒绝读取）"
                ));
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

    /// 审计修复 #7：路径穿越（../ 或绝对路径）不读盘直接丢弃，原因标「越界」非「不存在」
    /// ——核验器只做存在性检查，不能被当成任意路径读盘器。
    #[test]
    fn traversal_paths_are_rejected_without_disk_read() {
        let r = fixture();
        let findings = vec![finding("../../etc/passwd", 1), finding("/etc/passwd", 1)];
        let v = verify(r.path(), &findings);
        assert!(v.kept.is_empty(), "穿越路径不得进入 kept");
        assert_eq!(v.dropped.len(), 2);
        for (loc, reason) in &v.dropped {
            assert!(reason.contains("路径越界"), "{loc}: {reason}");
            assert!(!reason.contains("不存在"), "应静态拒绝而非当幽灵：{reason}");
        }
    }

    /// 审计修复 #9：行号 0 与越界同样丢弃（1-based 约定下 0 必非法）。
    #[test]
    fn zero_line_is_dropped() {
        let r = fixture();
        let v = verify(r.path(), &[finding("src/real.rs", 0)]);
        assert!(v.kept.is_empty(), "line=0 不得进 kept");
        assert!(v.dropped[0].1.contains("行号越界"), "{}", v.dropped[0].1);
    }

    /// 审计修复 #4：兑现「零证据不入」——空 evidence 或无 path:line 引用的散文
    /// 直接丢弃（M0 起推给核验器至今未实现，结构合法就能进报告的口子被关上）。
    #[test]
    fn empty_or_referenceless_evidence_is_dropped() {
        let r = fixture();
        let mut no_evidence = finding("src/real.rs", 1);
        no_evidence.evidence = "   ".into();
        let mut prose_only = finding("src/real.rs", 2);
        prose_only.evidence = "看起来这里有些问题，建议检查一下".into(); // 无任何 path:line 引用
        let v = verify(r.path(), &[no_evidence, prose_only]);
        assert!(v.kept.is_empty(), "零证据发现不得进 kept：{:?}", v.kept);
        assert_eq!(v.dropped.len(), 2);
        for (_, reason) in &v.dropped {
            assert!(reason.contains("零证据"), "{reason}");
        }
    }
}
