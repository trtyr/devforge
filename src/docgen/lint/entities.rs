//! （ARC-R6 自 lint.rs 平移，纯移动。）

use super::*;
use super::checks::ParsedDoc;

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

pub(super) fn check_entities(doc: &ParsedDoc, file: &Path, index: &EntityIndex, report: &mut LintReport) {
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
///
/// 已知局限（自审 major 登记，不修）：全大写词一律当占位符跳过，全大写命名的
/// 真实实体永不被检查（漏报）——收紧会重新引入占位符误报，两害取其轻。
fn is_placeholder(token: &str) -> bool {
    matches!(token, "N/A" | "N" | "n/a")
        || token.chars().all(|c| c.is_ascii_uppercase() || c == '/')
}

/// 路径逃逸（绝对路径或含 .. 段）：文档路径引用只允许仓内相对路径。
fn path_escapes(rel: &str) -> bool {
    std::path::Path::new(rel).is_absolute() || rel.split(['/', '\\']).any(|seg| seg == "..")
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
pub(super) fn inline_code_spans(line: &str) -> Vec<String> {
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

pub(super) fn split_path_line(token: &str) -> Option<(&str, Option<usize>)> {
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
        // 自审 major：目录/非 UTF-8 目标不可读 → 行号检查不适用（不算越界）——
        // 旧实现 read 失败一律 false，被误报成「行号超出范围」的质量失败。
        if candidate.is_file() {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                return text.lines().count() >= n.max(1);
            }
            return true;
        }
        base = dir.parent();
    }
    false
}

pub(super) fn looks_like_path(token: &str) -> bool {
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

