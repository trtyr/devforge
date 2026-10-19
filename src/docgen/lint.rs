//! doc-lint：文档确定性门禁（D008 引擎侧工具 / D014 必填节分级）。
//!
//! 只做**机械可判**的检查，给具体行号、退出码可判（0 = 通过）。
//! 本版覆盖：必填节（按条目类型）/ 链接可解析 / Runbook 七件套 / 禁写内容扫描。
//! 实体存在性（幽灵引用）与同名异写需要清单（inventory），在后续任务接入。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{Context as _, Result};

/// 检查类别（报告里逐条标注，便于路由与统计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    /// 必填节缺失（D014 分级表）
    RequiredSection,
    /// 链接/锚点不可解析
    Link,
    /// Runbook 七件套不全
    Runbook,
    /// 实体存在性 / 同名异写（t4）
    Entity,
    /// 置信度诚实性（t8）：标 🟢 必须给得出证据
    Confidence,
    /// Mermaid 源码可解析（t9 / D005）
    Mermaid,
    /// 禁写内容（TODO/超长代码块/日志粘贴）——提示级
    Banned,
}

impl Check {
    pub fn as_str(self) -> &'static str {
        match self {
            Check::RequiredSection => "required-section",
            Check::Link => "link",
            Check::Runbook => "runbook",
            Check::Entity => "entity",
            Check::Confidence => "confidence",
            Check::Mermaid => "mermaid",
            Check::Banned => "banned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 直接判 FAIL（退出码非 0）
    Fail,
    /// 只提示，不影响通过
    Warn,
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub file: PathBuf,
    /// 1-based 行号；跨文件的整体检查（如 Runbook 缺项）报 1
    pub line: usize,
    pub check: Check,
    pub severity: Severity,
    pub message: String,
}

impl Violation {
    pub fn render(&self) -> String {
        format!(
            "{}:{} [{}] {}",
            self.file.display(),
            self.line,
            self.check.as_str(),
            self.message
        )
    }
}

#[derive(Debug, Default)]
pub struct LintReport {
    violations: Vec<Violation>,
}

impl LintReport {
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    pub fn failures(&self) -> impl Iterator<Item = &Violation> {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Fail)
    }

    /// 0 违规才判通过（提示级不影响）。
    pub fn passed(&self) -> bool {
        self.failures().next().is_none()
    }

    pub fn summary(&self) -> String {
        let fails = self.failures().count();
        let warns = self.violations.len() - fails;
        if self.violations.is_empty() {
            return "doc-lint: 通过（0 违规）".into();
        }
        format!(
            "doc-lint: {} 个失败 / {} 个提示 → {}",
            fails,
            warns,
            if self.passed() { "通过" } else { "不通过" }
        )
    }

    fn push(&mut self, file: &Path, line: usize, check: Check, severity: Severity, message: String) {
        self.violations.push(Violation {
            file: file.to_path_buf(),
            line,
            check,
            severity,
            message,
        });
    }

    fn fail(&mut self, file: &Path, line: usize, check: Check, message: impl Into<String>) {
        self.push(file, line, check, Severity::Fail, message.into());
    }

    fn warn(&mut self, file: &Path, line: usize, check: Check, message: impl Into<String>) {
        self.push(file, line, check, Severity::Warn, message.into());
    }
}

/// 条目类型（来自文档 frontmatter 的 `type:` 或清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Entry,
    Function,
    Class,
    Config,
    Model,
    ErrorCode,
    /// 内部/私有符号：必填节只记提示，不 FAIL（D014）
    Internal,
}

