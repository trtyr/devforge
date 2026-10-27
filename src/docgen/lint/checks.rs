//! （ARC-R6 自 lint.rs 平移，纯移动。）

use super::*;
use super::entities::{check_entities, inline_code_spans, looks_like_path, split_path_line};
use super::report::RUNBOOK_SECTIONS;

/// 对 `docs_dir` 下的全部 markdown 跑四类结构检查（无实体索引，t2 行为）。
pub fn lint_docs(docs_dir: &Path) -> Result<LintReport> {
    lint_with(docs_dir, None)
}

/// 带实体索引的完整检查（t4）：结构四类 + 实体存在性 + 同名异写。
pub fn lint_docs_with_entities(docs_dir: &Path, index: &EntityIndex) -> Result<LintReport> {
    lint_with(docs_dir, Some(index))
}

fn lint_with(docs_dir: &Path, index: Option<&EntityIndex>) -> Result<LintReport> {
    let mut report = LintReport::default();
    let files = collect_markdown(docs_dir)?;
    let mut gap_hits: Vec<(PathBuf, usize)> = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .ctx(format!("读取文档 {}", file.display()))?;
        let doc = ParsedDoc::parse(&text);
        // _meta/ 下是引擎与审查官自己的产物（缺口台账、审查意见、运行账单）：
        // 它们天然会抄一词「待补充/TODO」，拿交付文档的禁令去卡它们只会制造噪音。
        let in_meta = file.components().any(|c| c.as_os_str() == "_meta");

        if !in_meta {
            // 2026-10-27 自审 major：未知 type 值 = 文档缺陷 FAIL（fail-closed）——
            // 旧口径静默 None，整篇条目类型门禁全跳过，lint 报告为空 = 判定通过。
            if let Some(raw) = &doc.unknown_type {
                report.fail(
                    file,
                    1,
                    Check::RequiredSection,
                    format!(
                        "frontmatter type 值未知：`{raw}`（合法值见 doc-contract；\
                         不认识的类型会让本文件全部条目类型门禁失效）"
                    ),
                );
            }
            check_required_sections(&doc, file, &mut report);
            check_links(&doc, file, &mut report);
            check_banned(&doc, file, &mut report);
        }
        if let Some(index) = index {
            check_entities(&doc, file, index, &mut report);
        }
        if doc.entry_type.is_some() {
            check_confidence(&doc, file, &mut report);
        }
        check_mermaid(&doc, file, &mut report);
        // 全仓审计 #8：只在代码围栏**外**找缺口标记——围栏里的 🔴 是示例/数据，
        // 不是真缺口（旧实现只看本行是否以 ``` 开头，围栏内的行全被误计）。
        let mut in_fence = false;
        for (idx, line) in doc.lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            if !in_fence && line.contains('🔴') {
                gap_hits.push((file.clone(), idx + 1));
            }
        }
        if is_runbook(file) {
            check_runbook(&doc, file, &mut report);
        }
    }
    // 🔴 缺口必须有 gaps.md 登记（D013：不许静默缺口）。
    // 自审 major（unit-073）：登记文件存在 ≠ 缺口已登记——旧实现任一 gaps 文件
    // 存在就放行全部 🔴，空文件/陈年旧登记都能静音新缺口（fail-open）。现在要求
    // 登记文件里出现标记所在文件的名字，否则对该文件逐行 FAIL。
    if !gap_hits.is_empty() {
        let candidates = [
            docs_dir.join("gaps.md"),
            docs_dir.join("_meta/gaps.md"),
        ];
        let registry = candidates
            .iter()
            .find(|p| p.exists())
            .map(|p| std::fs::read_to_string(p).unwrap_or_default())
            .unwrap_or_default();
        if registry.is_empty() {
            for (file, line) in &gap_hits {
                report.fail(
                    file,
                    *line,
                    Check::Confidence,
                    "标 🔴 GAP 但缺 gaps.md 登记（缺口必须写成问题，不许静默）",
                );
            }
        } else {
            for (file, line) in &gap_hits {
                let fname = file
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !fname.is_empty() && !registry.contains(&fname) {
                    report.fail(
                        file,
                        *line,
                        Check::Confidence,
                        format!(
                            "标 🔴 GAP 但 gaps.md 未登记本文件（{fname}）——缺口必须逐条入册，不许靠陈年登记静音"
                        ),
                    );
                }
            }
        }
    }
    report
        .violations
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(report)
}

