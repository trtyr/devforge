//! 发现核验器（P013 M2 · T2.1，确定性零 LLM）。
//!
//! RepoAudit 教训：无 validator 时 FP +245%。实际实现的三道验证（2026-10-27 自审 minor 34：
//! 旧文档写的「行号顺序合法性（声明/引用行号不倒置）」根本不存在，那段代码是行号越界检查）：
//! ① 零证据 / 证据指错位置：evidence 必须为空以外、且含指回本条 path 的 `path:line`；
//! ② 位置存在性：path 在仓内可读、line 在 1..=总行数 内（不是「顺序合法性」）；
//! ③ 引用原文：目标行（≥8 字符）必须被 evidence 在 `path:line` 相邻窗口内引用。
//! 幽灵发现丢弃并计数（「先验证再交付」抗噪核心）。

use super::finding::Finding;
use std::collections::BTreeMap;
use std::path::Path;

/// 核验结果：放行的发现 + 被丢弃的幽灵（带原因，供报告统计）。
#[derive(Debug, Default, PartialEq)]
pub struct VerifyReport {
    pub kept: Vec<Finding>,
    /// (位置串 `path:line`, 丢弃原因)
    ///
    /// 自审 minor 10：旧文档写「finding 摘要」，实际存的是位置串——与报告「位置」列同口径。
    pub dropped: Vec<(String, String)>,
}

/// 路径穿越静态拒绝（审计修复 #7）：绝对路径或含 `..` 段的发现路径不读盘直接丢。
/// 防 `workspace.join` 对绝对路径的整体替换（`/w".join("/etc/passwd")` = `/etc/passwd`）
/// 与 `../` 爬出仓库——核验器只做存在性检查，不能被当成任意路径读盘器。
fn path_traversal(p: &str) -> bool {
    std::path::Path::new(p).is_absolute() || p.split('/').any(|seg| seg == "..")
}

/// 空白压缩（证据与原文比对前统一口径：换行/缩进不计）。
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 目标行可引用的前缀（空白压缩后取前 40 字符）。
///
/// 自审 minor 8：旧口径把「行短于 8 字符」整条豁免——短结构行（`}`、`});`）上的发现
/// 可以与所指位置毫无关系地放行，「读过再报」底线对这类行完全失效。现在短行照样要求
/// 引用，只是**必须与 `path:line` 相邻**（`quoted_next_to_ref`），因此高频短串也不足以蒙过。
fn line_prefix(line: Option<&str>) -> Option<String> {
    let t = line?.trim();
    if t.is_empty() {
        return None;
    }
    Some(normalize_ws(&t.chars().take(40).collect::<String>()))
}

/// 自审 AU-01（P012）：evidence 中出现 `path:line` 字面组合，且该组合前后各 200 字符
/// 窗口内出现目标行前缀——引用与被引原文紧邻才算「真的引了这一行」。
fn quoted_next_to_ref(hay: &str, path: &str, line: u32, prefix: &str) -> bool {
    let needle = format!("{path}:{line}");
    let mut from = 0usize;
    while let Some(pos) = hay[from..].find(&needle) {
        let abs = from + pos;
        let start = abs.saturating_sub(200);
        let end = (abs + needle.len() + 200).min(hay.len());
        if hay[start..end].contains(prefix) {
            return true;
        }
        from = abs + needle.len();
    }
    false
}