impl EntryType {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "entry" | "入口" | "入口点" => Some(Self::Entry),
            "function" | "method" | "函数" => Some(Self::Function),
            "class" | "interface" | "类" => Some(Self::Class),
            "config" | "配置" | "配置项" => Some(Self::Config),
            "model" | "data" | "数据模型" => Some(Self::Model),
            "error" | "error-code" | "错误码" => Some(Self::ErrorCode),
            "internal" | "private" | "内部" => Some(Self::Internal),
            _ => None,
        }
    }

    /// D014 必填节分级表（关键词任一命中即视为该节存在）。
    pub fn required_sections(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            Self::Entry => &[
                ("用途", &["用途", "目的"]),
                ("输入", &["输入", "参数", "请求"]),
                ("输出", &["输出", "响应", "返回"]),
                ("错误", &["错误", "异常", "错误码"]),
                ("示例", &["示例", "用法", "example"]),
            ],
            Self::Function => &[
                ("摘要", &["摘要", "概述", "说明"]),
                ("行为语义", &["行为", "语义", "边界", "副作用"]),
                ("参数", &["参数", "args", "parameters"]),
                ("返回", &["返回", "returns"]),
                ("异常", &["异常", "错误", "raises"]),
            ],
            Self::Class => &[
                ("职责", &["职责", "范围"]),
                ("构造或初始化", &["构造", "初始化", "init"]),
                ("公开成员", &["成员", "属性", "方法"]),
                ("生命周期", &["生命周期", "何时"]),
            ],
            Self::Config => &[
                ("名称", &["名称", "名字"]),
                ("类型", &["类型", "type"]),
                ("默认值", &["默认"]),
                ("是否必填", &["必填", "可选"]),
                ("影响", &["影响", "作用"]),
            ],
            Self::Model => &[
                ("字段", &["字段", "列"]),
                ("约束", &["约束", "索引", "唯一"]),
                ("生命周期", &["生命周期", "何时"]),
                ("关系", &["关系", "关联"]),
            ],
            Self::ErrorCode => &[
                ("含义", &["含义", "说明"]),
                ("触发条件", &["触发", "条件"]),
                ("处理建议", &["处理", "客户端"]),
            ],
            Self::Internal => &[
                ("摘要", &["摘要", "概述", "说明"]),
                ("行为语义", &["行为", "语义"]),
            ],
        }
    }
}

