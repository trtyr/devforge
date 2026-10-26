//! 确定性审计（P008 · D013/§阶段4）：覆盖对账 + 报告渲染。
//!
//! 分工：结构/实体/置信度/Mermaid 已在 doc-lint（`docgen::lint`）；
//! 本模块补「覆盖」——清单每一条 ↔ 文档是否有落点；并按 `doc_level` 决定要求哪些条目。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::docgen::inventory::{EntryKind, Inventory, InventoryEntry};
use crate::error::{Context as _, Result};

/// 文档档位（D007：默认 complete）。
///
/// 全仓审计 #4：这是**全仓唯一的 DocLevel**——change SOP 曾有一份同名不同值域的
/// 副本（`None/Essential/Complete`，无 `Detailed`），跨模块传递无编译期区分。
/// 现归一到本处：change 侧的「不碰文档」就是 `None` 变体，严格解析走
/// [`DocLevel::parse_strict`]（未知值报配置错），docgen/CLI 的宽松解析走 [`DocLevel::parse`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLevel {
    /// 不碰文档（change SOP 的默认档；覆盖口径 = 不要求任何条目）
    None,
    /// 只要入口点能跑能接
    Essential,
    /// + 模块行为语义、配置项、错误码、数据模型（默认）
    Complete,
    /// + 构建/启动命令等约定面行为（机械覆盖口径 = complete + behavior）
    Detailed,
}

