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

/// 解析批次审查回复中的 `<audit-findings>` 块（verdict 块家族协议）。
///
/// 返回 `(findings, protocol_error)`：块缺失/JSON 非法/字段非法 → 协议违例。
/// 处置（审计修复 #11，注释对齐实现）：由 `stage1_review_batch` 自制 2 次循环换实例重审，
/// 再违例放弃该批次——不经 review_with_gate（实例失败保持原分类上抛打断整次，见 #1）。
pub fn parse_findings(reply: &str) -> Result<Vec<Finding>, String> {
    let start = reply
        .find("<audit-findings>")
        .ok_or_else(|| "缺 <audit-findings> 协议块".to_string())?;
    let end = reply[start..]
        .find("</audit-findings>")
        .ok_or_else(|| "协议块未闭合（截断）".to_string())?;
    let body = &reply[start + "<audit-findings>".len()..start + end];
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(body.trim()).map_err(|e| format!("findings JSON 非法：{e}"))?;
    let mut findings = Vec::with_capacity(raw.len());
    if raw.len() > MAX_FINDINGS_PER_BATCH {
        tracing::warn!(
            component = "audit-engine",
            total = raw.len(),
            cap = MAX_FINDINGS_PER_BATCH,
            "单批次发现数超限，已截断（m14）"
        );
    }
    for (i, v) in raw.iter().take(MAX_FINDINGS_PER_BATCH).enumerate() {
        let obj = v.as_object().ok_or_else(|| format!("第 {i} 条非对象"))?;
        let get_str = |k: &str| -> Result<String, String> {
            obj.get(k)
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("第 {i} 条缺字段 {k}"))
        };
        let path = get_str("path")?;
        let line = obj
            .get("line")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| format!("第 {i} 条缺合法 line"))?;
        // 全仓审计 m6：`as u32` 会把超大值静默回绕成合法小行号 → 直接拒为协议违例
        let line = u32::try_from(line).map_err(|_| format!("第 {i} 条 line 越界：{line}"))?;
        let view = serde_json::from_value(
            obj.get("view")
                .cloned()
                .ok_or_else(|| format!("第 {i} 条缺 view"))?,
        )
        .map_err(|e| format!("第 {i} 条 view 非法：{e}"))?;
        let severity = serde_json::from_value(
            obj.get("severity")
                .cloned()
                .ok_or_else(|| format!("第 {i} 条缺 severity"))?,
        )
        .map_err(|e| format!("第 {i} 条 severity 非法：{e}"))?;
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
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK_REPLY: &str = r#"<audit-findings>
[{"path":"src/x.rs","line":42,"view":"correctness","severity":"major","claim":"边界遗漏","evidence":"src/x.rs:42 在 None 时直接解包"}]
</audit-findings>
批次总评：……"#;

    #[test]
    fn parses_well_formed_findings() {
        let f = parse_findings(OK_REPLY).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].path, "src/x.rs");
        assert_eq!(f[0].line, 42);
        assert_eq!(f[0].view, View::Correctness);
        assert_eq!(f[0].severity, Severity::Major);
    }

    #[test]
    fn missing_block_is_protocol_violation() {
        assert!(parse_findings("没有协议块的回复").is_err());
    }

    #[test]
    fn illegal_view_is_protocol_violation() {
        let bad = r#"<audit-findings>
[{"path":"a","line":1,"view":"幻想视角","severity":"major","claim":"x","evidence":"y"}]
</audit-findings>"#;
        assert!(parse_findings(bad).unwrap_err().contains("view 非法"));
    }

    /// 协议层与核验层的职责边界：空 evidence **可解析**（结构合法），但核验层会丢弃；
    /// 丢弃行为的回归在 `verify::tests::empty_or_referenceless_evidence_is_dropped`。
    /// 第三方 r2 m6：旧版本叫这个名字却拿带证据的 OK_REPLY 断言 `!evidence.is_empty()`——
    /// 零证据样本从未构造，删掉被测行为测试仍绿（空测试）。
    #[test]
    fn zero_evidence_parses_at_protocol_layer() {
        let reply = r#"<audit-findings>
[{"path":"a.rs","line":1,"view":"correctness","severity":"minor","claim":"x","evidence":""}]
</audit-findings>"#;
        let f = parse_findings(reply).expect("协议层不因空 evidence 拒收（结构合法）");
        assert_eq!(f.len(), 1, "零证据样本必须真能解析出一条");
        assert!(f[0].evidence.is_empty(), "样本本身必须真的是零证据");
    }

    /// 全仓审计 m6：行号超大值不得 `as u32` 静默回绕成合法小行号。
    #[test]
    fn oversized_line_is_protocol_violation() {
        let bad = r#"<audit-findings>
[{"path":"a.rs","line":4294967296,"view":"correctness","severity":"minor","claim":"c","evidence":"a.rs:1"}]
</audit-findings>"#;
        let err = parse_findings(bad).unwrap_err();
        assert!(err.contains("line 越界"), "{err}");
    }

    /// 全仓审计 m14：单批发现数上限——超出截断，不得无上限刷爆报告。
    #[test]
    fn findings_are_capped_per_batch() {
        let items: Vec<String> = (0..(MAX_FINDINGS_PER_BATCH + 5))
            .map(|i| {
                format!(
                    r#"{{"path":"a.rs","line":1,"view":"correctness","severity":"nit","claim":"c{i}","evidence":"a.rs:1"}}"#
                )
            })
            .collect();
        let reply = format!("<audit-findings>[{}]</audit-findings>", items.join(","));
        let out = parse_findings(&reply).unwrap();
        assert_eq!(out.len(), MAX_FINDINGS_PER_BATCH, "超限应截断到上限");
    }
}