/// 置信度诚实性（t8）：标 🟢 CONFIRMED 的行必须同行给出 `path:line` 或可重放命令。
fn check_confidence(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    let mut in_fence = false;
    for (idx, line) in doc.lines.iter().enumerate() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let marked = line.contains('🟢') || line.to_ascii_uppercase().contains("CONFIRMED");
        if !marked {
            continue;
        }
        let has_evidence = inline_code_spans(line).iter().any(|span| {
            let s = span.trim();
            split_path_line(s).is_some()
                || looks_like_path(s)
                || is_replayable_command(s)
        });
        if !has_evidence {
            report.fail(
                file,
                idx + 1,
                Check::Confidence,
                "标 🟢 CONFIRMED 但给不出 `path:line` 或可重放命令（最严重类 FAIL）",
            );
        }
    }
}

/// Mermaid 源码校验（D005）：只产源码不渲染，但源码必须能解析——
/// 首行必须是已知图类型关键字，空块或未闭合 = FAIL。
fn check_mermaid(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    const KINDS: &[&str] = &[
        "graph",
        "flowchart",
        "sequenceDiagram",
        "classDiagram",
        "erDiagram",
        "stateDiagram",
        "gantt",
        "pie",
        "mindmap",
        "timeline",
        "journey",
        "gitGraph",
        // 自审 major：白名单补齐现代图型——合法的 requirementDiagram/C4/sankey
        // 等不再被误报「无法解析」。
        "requirementDiagram",
        "zenuml",
        "sankey-beta",
        "xychart-beta",
        "quadrantChart",
        "info",
        "C4Context",
        "C4Container",
    ];
    let mut fence_start: Option<usize> = None;
    let mut first_line: Option<String> = None;
    let mut body_lines = 0usize;
    for (idx, line) in doc.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if let Some(start) = fence_start.take() {
                let ok = first_line
                    .as_deref()
                    .map(|l| KINDS.iter().any(|k| l.starts_with(k)))
                    .unwrap_or(false);
                if body_lines == 0 || !ok {
                    report.fail(
                        file,
                        start,
                        Check::Mermaid,
                        "Mermaid 块无法解析：需非空且首行以 graph/flowchart/sequenceDiagram/classDiagram/erDiagram/stateDiagram 等开头",
                    );
                }
                first_line = None;
                body_lines = 0;
            } else if trimmed.trim_start_matches('`').trim() == "mermaid" {
                fence_start = Some(idx + 1);
            }
            continue;
        }
        if fence_start.is_some() {
            if !trimmed.is_empty() && first_line.is_none() {
                first_line = Some(trimmed.to_string());
            }
            if !trimmed.is_empty() {
                body_lines += 1;
            }
        }
    }
    if let Some(start) = fence_start {
        report.fail(file, start, Check::Mermaid, "Mermaid 块未闭合");
    }
}

/// 可重放命令前缀（与契约里“重放命令”一致）。
fn is_replayable_command(token: &str) -> bool {
    const CMDS: &[&str] = &[
        "devforge", "python", "python3", "pytest", "cargo", "npm", "pnpm", "yarn", "curl", "wget",
        "git", "bash", "sh", "make", "go", "node", "docker", "psql", "sqlite3", "redis-cli",
    ];
    let first = token.split_whitespace().next().unwrap_or("");
    CMDS.contains(&first)
}