impl DocLevel {
    /// 宽松解析（docgen/CLI）：未知值落 Complete。
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "essential" => Self::Essential,
            "detailed" => Self::Detailed,
            _ => Self::Complete,
        }
    }

    /// 严格解析（change SOP 配置面）：未知值 = 配置错误，不静默降级。
    pub fn parse_strict(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Ok(Self::None),
            "essential" => Ok(Self::Essential),
            "complete" => Ok(Self::Complete),
            "detailed" => Ok(Self::Detailed),
            other => Err(crate::error::DevforgeError::Config(format!(
                "未知 doc_level `{other}`（只认 none/essential/complete/detailed）"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Essential => "essential",
            Self::Complete => "complete",
            Self::Detailed => "detailed",
        }
    }

    /// 该档位是否需要先跑文档阶段。
    pub fn needs_docs(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn requires(self, kind: EntryKind) -> bool {
        match self {
            Self::None => false,
            Self::Essential => kind == EntryKind::Entry,
            Self::Complete => matches!(
                kind,
                EntryKind::Entry | EntryKind::Module | EntryKind::Symbol | EntryKind::Config | EntryKind::Data
            ),
            Self::Detailed => matches!(
                kind,
                EntryKind::Entry
                    | EntryKind::Module
                    | EntryKind::Symbol
                    | EntryKind::Config
                    | EntryKind::Data
                    | EntryKind::Behavior
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoverageMiss {
    pub id: String,
    pub kind: EntryKind,
    pub canonical_name: String,
    pub path: String,
    pub line: usize,
    pub unit_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct CoverageReport {
    pub required: usize,
    pub covered: usize,
    pub missing: Vec<CoverageMiss>,
}

impl CoverageReport {
    pub fn passed(&self) -> bool {
        self.missing.is_empty()
    }

    /// 渲染给审查输入（确定性事实，不是建议）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "覆盖审计（引擎）：要求 {} 条 / 已覆盖 {} 条 / 缺失 {} 条\n",
            self.required,
            self.covered,
            self.missing.len()
        );
        for m in &self.missing {
            out.push_str(&format!(
                "- [{}] {} 未在文档中出现（{}:{}，单元 {}）\n",
                kind_str(m.kind),
                m.canonical_name,
                m.path,
                m.line,
                m.unit_id
            ));
        }
        if self.passed() {
            out.push_str("结论：覆盖 100%（该档位要求集合内）。\n");
        } else {
            out.push_str("结论：覆盖不完整——缺一条即零覆盖，必须补文档或登记缺口（D013）。\n");
        }
        out
    }
}

fn kind_str(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Entry => "entry",
        EntryKind::Module => "module",
        EntryKind::Symbol => "symbol",
        EntryKind::Config => "config",
        EntryKind::Data => "data",
        EntryKind::Behavior => "behavior",
    }
}

/// 覆盖对账：按档位取条目集合，逐条查是否在 docs 目录下被提及。
pub fn coverage(inv: &Inventory, docs_dir: &Path, level: DocLevel) -> Result<CoverageReport> {
    let docs = collect_docs(docs_dir)?;
    let mut report = CoverageReport::default();
    for e in &inv.entries {
        if !level.requires(e.kind) {
            continue;
        }
        report.required += 1;
        if entry_covered(&docs, e) {
            report.covered += 1;
        } else {
            report.missing.push(CoverageMiss {
                id: e.id.clone(),
                kind: e.kind,
                canonical_name: e.canonical_name.clone(),
                path: e.path.clone(),
                line: e.line,
                unit_id: e.unit_id.clone(),
            });
        }
    }
    Ok(report)
}

/// 单单元覆盖（只算该单元的条目），供逐单元审查使用。
///
/// 自审 major 登记（IO 相乘，不修）：每次调用重读全部文档——这是**故意的**：
/// doc SOP 的流水线在审查单元 N 的同时 lead 在写 N+1，文档树是活的；
/// 一次装载的语料快照会让后审单元拿到陈旧覆盖（假缺失）。文档量级是 KB，
/// 新鲜度优先于 IO。
pub fn coverage_for_unit(
    inv: &Inventory,
    docs_dir: &Path,
    level: DocLevel,
    unit: &str,
) -> Result<CoverageReport> {
    let docs = collect_docs(docs_dir)?;
    let mut report = CoverageReport::default();
    for e in inv.entries.iter().filter(|e| e.unit_id == unit) {
        if !level.requires(e.kind) {
            continue;
        }
        report.required += 1;
        if entry_covered(&docs, e) {
            report.covered += 1;
        } else {
            report.missing.push(CoverageMiss {
                id: e.id.clone(),
                kind: e.kind,
                canonical_name: e.canonical_name.clone(),
                path: e.path.clone(),
                line: e.line,
                unit_id: e.unit_id.clone(),
            });
        }
    }
    Ok(report)
}

/// 解析后的文档：全文 + frontmatter 的 type + 首个标题。
struct DocFile {
    text: String,
    ty: Option<String>,
    heading: Option<String>,
}

fn collect_docs(dir: &Path) -> Result<Vec<DocFile>> {
    let mut out = Vec::new();
    for f in collect_markdown(dir)? {
        let text = std::fs::read_to_string(&f).ctx(format!("读文档 {}", f.display()))?;
        let ty = frontmatter_type(&text);
        let heading = text
            .lines()
            .find(|l| l.trim_start().starts_with('#'))
            .map(|l| l.trim().trim_start_matches('#').trim().to_string());
        out.push(DocFile { text, ty, heading });
    }
    Ok(out)
}

fn frontmatter_type(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for l in lines {
        let t = l.trim();
        if t == "---" {
            break;
        }
        if let Some(v) = t.strip_prefix("type:") {
            return Some(v.trim().trim_matches(['"', '\'']).to_lowercase());
        }
    }
    None
}

/// 条目是否真被文档覆盖。
///
/// 模块必须有一篇**自称模块文档**（frontmatter `type: module`）并提到它，
/// 或者有文档**链接**到它；只在 Runbook 里被顺手提一句不算。
/// 反例（2026-10-19 实测）：某单元整个被跳过，但它的 `main.py` 在 Runbook 代码块里
/// 出现过一行，旧口径就把它算成已覆盖——分母没洞，覆盖却假了。
fn entry_covered(docs: &[DocFile], e: &InventoryEntry) -> bool {
    match e.kind {
        EntryKind::Module => {
            docs.iter().any(|d| {
                // 自审 major（unit-0xx）：词边界匹配——`log` 不得命中 `login`/`catalog`；
                // 标题同名判定同口径（「Authorization Guide」不算 auth 的模块文档标题）。
                let names_it = d.text.contains(&e.path) || mentions_name(&d.text, &e.canonical_name);
                let self_declared = matches!(d.ty.as_deref(), Some("module") | Some("模块"));
                let titled = d
                    .heading
                    .as_deref()
                    .map(|h| h.contains(&e.path) || mentions_name(h.trim(), &e.canonical_name))
                    .unwrap_or(false);
                names_it && (self_declared || titled)
            }) || docs
                .iter()
                .any(|d| d.text.contains(&format!("]({}", e.path)))
        }
        _ => docs.iter().any(|d| mentions_name(&d.text, &e.canonical_name)),
    }
}

/// 自审 major：词边界匹配——`log` 不得命中 `login`/`catalog`。
/// 边界字符 = 非 ASCII 字母数字且非 `_`；中日韩等非 ASCII 字符视为边界
///（代码标识符是 ASCII 的，中文文档直接嵌入标识符是常态）。
fn mentions_name(haystack: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut rest = haystack;
    while let Some(pos) = rest.find(name) {
        let before_ok = pos == 0
            || rest[..pos]
                .chars()
                .next_back()
                .map(|c| !is_word_char(c))
                .unwrap_or(true);
        let after_start = pos + name.len();
        let after_ok = after_start >= rest.len()
            || rest[after_start..]
                .chars()
                .next()
                .map(|c| !is_word_char(c))
                .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        rest = &rest[pos + name.len()..];
    }
    false
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn collect_markdown(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let rd = match std::fs::read_dir(&cur) {
            Ok(rd) => rd,
            // 自审 major：读失败不再静默——整子树文档被排除出覆盖分母 = 假绿。
            Err(err) => {
                tracing::warn!(
                    component = "doc-engine",
                    path = %cur.display(),
                    error = %err,
                    "覆盖对账跳过不可读目录"
                );
                continue;
            }
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                // 引擎审计修复 #1：_meta/ 是 SOP 自产元数据（门 0 的清单与词表）——
                // 词表含全部 Symbol 的 canonical_name，纳入覆盖分母会让 Symbol 在
                // 单元正文写出前就 100% 假绿（与 duplicate_mentions 的排除口径对齐）。
                if name == "_meta" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 抽取 Runbook 里的可执行命令块（bash/sh/console），供审查官“从零 boot”时逐条跑。
pub fn runbook_commands(docs_dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for f in collect_markdown(docs_dir)? {
        let is_runbook = f
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase().contains("runbook"))
            .unwrap_or(false);
        if !is_runbook {
            continue;
        }
        let text = std::fs::read_to_string(&f).ctx(format!("读 {}", f.display()))?;
        let mut cur: Vec<String> = Vec::new();
        let mut in_block = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("```") {
                if in_block {
                    if !cur.is_empty() {
                        out.push(cur.join("\n"));
                    }
                    cur.clear();
                    in_block = false;
                } else {
                    let info = t.trim_start_matches('`').trim();
                    in_block = matches!(info, "bash" | "sh" | "console" | "shell" | "zsh");
                }
                continue;
            }
            if in_block && !t.is_empty() && !t.starts_with('#') {
                cur.push(line.to_string());
            }
        }
    }
    Ok(out)
}

/// 跨模块重复提及（终审用）：同一符号名出现在 ≥2 个模块文档里，说明该实体被重复描述。
pub fn duplicate_mentions(docs_dir: &Path, inv: &Inventory) -> Result<Vec<String>> {
    let files = collect_markdown(docs_dir)?;
    let mut out = Vec::new();
    for e in &inv.entries {
        if e.kind != EntryKind::Symbol || e.canonical_name.len() < 4 {
            continue;
        }
        let mut files_hit = Vec::new();
        for f in &files {
            // 元数据目录不参与（词表/置信度表天然会列全部名字）
            if f.components().any(|c| c.as_os_str() == "_meta") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(f)
                && text.contains(&e.canonical_name)
            {
                files_hit.push(f.display().to_string());
            }
        }
        if files_hit.len() >= 2 {
            out.push(format!(
                "`{}` 在 {} 个文档中重复出现：{}",
                e.canonical_name,
                files_hit.len(),
                files_hit.join(", ")
            ));
        }
    }
    out.sort();
    Ok(out)
}

/// 跨模块重复正文（终审判据）：归一化后 ≥40 字且逐字相同的行出现在 ≥2 篇非 _meta 文档。
/// 为什么不用「名字出现几次」当判据：实体名在多篇文档里互相引用是**正常写法**，
/// 拿它当重复会让小仓全绿文档全员背锅；重复该看「同一段解释被拄了两遍」。
pub fn duplicated_content(docs_dir: &Path) -> Result<Vec<String>> {
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in collect_markdown(docs_dir)? {
        if f.components().any(|c| c.as_os_str() == "_meta") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let mut local: BTreeSet<String> = BTreeSet::new();
        let mut in_fence = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            // 代码示例在几篇文档里重复是正常的（同一个调用例），不是「拄了两遍解释」
            if in_fence {
                continue;
            }
            if looks_like_index(line) {
                continue;
            }
            let n = normalize_line(line);
            if n.chars().count() >= 40 {
                local.insert(n);
            }
        }
        for n in local {
            seen.entry(n).or_default().push(f.display().to_string());
        }
    }
    Ok(seen
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .map(|(line, files)| {
            let preview: String = line.chars().take(60).collect();
            format!(
                "同一段落在 {} 篇文档中重复：{}…（{}）",
                files.len(),
                preview,
                files.join(", ")
            )
        })
        .collect())
}

/// 行归一化：去列表符号/引用符号、合并空白（避免缩进差异掩盖重复）。
fn normalize_line(raw: &str) -> String {
    let t = raw.trim().trim_start_matches(['-', '*', '>']).trim();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 导航索引行（大量 markdown 链接）不算重复：它本来就该在多个文档里指同一批叶子。
fn looks_like_index(raw: &str) -> bool {
    let mut link_chars = 0;
    let mut depth = false;
    for c in raw.chars() {
        if c == '[' {
            depth = true;
        }
        if depth {
            link_chars += 1;
        }
        if c == ')' {
            depth = false;
        }
    }
    let total = raw.chars().count().max(1);
    link_chars * 2 >= total
}

/// 未被任何文档提及的单元（供引擎在终审前定位返工范围）。
pub fn uncovered_units(report: &CoverageReport) -> BTreeSet<String> {
    report
        .missing
        .iter()
        .map(|m| m.unit_id.clone())
        .filter(|u| !u.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