/// 解析 evidence 里的全部 `path:line` 引用。
///
/// **单一实现**（自审 minor 35：旧口径有一份复制体且已漂移——6 位行号上限与穿越检查
/// 只在其中一份里，导致「零证据」闸门放行的引用可以永远不被行号核验）。
/// 返回 (path, line)；行号 > 6 位（时间戳/版本号）与不像文件的段一律不认。
fn evidence_refs(evidence: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(pos) = evidence[i..].find(':') {
        let abs = i + pos;
        let head = &evidence[..abs];
        let path_start = head
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-' || *c == '/'))
            // 多字节分隔符（如全角括号）：idx+1 会切在字符中间 → panic
            .map(|(idx, c)| idx + c.len_utf8())
            .unwrap_or(0);
        let path = &evidence[path_start..abs];
        let tail = &evidence[abs + 1..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        i = abs + 1;
        if path.is_empty()
            || digits.is_empty()
            || !(path.contains('.') || path.contains('/'))
            || digits.len() > 6
        {
            continue;
        }
        if let Ok(line) = digits.parse::<usize>() {
            out.push((path.to_string(), line));
        }
    }
    out
}

/// evidence 中是否含 `path:line` 形态引用（零证据闸门的判定基础）。
fn has_evidence_ref(evidence: &str) -> bool {
    !evidence_refs(evidence).is_empty()
}

/// 引用路径是否指回发现自身位置（自审 major）：`./` 归一后全等，或引用是发现路径的
/// 段后缀（容忍仓根外层冗余前缀）。
fn ref_matches_path(ref_path: &str, finding_path: &str) -> bool {
    fn norm(p: &str) -> &str {
        p.trim_start_matches("./")
    }
    let (r, f) = (norm(ref_path), norm(finding_path));
    r == f || r.ends_with(&format!("/{f}"))
}

/// 对一批发现跑核验：`workspace` = 仓库根（存在性判定的基准）。
pub fn verify(workspace: &Path, findings: &[Finding]) -> VerifyReport {
    // 行缓存：path → 文件全文行（同一文件的多次发现只读一次盘）
    // 自审 minor（11）：缓存对**证据引用**同样生效（旧口径只缓存主 path，证据引用每次
    // 重新读盘 + 重新 canonicalize 工作区根）。值含失败分类，失败也缓存。
    let mut cache: BTreeMap<String, Result<Vec<String>, ReadError>> = BTreeMap::new();
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
        // 自审 major：证据至少有一处引用指回发现自身的位置——旧实现允许 evidence
        // 全部指向别的文件，「读过再报」对跨文件引用失效（短行豁免引用原文，
        // 但不豁免引用自己的位置）。
        if !evidence_refs(&f.evidence)
            .iter()
            .any(|(p, _)| ref_matches_path(p, &f.path))
        {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                "证据指错位置：evidence 中没有任何引用指向本条 path".to_string(),
            ));
            continue;
        }
        if !cache.contains_key(&f.path) {
            let loaded = read_within(workspace, &f.path)
                .map(|s| s.lines().map(|l| l.to_string()).collect::<Vec<String>>());
            cache.insert(f.path.clone(), loaded);
        }
        let file_lines = match cache.get(&f.path) {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                // 自审 minor（36）：区分「路径幻觉」与「读不到」——基础设施故障不该被
                // 记成 agent 编了路径。
                report.dropped.push((
                    format!("{}/{}", f.path, f.line),
                    e.describe(&f.path),
                ));
                continue;
            }
            None => continue,
        };
        // 审计修复 #9：line 0 与越界同样丢弃（1-based 约定下 0 必非法，与 evidence 分支对齐）
        let total = file_lines.len();
        if total == 0 || f.line == 0 || f.line as usize > total {
            report.dropped.push((
                format!("{}/{}", f.path, f.line),
                format!(
                    "行号越界：{} 共 {total} 行，发现指认第 {} 行",
                    f.path, f.line
                ),
            ));
            continue;
        }
        // 第三方 r3 遗留：**证据必须引用目标行原文**——旧核验器只验 path:line 存在，
        // 猜一个真实行号就能进报告（自审「从未真正读过代码」的最后一环）。
        // 判据取目标行的前 40 字符前缀（空白压缩后包含），够宽松不卡正常引用，
        // 但猜行号的人不可能正好凑中原文。行太短（<8 字符，如 `}`）不强制。
        // 自审 AU-01（P012）：前缀 contains 只证「evidence 里出现过这个开头」，
        // 高频前缀（use crate::…/let …）可借任意同前缀行通过，且不要求引用的就是
        // 指认行。加一档：evidence 必须出现 `path:line` 字面组合，且该组合前后窗口
        // 内出现目标行前缀——引用与原文紧邻，杜绝「引别处 + 借前缀」。
        if let Some(prefix) = line_prefix(file_lines.get(f.line as usize - 1).map(|s| s.as_str())) {
            let hay = normalize_ws(&f.evidence);
            let quoted = hay.contains(&prefix)
                && quoted_next_to_ref(&hay, &f.path, f.line, &prefix);
            if !quoted {
                report.dropped.push((
                    format!("{}/{}", f.path, f.line),
                    format!("未引用原文：evidence 未出现第 {} 行的代码（疑猜行号，前缀={prefix}）", f.line),
                ));
                continue;
            }
        }
        // ② 顺序合法性（evidence 里的引用行号不得全部大于文件行数——
        //    解析 evidence 中的 `path:line` 引用并查存在性）
        if let Some(bad) = evidence_refs_out_of_file(workspace, &f.evidence, &mut cache) {
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
fn read_within(workspace: &Path, rel: &str) -> Result<String, ReadError> {
    let root = workspace.canonicalize().map_err(|e| ReadError::Io(e.to_string()))?;
    let target = match workspace.join(rel).canonicalize() {
        Ok(t) => t,
        // 自审 minor（36）：读盘失败不再一律写成「文件不存在」——路径幻觉（NotFound）与
        // 基础设施故障（权限/非 UTF-8/目标是目录）必须可区分。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ReadError::NotFound),
        Err(e) => return Err(ReadError::Io(e.to_string())),
    };
    if !target.starts_with(&root) {
        tracing::warn!(
            component = "audit-engine",
            path = %rel,
            resolved = %target.display(),
            "核验路径经符号链接越出仓库，拒绝读取"
        );
        return Err(ReadError::OutsideWorkspace);
    }
    std::fs::read_to_string(&target).map_err(|e| ReadError::Io(e.to_string()))
}

/// 读盘失败分类（自审 minor 36）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadError {
    NotFound,
    OutsideWorkspace,
    Io(String),
}