/// 递归收集 `*.md`（跳过隐藏目录与 `node_modules`/`target`）。
fn collect_markdown(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let rd = match std::fs::read_dir(&cur) {
            Ok(rd) => rd,
            // 自审 major：读失败不再静默——整个子树的文档被排除在 lint 之外
            // 会造成假绿（lint 通过 = 全树检查过，实际少扫了一片）。
            Err(e) => {
                tracing::warn!(
                    component = "doc-engine",
                    path = %cur.display(),
                    error = %e,
                    "lint 跳过不可读目录"
                );
                continue;
            }
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_runbook(file: &Path) -> bool {
    file.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase().contains("runbook"))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub(super) struct Heading {
    pub(super) text: String,
    /// 1-based 行号
    pub(super) line: usize,
}

pub(super) struct ParsedDoc {
    /// 文档声明的条目类型（frontmatter `type:`）
    pub(super) entry_type: Option<EntryType>,
    /// 2026-10-27 自审 major：frontmatter 写了 type 但值不认识（旧口径静默 None →
    /// 整篇文档的全部条目类型门禁失效，lint 报告为空 = 判定通过 = fail-open）。
    /// 记下原值，由主循环报 FAIL。
    pub(super) unknown_type: Option<String>,
    pub(super) headings: Vec<Heading>,
    /// 1-based 行号 → 原文
    pub(super) lines: Vec<String>,
}

impl ParsedDoc {
    pub(super) fn parse(text: &str) -> Self {
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        let mut entry_type = None;
        let mut unknown_type: Option<String> = None;
        let mut headings = Vec::new();

        // frontmatter：仅当首行是 `---`，读到下一个 `---` 为止
        let mut body_start = 0;
        if lines.first().map(|l| l.trim()) == Some("---")
            && let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---")
        {
            let end = end + 1;
            for line in &lines[1..end] {
                if let Some(rest) = line.trim().strip_prefix("type:") {
                    match EntryType::parse(rest) {
                        Some(t) => entry_type = Some(t),
                        None => {
                            let raw = rest.trim().to_string();
                            tracing::warn!(
                                component = "doc-engine",
                                type_value = %raw,
                                "frontmatter type 值未知——按文档缺陷 FAIL（旧口径静默失效门禁）"
                            );
                            unknown_type = Some(raw);
                        }
                    }
                }
            }
            body_start = end + 1;
        }

        let mut in_fence = false;
        for (idx, line) in lines.iter().enumerate().skip(body_start) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('#') {
                let text = rest.trim_start_matches('#').trim().to_string();
                if !text.is_empty() {
                    headings.push(Heading {
                        text,
                        line: idx + 1,
                    });
                }
            }
        }

        Self {
            entry_type,
            unknown_type,
            headings,
            lines,
        }
    }

    pub(super) fn has_section(&self, keywords: &[&str]) -> bool {
        self.find_section(keywords).is_some()
    }

    /// 找到匹配关键词的标题索引。
    pub(super) fn find_section(&self, keywords: &[&str]) -> Option<usize> {
        self.headings.iter().position(|h| {
            let lower = h.text.to_ascii_lowercase();
            keywords.iter().any(|kw| {
                let kw_lower = kw.to_ascii_lowercase();
                h.text.contains(kw) || lower.contains(&kw_lower)
            })
        })
    }

    /// 该标题下的正文是否为空（下一个标题之前全是空行）。
    pub(super) fn is_section_empty(&self, heading_index: usize) -> bool {
        let start = self.headings[heading_index].line; // 1-based → lines[start] 是下一行
        let end = self
            .headings
            .get(heading_index + 1)
            .map(|h| h.line - 1)
            .unwrap_or(self.lines.len());
        let end = end.min(self.lines.len());
        if start >= end {
            return true;
        }
        self.lines[start..end].iter().all(|l| l.trim().is_empty())
    }
}

fn check_required_sections(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    let Some(ty) = doc.entry_type else {
        return; // 未声明类型：本节无法机械判定（实体检查接清单后再管）
    };
    let mut missing = Vec::new();
    let mut empty: Vec<(&str, usize)> = Vec::new();
    for (name, keywords) in ty.required_sections() {
        match doc.find_section(keywords) {
            None => missing.push(*name),
            Some(idx) if doc.is_section_empty(idx) => empty.push((*name, doc.headings[idx].line)),
            Some(_) => {}
        }
    }
    let severity = if ty == EntryType::Internal {
        Severity::Warn
    } else {
        Severity::Fail
    };
    if !missing.is_empty() {
        report.push(
            file,
            1,
            Check::RequiredSection,
            severity,
            format!(
                "条目类型 {:?} 缺必填节：{}（不适用须写 `N/A` 并给理由，省略不算）",
                ty,
                missing.join(" / ")
            ),
        );
    }
    for (name, line) in empty {
        report.push(
            file,
            line,
            Check::RequiredSection,
            severity,
            format!("必填节「{name}」为空：不适用请写 `N/A` 并给理由"),
        );
    }
}

fn check_links(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    let base = file.parent().unwrap_or_else(|| Path::new("."));
    // 自审 major：围栏里的示例链接不检查——README/文档常在代码块里示范链接
    // 语法，旧实现全行扫描把示例当死链误报（``` 与 ~~~ 双支持）。
    let mut in_fence = false;
    for (idx, line) in doc.lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for target in markdown_link_targets(line) {
            let target = target.trim();
            if target.is_empty()
                || target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("mailto:")
            {
                continue;
            }
            let (path_part, anchor) = match target.split_once('#') {
                Some((p, a)) => (p, Some(a)),
                None => (target, None),
            };
            if path_part.is_empty() {
                // 同文件锚点
                if let Some(anchor) = anchor
                    && !doc_has_anchor(doc, anchor)
                {
                    report.fail(
                        file,
                        idx + 1,
                        Check::Link,
                        format!("锚点不存在：#{}", anchor),
                    );
                }
                continue;
            }
            let resolved = base.join(path_part);
            if !resolved.exists() {
                report.fail(
                    file,
                    idx + 1,
                    Check::Link,
                    format!("死链：{}（解析为 {}）", path_part, resolved.display()),
                );
                continue;
            }
            if let Some(anchor) = anchor {
                match std::fs::read_to_string(&resolved) {
                    Ok(target_text) => {
                        let target_doc = ParsedDoc::parse(&target_text);
                        if !doc_has_anchor(&target_doc, anchor) {
                            report.fail(
                                file,
                                idx + 1,
                                Check::Link,
                                format!("锚点在目标文件中不存在：{}#{}", path_part, anchor),
                            );
                        }
                    }
                    Err(_) => report.fail(
                        file,
                        idx + 1,
                        Check::Link,
                        format!("链接目标不可读：{}", path_part),
                    ),
                }
            }
        }
    }
}