/// Runbook 七件套（D001 文档契约）：关键词任一命中即视为该件存在。
const RUNBOOK_SECTIONS: &[(&str, &[&str])] = &[
    ("环境要求", &["环境要求", "运行环境", "环境依赖"]),
    ("依赖安装", &["依赖安装", "安装依赖", "安装"]),
    ("配置项", &["配置项", "配置", "环境变量"]),
    ("启停与健康检查", &["启动", "停止", "健康检查", "health"]),
    ("种子数据", &["种子数据", "示例数据", "初始化数据"]),
    ("测试运行", &["测试运行", "运行测试", "测试"]),
    ("公开接口清单", &["公开接口", "接口清单", "端点", "api"]),
];

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

        check_required_sections(&doc, file, &mut report);
        check_links(&doc, file, &mut report);
        check_banned(&doc, file, &mut report);
        if let Some(index) = index {
            check_entities(&doc, file, index, &mut report);
        }
        if doc.entry_type.is_some() {
            check_confidence(&doc, file, &mut report);
        }
        check_mermaid(&doc, file, &mut report);
        for (idx, line) in doc.lines.iter().enumerate() {
            if line.contains('🔴') && !line.trim_start().starts_with("```") {
                gap_hits.push((file.clone(), idx + 1));
            }
        }
        if is_runbook(file) {
            check_runbook(&doc, file, &mut report);
        }
    }
    // 🔴 缺口必须有 gaps.md 登记（D013：不许静默缺口）
    if !gap_hits.is_empty() {
        let candidates = [
            docs_dir.join("gaps.md"),
            docs_dir.join("_meta/gaps.md"),
        ];
        if !candidates.iter().any(|p| p.exists()) {
            for (file, line) in &gap_hits {
                report.fail(
                    file,
                    *line,
                    Check::Confidence,
                    "标 🔴 GAP 但缺 gaps.md 登记（缺口必须写成问题，不许静默）",
                );
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
        if line.trim_start().starts_with("```") {
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
            Err(_) => continue,
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
struct Heading {
    text: String,
    /// 1-based 行号
    line: usize,
}

struct ParsedDoc {
    /// 文档声明的条目类型（frontmatter `type:`）
    entry_type: Option<EntryType>,
    headings: Vec<Heading>,
    /// 1-based 行号 → 原文
    lines: Vec<String>,
}

impl ParsedDoc {
    fn parse(text: &str) -> Self {
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        let mut entry_type = None;
        let mut headings = Vec::new();

        // frontmatter：仅当首行是 `---`，读到下一个 `---` 为止
        let mut body_start = 0;
        if lines.first().map(|l| l.trim()) == Some("---")
            && let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---")
        {
            let end = end + 1;
            for line in &lines[1..end] {
                if let Some(rest) = line.trim().strip_prefix("type:") {
                    entry_type = EntryType::parse(rest);
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
            headings,
            lines,
        }
    }

    fn has_section(&self, keywords: &[&str]) -> bool {
        self.find_section(keywords).is_some()
    }

    /// 找到匹配关键词的标题索引。
    fn find_section(&self, keywords: &[&str]) -> Option<usize> {
        self.headings.iter().position(|h| {
            let lower = h.text.to_ascii_lowercase();
            keywords.iter().any(|kw| {
                let kw_lower = kw.to_ascii_lowercase();
                h.text.contains(kw) || lower.contains(&kw_lower)
            })
        })
    }

    /// 该标题下的正文是否为空（下一个标题之前全是空行）。
    fn is_section_empty(&self, heading_index: usize) -> bool {
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
    for (idx, line) in doc.lines.iter().enumerate() {
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

/// 收集文档里出现的实体名（供后续实体存在性检查使用；本版只导出，不做判定）。
pub fn mentioned_entities(doc_text: &str) -> BTreeSet<String> {
    let doc = ParsedDoc::parse(doc_text);
    let mut out = BTreeSet::new();
    for line in &doc.lines {
        for token in line.split(|c: char| {
            c.is_whitespace() || c == ',' || c == '。' || c == '，' || c == '(' || c == ')' || c == '`'
        }) {
            let token = token.trim_matches(|c: char| {
                c == '.' || c == ':' || c == ';' || c == '"' || c == '\'' || c == '*' || c == '_'
            });
            if token.len() >= 3 && is_identifier_like(token) {
                out.insert(token.to_string());
            }
        }
    }
    out
}

fn is_identifier_like(token: &str) -> bool {
    let has_alpha = token.chars().any(|c| c.is_ascii_alphabetic() || c == '_');
    let code_shape = token.contains('_')
        || token.chars().any(|c| c.is_ascii_uppercase())
        || token.contains("::")
        || token.contains('/');
    has_alpha && code_shape
}

/// 实体索引：从源码语料建“什么名字/路径在代码里真实存在”（t4）。
///
/// 真相源选**源码语料**而不是清单符号表：符号表只收公开符号且靠启发式，
/// 拿它当唯一真相会把合法的内部标识符误报成幽灵实体。
#[derive(Debug, Default, Clone)]
pub struct EntityIndex {
    names: BTreeSet<String>,
    paths: BTreeSet<String>,
    /// alias → canonical（同名异写提示用，D011）
    aliases: std::collections::BTreeMap<String, String>,
}

impl EntityIndex {
    /// 扫仓库源文件建索引；`inv` 提供别名登记（可缺）。
    pub fn build(
        root: &Path,
        inv: Option<&crate::docgen::inventory::Inventory>,
    ) -> Result<Self> {
        let root = root
            .canonicalize()
            .ctx(format!("解析仓库根 {}", root.display()))?;
        let files = crate::docgen::inventory::scan_source_files(&root)?;
        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for (rel, _) in &files {
            paths.insert(rel.clone());
            if let Some(base) = rel.rsplit('/').next() {
                paths.insert(base.to_string());
                if let Some(stem) = base.rsplit_once('.').map(|(s, _)| s) {
                    names.insert(stem.to_string());
                }
            }
            if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
                for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                    if token.len() >= 3 {
                        names.insert(token.to_string());
                    }
                }
            }
        }
        let mut aliases = std::collections::BTreeMap::new();
        if let Some(inv) = inv {
            for e in &inv.entries {
                for alias in &e.aliases {
                    aliases.insert(alias.clone(), e.canonical_name.clone());
                }
            }
        }
        Ok(Self { names, paths, aliases })
    }

    pub fn has_name(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn has_path(&self, path: &str) -> bool {
        self.paths.contains(path) || self.paths.iter().any(|p| p.ends_with(&format!("/{path}")))
    }

    pub fn alias_of(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(String::as_str)
    }
}

/// 不是实体引用的词（语言关键字/字面量/HTTP 方法）。
const NON_ENTITY_WORDS: &[&str] = &[
    "true", "false", "null", "none", "self", "cls", "this", "return", "def", "class",
    "import", "from", "async", "await", "lambda", "pass", "const", "let", "var", "function",
    "export", "default", "public", "private", "void", "string", "number", "boolean", "int",
    "str", "list", "dict", "any", "optional", "GET", "POST", "PUT", "DELETE", "PATCH",
    "HEAD", "OPTIONS", "localhost",
];

fn check_entities(doc: &ParsedDoc, file: &Path, index: &EntityIndex, report: &mut LintReport) {
    // 实体检查只对**声明了条目类型的文档**生效（契约条目）；
    // 总览/Runbook/plan 设计文档自由引用概念，强制实体检查只会制造噪音。
    let entry_scope = doc.entry_type.is_some();
    let mut in_fence = false;
    for (idx, line) in doc.lines.iter().enumerate() {
        let no = idx + 1;
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for span in inline_code_spans(line) {
            let span = span.trim();
            if span.is_empty() || span.contains(char::is_whitespace) {
                continue;
            }
            // 协议块/占位符/命令行参数不是实体引用
            if span.starts_with(['-', '$', '#', '@', '|']) || span.starts_with("http") {
                continue;
            }
            if span.contains(['<', '>', '|']) {
                continue;
            }
            let cleaned = span.trim_end_matches(')').trim_end_matches('(');
            if is_placeholder(cleaned) {
                continue;
            }
            // 实体检查只对声明了条目类型的契约文档生效（见函数头注释）
            if !entry_scope {
                continue;
            }
            if let Some((path, line_ref)) = split_path_line(cleaned) {
                if find_existing(file, path).is_none() {
                    report.fail(
                        file,
                        no,
                        Check::Entity,
                        format!("幽灵路径：{path} 在代码中不存在"),
                    );
                    continue;
                }
                if let Some(n) = line_ref
                    && !path_line_in_range(file, path, n)
                {
                    report.fail(
                        file,
                        no,
                        Check::Entity,
                        format!("{path}:{n} 行号超出文件范围"),
                    );
                }
                continue;
            }
            if looks_like_path(cleaned) {
                if find_existing(file, cleaned).is_none() {
                    report.fail(
                        file,
                        no,
                        Check::Entity,
                        format!("幽灵路径：{cleaned} 在代码中不存在"),
                    );
                }
                continue;
            }
            if !looks_like_entity_token(cleaned) {
                continue;
            }
            if let Some(canonical) = index.alias_of(cleaned) {
                report.warn(
                    file,
                    no,
                    Check::Entity,
                    format!("同名异写：`{cleaned}` 是 `{canonical}` 的别名，建议用规范名"),
                );
                continue;
            }
            if index.has_name(cleaned) {
                continue;
            }
            report.fail(
                file,
                no,
                Check::Entity,
                format!("幽灵实体：`{cleaned}` 在代码中找不到"),
            );
        }
    }
}

/// 占位符（不是实体）：`N/A`、`N`、`TODO` 类。
fn is_placeholder(token: &str) -> bool {
    matches!(token, "N/A" | "N" | "n/a")
        || token.chars().all(|c| c.is_ascii_uppercase() || c == '/')
}

/// 从文档所在目录逐级向上找目标文件（文档里的相对路径以文档为锚）。
fn find_existing(doc_file: &Path, rel: &str) -> Option<PathBuf> {
    let mut base = doc_file.parent();
    while let Some(dir) = base {
        let candidate = dir.join(rel);
        if candidate.exists() {
            return Some(candidate);
        }
        base = dir.parent();
    }
    None
}

/// 单行内所有 `` `...` `` 内容（不处理跨行）。
fn inline_code_spans(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        match after.find('`') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

fn split_path_line(token: &str) -> Option<(&str, Option<usize>)> {
    let (path, tail) = token.rsplit_once(':')?;
    if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !looks_like_path(path) {
        return None;
    }
    Some((path, tail.parse().ok()))
}

fn path_line_in_range(doc_file: &Path, path: &str, n: usize) -> bool {
    // 相对文档所在目录逐级向上找目标文件
    let mut base = doc_file.parent();
    while let Some(dir) = base {
        let candidate = dir.join(path);
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            return text.lines().count() >= n.max(1);
        }
        base = dir.parent();
    }
    false
}

fn looks_like_path(token: &str) -> bool {
    if token.contains('/') {
        return true;
    }
    const EXTS: &[&str] = &[
        "md", "py", "rs", "ts", "tsx", "js", "jsx", "json", "toml", "yaml", "yml", "sh",
        "sql", "go", "java", "cs", "php", "rb", "c", "h", "cpp", "kt", "swift",
    ];
    match token.rsplit_once('.') {
        Some((_, ext)) => EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

fn looks_like_entity_token(token: &str) -> bool {
    if token.len() < 3 || token.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if !token.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !token
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '.' || c == '$')
    {
        return false;
    }
    !NON_ENTITY_WORDS
        .iter()
        .any(|w| w.eq_ignore_ascii_case(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn clean_doc_passes() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "entry-add.md",
            "---\ntype: entry\n---\n\n# 新增笔记\n\n## 用途\n创建一条笔记。\n\n## 输入\n- title: 必填\n\n## 输出\n返回笔记对象。\n\n## 错误\n400 validation_failed。\n\n## 示例\n`curl -X POST /api/notes`\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed(), "{:?}", report.violations());
    }

    #[test]
    fn missing_required_section_is_failure_with_line() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "entry-bad.md",
            "---\ntype: entry\n---\n\n# 新增笔记\n\n## 用途\n创建一条笔记。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        let v: Vec<_> = report.failures().collect();
        assert!(!v.is_empty());
        assert!(v.iter().any(|x| x.check == Check::RequiredSection && x.line == 1));
        let msg = &v[0].message;
        assert!(msg.contains("输入"), "{msg}");
    }

    #[test]
    fn na_section_counts_as_present_only_when_heading_exists() {
        let tmp = tempfile::tempdir().unwrap();
        // 有节 + 写 N/A → 通过
        write(
            tmp.path(),
            "fn-a.md",
            "---\ntype: function\n---\n\n# foo\n\n## 摘要\n做一件事。\n\n## 行为语义\n幂等。\n\n## 参数\nN/A（无参数）\n\n## 返回\n整数。\n\n## 异常\n不抛异常。\n",
        );
        assert!(lint_docs(tmp.path()).unwrap().passed());
    }

    #[test]
    fn dead_link_reports_line_and_target() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "index.md",
            "# 索引\n\n见 [架构](architecture.md)。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        let v: Vec<_> = report.failures().collect();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].line, 3);
        assert_eq!(v[0].check, Check::Link);
        assert!(v[0].message.contains("architecture.md"));
    }

    #[test]
    fn anchor_must_exist_in_target_file() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "target.md", "# 数据流\n\n内容\n");
        write(tmp.path(), "index.md", "# 索引\n\n见 [数据流](target.md#数据流)。\n");
        assert!(lint_docs(tmp.path()).unwrap().passed());

        write(tmp.path(), "index2.md", "# 索引\n\n见 [不存在](target.md#不存在)。\n");
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report
            .failures()
            .any(|v| v.check == Check::Link && v.message.contains("不存在")));
    }

    #[test]
    fn runbook_requires_seven_items() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "10-runbook.md",
            "# Runbook\n\n## 环境要求\nPython 3.11\n\n## 依赖安装\npip install -r req.txt\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        let v: Vec<_> = report.failures().collect();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].check, Check::Runbook);
        assert!(v[0].message.contains("配置项"), "{}", v[0].message);
    }

    #[test]
    fn banned_markers_are_warnings_not_failures() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "note.md", "# 笔记\n\nTODO: 补充错误码\n");
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed(), "提示级不应判失败");
        assert!(report
            .violations()
            .iter()
            .any(|v| v.check == Check::Banned && v.line == 3));
    }

    #[test]
    fn oversized_code_block_is_warned() {
        let tmp = tempfile::tempdir().unwrap();
        let mut body = String::from("# 文档\n\n```\n");
        for i in 0..50 {
            body.push_str(&format!("line {i}\n"));
        }
        body.push_str("```\n");
        write(tmp.path(), "big.md", &body);
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed());
        assert!(report
            .violations()
            .iter()
            .any(|v| v.message.contains("代码块 50 行")));
    }

    #[test]
    fn pasted_log_block_is_warned() {
        let tmp = tempfile::tempdir().unwrap();
        let mut body = String::from("# 文档\n\n");
        for i in 0..12 {
            body.push_str(&format!("2026-10-19T12:00:{i:02} INFO started\n"));
        }
        write(tmp.path(), "log.md", &body);
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report
            .violations()
            .iter()
            .any(|v| v.check == Check::Banned && v.message.contains("疑似粘贴日志 12 行")));
    }

    #[test]
    fn malformed_frontmatter_does_not_crash() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "weird.md", "---\ntype: ghost\ntitle: x\n");
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed());
    }

    #[test]
    fn internal_symbol_missing_sections_is_only_warning() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "internal.md",
            "---\ntype: internal\n---\n\n# helper\n\n## 摘要\n辅助函数。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed());
        assert!(report
            .violations()
            .iter()
            .any(|v| v.check == Check::RequiredSection && v.severity == Severity::Warn));
    }

    // ===== t4：实体存在性 / 同名异写 =====

    fn fixture_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/core.py"),
            "def create_note(title):\n    return title\n\nclass NoteStore:\n    pass\n",
        )
        .unwrap();
        tmp
    }

    #[test]
    fn real_entities_pass_with_zero_false_positives() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("core.md"),
            "---\ntype: internal\n---\n\n# core\n\n入口 `create_note()` 写入 `NoteStore`，源码在 `src/core.py`，参见 `src/core.py:1`。\n",
        )
        .unwrap();
        let index = EntityIndex::build(repo.path(), None).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        assert!(report.passed(), "不得有误报: {:?}", report.violations());
    }

    #[test]
    fn ghost_symbol_is_reported_with_line() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("ghost.md"),
            "---\ntype: internal\n---\n\n# ghost\n\n下面这个函数不存在：`ghost_function_xyz`。\n",
        )
        .unwrap();
        let index = EntityIndex::build(repo.path(), None).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        let hit = report
            .failures()
            .find(|v| v.message.contains("ghost_function_xyz"))
            .expect("必须报幽灵实体");
        assert_eq!(hit.check, Check::Entity);
        assert_eq!(hit.line, 7, "行号要指向引用处（frontmatter 占 4 行）");
    }

    #[test]
    fn ghost_path_is_reported() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("p.md"), "---\ntype: internal\n---\n\n# p\n\n见 `src/nowhere.py`。\n").unwrap();
        let index = EntityIndex::build(repo.path(), None).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        assert!(report
            .failures()
            .any(|v| v.check == Check::Entity && v.message.contains("幽灵路径")));
    }

    #[test]
    fn out_of_range_line_reference_is_reported() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("l.md"), "---\ntype: internal\n---\n\n# l\n\n见 `src/core.py:999`。\n").unwrap();
        let index = EntityIndex::build(repo.path(), None).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        assert!(report
            .failures()
            .any(|v| v.message.contains("行号超出文件范围")));
    }

    #[test]
    fn alias_usage_is_warning_not_failure() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("a.md"),
            "---\ntype: internal\n---\n\n# a\n\n用 `create_note`（别名 `addNote` 也出现过）。\n",
        )
        .unwrap();
        let mut inv = crate::docgen::inventory::build_inventory(repo.path()).unwrap();
        for e in &mut inv.entries {
            if e.canonical_name == "create_note" {
                e.aliases.push("addNote".into());
            }
        }
        let index = EntityIndex::build(repo.path(), Some(&inv)).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        assert!(report.passed(), "别名只是提示: {:?}", report.violations());
        assert!(report
            .violations()
            .iter()
            .any(|v| v.check == Check::Entity && v.message.contains("同名异写")));
    }

    #[test]
    fn fenced_code_is_not_entity_checked() {
        let repo = fixture_repo();
        let docs = repo.path().join("docs/contexts");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("code.md"),
            "# code\n\n```bash\ncurl -X POST `ghost_thing`\n```\n",
        )
        .unwrap();
        let index = EntityIndex::build(repo.path(), None).unwrap();
        let report = lint_docs_with_entities(&docs, &index).unwrap();
        assert!(report.passed(), "代码块内不查实体: {:?}", report.violations());
    }

    // ===== t8：置信度诚实性 / 缺口登记 =====

    #[test]
    fn green_mark_with_path_line_passes() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.md",
            "---\ntype: internal\n---\n\n# a\n\n🟢 入口在 `src/core.py:3`。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.passed(), "{:?}", report.violations());
    }

    #[test]
    fn green_mark_with_replayable_command_passes() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.md",
            "---\ntype: internal\n---\n\n# a\n\n🟢 可重放：`python -m pytest -q`。\n",
        );
        assert!(lint_docs(tmp.path()).unwrap().passed());
    }

    #[test]
    fn green_mark_without_evidence_fails() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.md",
            "---\ntype: internal\n---\n\n# a\n\n🟢 这里肯定是对的。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        let v: Vec<_> = report.failures().collect();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].check, Check::Confidence);
        assert_eq!(v[0].line, 7);
    }

    #[test]
    fn red_gap_without_gaps_file_fails() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.md",
            "---\ntype: internal\n---\n\n# a\n\n🔴 设计意图看不出来。\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report
            .failures()
            .any(|v| v.check == Check::Confidence && v.message.contains("gaps.md")));
    }

    #[test]
    fn red_gap_with_gaps_file_passes() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "a.md",
            "---\ntype: internal\n---\n\n# a\n\n🔴 设计意图看不出来。\n",
        );
        write(tmp.path(), "_meta/gaps.md", "# 缺口\n\n- 设计意图（a.md）\n");
        assert!(lint_docs(tmp.path()).unwrap().passed());
    }

    // ===== t9：汇编 / Mermaid / 空节 =====

    #[test]
    fn mermaid_block_must_start_with_diagram_kind() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "arch.md",
            "# 架构\n\n```mermaid\nflowchart TD\n  A --> B\n```\n",
        );
        assert!(lint_docs(tmp.path()).unwrap().passed());

        write(
            tmp.path(),
            "bad.md",
            "# 架构\n\n```mermaid\n这不是图源码\n```\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report
            .failures()
            .any(|v| v.check == Check::Mermaid && v.line == 3));
    }

    #[test]
    fn empty_mermaid_block_fails() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "e.md", "# e\n\n```mermaid\n```\n");
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.failures().any(|v| v.check == Check::Mermaid));
    }

    #[test]
    fn required_section_with_empty_body_fails() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "e.md",
            "---\ntype: function\n---\n\n# f\n\n## 摘要\n做一件事。\n\n## 行为语义\n\n## 参数\nN/A（无参数）\n\n## 返回\nint\n\n## 异常\n不抛异常\n",
        );
        let report = lint_docs(tmp.path()).unwrap();
        let v: Vec<_> = report.failures().collect();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("为空"), "{}", v[0].message);
        assert_eq!(v[0].line, 10);
    }

    #[test]
    fn summary_reports_counts() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.md", "# 无类型文档\n\nTODO: x\n");
        let report = lint_docs(tmp.path()).unwrap();
        assert!(report.summary().contains("0 个失败"));
    }
}