impl ReadError {
    fn describe(&self, rel: &str) -> String {
        match self {
            ReadError::NotFound => format!("幽灵发现：文件 {rel} 不存在"),
            ReadError::OutsideWorkspace => {
                format!("拒绝读取：{rel} 经符号链接越出仓库")
            }
            ReadError::Io(e) => format!("读取 {rel} 失败（基础设施故障，非路径幻觉）：{e}"),
        }
    }
}

/// 证据文本中的 `xx.rs:NN` 引用核验：引用的文件必须存在且行号在界内。
/// 返回第一条越界引用的描述（无越界 = None）。
fn evidence_refs_out_of_file(
    workspace: &Path,
    evidence: &str,
    cache: &mut BTreeMap<String, Result<Vec<String>, ReadError>>,
) -> Option<String> {
    // 自审 minor（12）：旧口径这里有一段取了从不使用的字节切片（靠 `let _ = bytes;` 消音）
    let mut i = 0usize;
    while let Some(pos) = evidence[i..].find(':') {
        let abs = i + pos;
        // 向前找路径段（字母数字/_/./- 组成的非空段），向后找行号
        let head = &evidence[..abs];
        let path_start = head
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-' || *c == '/'))
            // 多字节分隔符：同上，不能 +1
            .map(|(idx, c)| idx + c.len_utf8())
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
        // 审计修复 #7：证据引用路径同样拒绝穿越——但**只跳过这条引用**，不判死整条
        // 发现（自审 minor 38：evidence 里 URL 端口、semver、仓外绝对路径这类次要引用
        // 都会把整条发现判死，理由还写成「路径越界」）。穿越引用直接忽略。
        if path_traversal(path) {
            tracing::debug!(
                component = "audit-engine",
                ref_path = %path,
                "证据里的仓外/穿越引用已忽略（不影响本条发现）"
            );
            continue;
        }
        let line_no: usize = match digits.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        // 第三方 r2 m5：证据引用读盘也走 read_within——旧实现只拦绝对路径与 `..`，
        // 仓内符号链接指向仓外仍会打开（主 path 已是前缀狱，这里漏了）。
        if !cache.contains_key(path) {
            let loaded = read_within(workspace, path)
                .map(|s| s.lines().map(|l| l.to_string()).collect::<Vec<String>>());
            cache.insert(path.to_string(), loaded);
        }
        match cache.get(path) {
            Some(Ok(lines)) => {
                let total = lines.len();
                if line_no == 0 || line_no > total {
                    return Some(format!(
                        "{path}:{line_no}（该文件共 {total} 行）"
                    ));
                }
            }
            Some(Err(ReadError::NotFound)) => {
                // 自审 minor（38）：引用的次要文件不存在同样只跳过（warn），不判死——
                // 真实原因与「路径越界」完全不同。
                tracing::debug!(
                    component = "audit-engine",
                    ref_path = %path,
                    "证据引用的文件不存在，忽略该引用"
                );
                continue;
            }
            Some(Err(e)) => {
                return Some(format!("{path}:{line_no}（{}）", e.describe(path)));
            }
            None => continue,
        }
    }
    None
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