fn markdown_link_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            let start = i + 2;
            if let Some(end) = line[start..].find(')') {
                let candidate = &line[start..start + end];
                // 去掉可选标题：`path "title"`
                let candidate = candidate.split_whitespace().next().unwrap_or("");
                out.push(candidate.trim_matches(|c| c == '<' || c == '>').to_string());
                i = start + end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn doc_has_anchor(doc: &ParsedDoc, anchor: &str) -> bool {
    let anchor = anchor.trim().to_ascii_lowercase();
    doc.headings.iter().any(|h| {
        slugify(&h.text) == anchor
            || h.text.to_ascii_lowercase() == anchor
            || h.text.to_ascii_lowercase().replace(' ', "-") == anchor
    })
}

fn slugify(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.extend(ch.to_lowercase());
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    out
}

fn check_banned(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    let mut in_fence = false;
    let mut fence_start = 0usize;
    let mut fence_lines = 0usize;
    let mut log_run = 0usize;
    let mut log_start = 0usize;

    for (idx, line) in doc.lines.iter().enumerate() {
        let no = idx + 1;
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            if in_fence {
                if fence_lines > 40 {
                    report.warn(
                        file,
                        fence_start,
                        Check::Banned,
                        format!("代码块 {fence_lines} 行（>40）——文档宜引用而非整段复制"),
                    );
                }
                in_fence = false;
                fence_lines = 0;
            } else {
                in_fence = true;
                fence_start = no;
            }
            continue;
        }
        if in_fence {
            fence_lines += 1;
            continue;
        }

        let upper = trimmed.to_ascii_uppercase();
        if trimmed.contains("待补充")
            || trimmed.contains("后续补充")
            || upper.contains("TODO")
            || upper.contains("TBD")
        {
            report.warn(
                file,
                no,
                Check::Banned,
                "未完成标记（TODO/TBD/待补充）——未完成就是未完成，不许伪装",
            );
        }

        if looks_like_log_line(trimmed) {
            if log_run == 0 {
                log_start = no;
            }
            log_run += 1;
        } else {
            if log_run >= 10 {
                report.warn(
                    file,
                    log_start,
                    Check::Banned,
                    format!("疑似粘贴日志 {log_run} 行——噪声且无法验证"),
                );
            }
            log_run = 0;
        }
    }
    if in_fence && fence_lines > 40 {
        report.warn(
            file,
            fence_start,
            Check::Banned,
            format!("未闭合代码块 {fence_lines} 行（>40）"),
        );
    }
    if log_run >= 10 {
        report.warn(
            file,
            log_start,
            Check::Banned,
            format!("疑似粘贴日志 {log_run} 行"),
        );
    }
}

fn looks_like_log_line(line: &str) -> bool {
    if line.len() < 12 {
        return false;
    }
    let bytes = line.as_bytes();
    // ISO 时间戳开头：2026-10-19T... 或 2026-10-19 12:00
    let iso = bytes.len() > 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[7] == b'-';
    let level = ["TRACE ", "DEBUG ", "INFO ", "WARN ", "ERROR ", "INFO:", "ERROR:", "WARN:"]
        .iter()
        .any(|l| line.contains(l));
    iso || level
}

fn check_runbook(doc: &ParsedDoc, file: &Path, report: &mut LintReport) {
    let missing: Vec<&str> = RUNBOOK_SECTIONS
        .iter()
        .filter(|(_, keywords)| !doc.has_section(keywords))
        .map(|(name, _)| *name)
        .collect();
    if !missing.is_empty() {
        report.fail(
            file,
            1,
            Check::Runbook,
            format!("Runbook 七件套缺：{}", missing.join(" / ")),
        );
    }
}
