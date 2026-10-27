//! 审计发现的数据模型与协议解析（P013 M1）。
//!
//! 协议（`<audit-findings>` JSON 数组，verdict 块家族）：每条发现强制
//! path:line 证据 + 视角 + 严重度主张（零证据不入，Google 标准）。

use serde::{Deserialize, Serialize};

/// 六视角（overview §八 视角矩阵）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Correctness,
    ErrorHandling,
    Observability,
    Architecture,
    Consistency,
    Security,
}

/// 四级严重度（Google：nit = 非阻塞打磨层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Critical,
    Major,
    Minor,
    Nit,
}

/// 一条审计发现。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// 仓库相对路径。
    pub path: String,
    /// 1-based 行号（核验器验证存在性与顺序合法性）。
    pub line: u32,
    pub view: View,
    pub severity: Severity,
    /// 一句话主张。
    pub claim: String,
    /// 证据与理由（path:line 指认）。
    pub evidence: String,
    /// 产出发现的批次（溯源：哪个单元审出来的）。
    #[serde(default)]
    pub batch: String,
}

/// 单批次发现数上限（全仓审计 m14）：LLM 可返回超大列表，报告层无 cap 会被刷爆。
/// 超过部分截断并 WARN（不升级为协议违例——截断后的发现仍然可用）。
pub const MAX_FINDINGS_PER_BATCH: usize = 100;

/// 一批发现的解析结果。
///
/// 自审 minor（2）：`truncated` 必须能进报告产物——旧口径超限只打一行 WARN，
/// 丢弃条数不进任何产物，「这次审计少看了 N 条」在报告里不可见。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedBatch {
    pub findings: Vec<Finding>,
    /// 超过单批上限被丢弃的条数（0 = 没截断）。
    pub truncated: usize,
}

/// 解析批次审查回复中的 `<audit-findings>` 块（verdict 块家族协议）。
///
/// 返回协议违例的**人读原因**（`Err(String)`）：块缺失/JSON 非法/字段非法。
/// 处置（审计修复 #11，注释对齐实现）：由 `stage1_review_batch` 自制 2 次循环换实例重审，
/// 再违例放弃该批次——不经 review_with_gate（实例失败保持原分类上抛打断整次，见 #1）。
///
/// 自审 minor（1）：返回 `String` 而非 `DevforgeError` 是**故意的分工**——协议违例在
/// 批次内部先重试，耗尽后统一升级成 `Quality`（见 pipeline::stage1_review_batch）；
/// 若这里直接给 Quality，调用链上就无法区分「重试一次就好」和「当场判死」。
pub fn parse_findings(reply: &str) -> Result<ParsedBatch, String> {
    let start = reply
        .find("<audit-findings>")
        .ok_or_else(|| "缺 <audit-findings> 协议块".to_string())?;
    let end = reply[start..]
        .find("</audit-findings>")
        .ok_or_else(|| "协议块未闭合（截断）".to_string())?;
    let body = &reply[start + "<audit-findings>".len()..start + end];
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(body.trim()).map_err(|e| format!("findings JSON 非法：{e}"))?;
    let mut findings = Vec::with_capacity(raw.len().min(MAX_FINDINGS_PER_BATCH));
    if raw.len() > MAX_FINDINGS_PER_BATCH {
        tracing::warn!(
            component = "audit-engine",
            total = raw.len(),
            cap = MAX_FINDINGS_PER_BATCH,
            "单批次发现数超限，已截断（m14）"
        );
    }
    for (i, v) in raw.iter().take(MAX_FINDINGS_PER_BATCH).enumerate() {
        // 自审 minor（455）：错误信息改 1 基序号——旧口径首条报「第 0 条」，
        // 排障时与「第 1 条」混淆。
        let n = i + 1;
        let obj = v.as_object().ok_or_else(|| format!("第 {n} 条非对象"))?;
        let get_str = |k: &str| -> Result<String, String> {
            obj.get(k)
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("第 {n} 条缺字段 {k}"))
        };
        let path = get_str("path")?;
        // 自审 minor（456）：协议层就要求**仓内相对路径**——旧口径只在下游核验器拦
        // 越界路径，幻觉路径晚一步暴露；这里直接判协议违例（重试换实例可能纠回来）。
        if !is_repo_relative(&path) {
            return Err(format!("第 {n} 条 path 不是仓内相对路径：{path}"));
        }
        let line = obj
            .get("line")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| format!("第 {n} 条缺合法 line"))?;
        // 全仓审计 m6：`as u32` 会把超大值静默回绕成合法小行号 → 直接拒为协议违例
        let line = u32::try_from(line).map_err(|_| format!("第 {n} 条 line 越界：{line}"))?;
        let view = serde_json::from_value(
            obj.get("view")
                .cloned()
                .ok_or_else(|| format!("第 {n} 条缺 view"))?,
        )
        .map_err(|e| format!("第 {n} 条 view 非法：{e}"))?;
        let severity = serde_json::from_value(
            obj.get("severity")
                .cloned()
                .ok_or_else(|| format!("第 {n} 条缺 severity"))?,
        )
        .map_err(|e| format!("第 {n} 条 severity 非法：{e}"))?;
        findings.push(Finding {
            path,
            line,
            view,
            severity,
            claim: get_str("claim")?,
            evidence: get_str("evidence")?,
            batch: String::new(),
        });
    }
    Ok(ParsedBatch {
        truncated: raw.len().saturating_sub(MAX_FINDINGS_PER_BATCH),
        findings,
    })
}

/// path 必须是仓内相对路径：非空、不是绝对路径、无 `..` 段、无盘符。
fn is_repo_relative(path: &str) -> bool {
    let p = path.trim();
    if p.is_empty() || p.starts_with('/') || p.starts_with('\\') {
        return false;
    }
    // Windows 盘符（`C:`）与协议前缀（`file:`）一并拒掉
    if p.len() >= 2 && p.as_bytes()[1] == b':' {
        return false;
    }
    std::path::Path::new(p)
        .components()
        .all(|c| !matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir))
}

#[cfg(test)]
#[path = "finding_tests.rs"]
mod tests;
