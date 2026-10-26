//! 变更计划的机械审计（P009 T007b）。
//!
//! 分工（D012）：**引擎算确定性的东西**，审查官只做语义判断。
//! 这里只回答「路径存不存在 / 白名单齐不齐 / 验收条目能不能判」——不算语义，也不替审查下结论。
//!
//! 审计结果会整段注入审查指令，审查官被要求**不得重算**。

use std::path::Path;

use crate::error::Result;

/// 六块必备（与 T006 提示词同口径）。
pub const PLAN_BLOCKS: [&str; 6] = [
    "需求复述",
    "影响面",
    "要改的",
    "白名单",
    "验收标准",
    "风险",
];

/// 不可观测的验收措辞（出现且没有 `→` 即判不可判）。
const VAGUE_ACCEPTANCE: [&str; 4] = ["功能正常", "正常工作", "体验良好", "没问题"];

/// 仓库手术的文件迁移登记（ARC 系列，单点事实源）：仓库手术会迁移文件（如 ARC-R3 把
/// `sop/doc_fix.rs` 迁 `sop/doc/fix.rs`），历史步提交是**事实**不可改，而白名单登记**现状**——
/// 两侧对账（a16 步提交扫描等）在比对前先经 [`normalize_path`] 归一化。
pub const PATH_MIGRATIONS: &[(&str, &str)] = &[
    ("src/sop/doc_fix.rs", "src/sop/doc/fix.rs"),
    ("src/sop/doc_from_zero.rs", "src/sop/doc/runner.rs"),
    ("src/engine/retry.rs", "src/engine/policy.rs"),
    ("tests/f6_observability.rs", "tests/f6_observability/main.rs"),
    ("tests/change_review_protocol.rs", "tests/change_review_protocol/main.rs"),
    ("tests/change_failure_context.rs", "tests/change_failure_context/main.rs"),
    ("src/server/tests.rs", "src/server/tests/mod.rs"),
];

/// 把历史路径归一化到现状路径（不在迁移表中的路径原样返回）。
pub fn normalize_path(path: &str) -> &str {
    for (old, new) in PATH_MIGRATIONS {
        if path == *old {
            return new;
        }
    }
    path
}

// ---------------- 问询环的结构契约（提示词与机械门共用同一批字面量） ----------------
//
// 分工同 D012：门只判**结构与留痕**（确定性），「该不该问」这种语义判断交审查官。
// 同一批字面量出现两处（提示词 / 门），唯一的防漂移手段是提示词锚点单测双向钉住
// （`src/config/manifest.rs` 断言提示词含这里的常量）——改一边不改另一边 = 门空转。

/// 标记：这条歧义**必须由用户裁决**（不许带默认值自决放行）。
pub const VERDICT_MUST_ASK: &str = "[必须裁决]";

/// 标记：这条歧义可由 agent 自决（分类恰当性由审查官判）。
pub const VERDICT_SELF: &str = "[自决]";

/// 载体：用户答复原文（逐字抄）写在同一条目里。
pub const VERDICT_ANSWER_FIELD: &str = "用户裁决：";

/// 裁决门结论：只判结构与留痕（确定性），不判语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerdictAudit {
    /// 硬失败：标了「必须裁决」却没留痕 → 计划轮直接打回。
    pub issues: Vec<String>,
    /// 确定性事实：注入审查指令，审查官在其之上做语义判断（分类恰当性、结构缺失）。
    pub facts: Vec<String>,
}

impl VerdictAudit {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// 渲染成注入审查指令的文本。
    pub fn render_facts(&self) -> String {
        if self.facts.is_empty() {
            return "（无）\n".to_string();
        }
        let mut out = String::new();
        for f in &self.facts {
            out.push_str(&format!("- {f}\n"));
        }
        out
    }
}

/// 裁决门：核对计划里的「必须用户裁决」条目有没有留痕。
///
/// **不进 `audit()`**：既有四条机械审计的行为一个字不动，这里是独立的纯函数
/// （`audit()` / `audit_plan_file()` 的签名与语义都不变）。
///
/// 判据（只判结构，不判语义）：
/// - 硬失败：歧义块里标了 `[必须裁决]` 的条目，缺**非空**的 `用户裁决：` 字段；
/// - 只进事实（交审查官判）：歧义块缺失、未分类的顶层条目、顶层散文里的必须裁决声明。
pub fn audit_verdicts(plan: &str) -> VerdictAudit {
    let mut a = VerdictAudit::default();
    let Some(block) = ambiguity_block(plan) else {
        a.facts.push(
            "歧义块缺失：计划里没有含「歧义」的标题——无法核对「必须裁决」条目的留痕".to_string(),
        );
        return a;
    };
    let (entries, prose) = top_level_entries(&block);
    let (mut must, mut self_decided, mut unclassified) = (0usize, 0usize, 0usize);
    for e in &entries {
        if e.text.contains(VERDICT_MUST_ASK) {
            must += 1;
            if answer_of(&e.text).is_none() {
                a.issues.push(format!(
                    "歧义条目标了「{VERDICT_MUST_ASK}」却没有「{VERDICT_ANSWER_FIELD}」留痕：{} —— \
必须先用 ask-user-question 问用户，把答复逐字写进「{VERDICT_ANSWER_FIELD}」字段，不许带默认值自决放行",
                    e.first
                ));
            }
        } else if e.text.contains(VERDICT_SELF) {
            self_decided += 1;
        } else {
            unclassified += 1;
            a.facts.push(format!(
                "歧义块里有未分类的顶层条目（既没标「{VERDICT_MUST_ASK}」也没标「{VERDICT_SELF}」）：{}",
                e.first
            ));
        }
    }
    for p in &prose {
        // 「必须…裁决」的散文声明（必须用户裁决 / 必须裁决 / 必须由用户裁决……）
        if p.contains("必须") && p.contains("裁决") {
            a.facts.push(format!(
                "歧义块里有顶层散文声明必须裁决、但没有对应的分类条目：{p}"
            ));
        }
    }
    a.facts.push(format!(
        "歧义块统计：必须裁决 {must} 条 / 自决 {self_decided} 条 / 未分类 {unclassified} 条"
    ));
    a
}

/// 歧义块：**最后一个**含「歧义」的标题之后、到下一个标题为止的内容行。
///
/// 后出现的同名标题覆盖先前的（既有 `section_lines` 语义）——计划的「需求复述与歧义」
/// 与「歧义与本计划采用的默认」两处同名时，只判后者。
fn ambiguity_block(plan: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = plan.lines().collect();
    // 最后一个含「歧义」的标题（后出现的同名标题覆盖先前的——既有 `section_lines` 语义）
    let start = lines
        .iter()
        .rposition(|l| l.trim_start().starts_with('#') && l.contains("歧义"))?;
    let mut block = Vec::new();
    for line in &lines[start + 1..] {
        if line.trim_start().starts_with('#') {
            break;
        }
        block.push(*line);
    }
    Some(block)
}

/// 顶层条目：顶层列表行 + 其缩进续行（条目是「一条歧义」的单位）。
struct Entry {
    /// 条目首行（报错时引用它）。
    first: String,
    /// 条目全文（首行 + 续行）——`用户裁决：` 字段可能在续行上。
    text: String,
}

/// 顶层列表行 = 行首无缩进且以 `- ` / `* ` 开头（**带空格**）；分隔线与散文行都不算。
fn is_top_level_list(trimmed: &str) -> bool {
    trimmed.starts_with("- ") || trimmed.starts_with("* ")
}

/// 分隔线（`---` / `***` / `___`）不是散文。
fn is_separator(trimmed: &str) -> bool {
    trimmed.len() >= 3
        && trimmed
            .chars()
            .all(|c| c == '-' || c == '*' || c == '_')
}

/// 拆块：顶层条目 + 顶层散文行。
fn top_level_entries(block: &[&str]) -> (Vec<Entry>, Vec<String>) {
    let mut entries: Vec<Entry> = Vec::new();
    let mut prose: Vec<String> = Vec::new();
    for raw in block {
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !indented {
            if is_top_level_list(trimmed) {
                entries.push(Entry {
                    first: trimmed.to_string(),
                    text: trimmed.to_string(),
                });
            } else if !is_separator(trimmed) {
                prose.push(trimmed.to_string());
            }
        } else if let Some(last) = entries.last_mut() {
            last.text.push('\n');
            last.text.push_str(trimmed);
        }
    }
    (entries, prose)
}

/// 条目里的用户答复原文：`用户裁决：` 之后的非空文本。
///
/// 容忍 ASCII 冒号（`用户裁决:`）——硬门因一个标点误判要白烧掉一轮修复上限。
fn answer_of(text: &str) -> Option<&str> {
    for field in [VERDICT_ANSWER_FIELD, "用户裁决:"] {
        if let Some((_, rest)) = text.split_once(field) {
            let answer = rest.trim();
            if !answer.is_empty() {
                return Some(answer);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanAudit {
    /// 确定性发现（审查官不得重算，只在此基础上做语义判断）。
    pub issues: Vec<String>,
    /// 命中的 path:line 条数（证据密度参考）。
    pub path_refs: usize,
    /// 缺失的六块。
    pub missing_blocks: Vec<String>,
}

impl PlanAudit {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// 渲染成注入审查指令的文本。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "命中 path:line {} 条；缺失块：{}；问题 {} 条\n",
            self.path_refs,
            if self.missing_blocks.is_empty() {
                "无".to_string()
            } else {
                self.missing_blocks.join("、")
            },
            self.issues.len()
        ));
        for i in &self.issues {
            out.push_str(&format!("- {i}\n"));
        }
        out
    }
}

/// 审计计划文本。`repo` 用于判定路径是否存在。
pub fn audit(plan: &str, repo: &Path) -> PlanAudit {
    audit_with_contract(plan, repo, None)
}

/// 审计计划文本（台账感知）：计划里的 `docs/plantree/…` 引用在台账已移出仓后
/// 本来就不在 `repo` 下——`plantree` 给了就按去前缀后的相对路径再找一次。
/// 用于重审历史计划（那些计划的引用目标如今在台账根）。
pub fn audit_with_plantree(plan: &str, repo: &Path, plantree: &Path) -> PlanAudit {
    audit_inner_roots(plan, repo, Some(plantree))
}

/// 审计计划文本（带契约，P012 挂账①）：契约存在时校验「白名单 ⊆ 契约 owned_paths」——
/// 提案越界在计划阶段就打回（早失败），不等引擎终校验（晚失败）。
pub fn audit_with_contract(
    plan: &str,
    repo: &Path,
    contract: Option<&crate::domain::contract::WorkItemContract>,
) -> PlanAudit {
    let mut a = audit_inner(plan, repo);
    if let Some(c) = contract {
        let whitelist = section_paths(plan, "白名单");
        for f in &whitelist {
            if !c.covers(f) {
                a.issues.push(format!(
                    "白名单里的 {f} 超出契约 owned_paths（提案越界：申报与契约不一致，须改计划或改契约）"
                ));
            }
        }
    }
    a
}

fn audit_inner(plan: &str, repo: &Path) -> PlanAudit {
    audit_inner_roots(plan, repo, None)
}

/// 存在性判定：仓内优先；给了台账根且路径带 `docs/plantree/` 前缀时，
/// 去掉前缀去台账根下再找一次（2026-10-25 台账移出仓后的历史计划重审）。
fn resolves(repo: &Path, plantree: Option<&Path>, path: &str) -> bool {
    if repo.join(path).exists() {
        return true;
    }
    match (plantree, path.strip_prefix("docs/plantree/")) {
        (Some(root), Some(rel)) => root.join(rel).exists(),
        _ => false,
    }
}

fn audit_inner_roots(plan: &str, repo: &Path, plantree: Option<&Path>) -> PlanAudit {
    let mut a = PlanAudit::default();

    for block in PLAN_BLOCKS {
        if !plan.contains(block) {
            a.missing_blocks.push(block.to_string());
        }
    }
    if !a.missing_blocks.is_empty() {
        a.issues.push(format!(
            "六块不全，缺：{}",
            a.missing_blocks.join("、")
        ));
    }

    // 1) path:line 存在性
    for (path, line) in path_refs(plan) {
        a.path_refs += 1;
        if !resolves(repo, plantree, &path) {
            a.issues.push(format!("影响面引用了不存在的路径：{path}:{line}"));
        }
    }

    // 2) 白名单：路径必须存在——引擎审计修复 #2：存在性只对「要改的」强制
    // （修改目标不存在 = 计划错误）；白名单中「要改的」之外的部分是新增文件
    // （coding 阶段创建），plan 时必然不存在，强制 exists 会把合法新文件
    // 逼成「先建空文件再规划」或「越界申报」的绕行。
    let whitelist = section_paths(plan, "白名单");
    let modify = section_paths(plan, "要改的");
    for f in &modify {
        if !resolves(repo, plantree, f) {
            a.issues.push(format!("要改的文件不存在：{f}"));
        }
    }

    // 3) 要改的 ⊆ 白名单
    for f in &modify {
        if !whitelist.iter().any(|w| w == f) {
            a.issues.push(format!("「要改的」里的 {f} 不在白名单里"));
        }
    }
    if whitelist.is_empty() {
        a.issues.push("白名单为空（改动边界没写）".into());
    }

    // 4) 验收标准可观测性
    for line in section_lines(plan, "验收标准") {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if VAGUE_ACCEPTANCE.iter().any(|v| t.contains(v)) && !t.contains('→') {
            a.issues.push(format!("验收标准不可观测（缺「输入 → 期望输出」）：{t}"));
        }
    }

    a
}

/// 抽出 `path:line` 形式的引用（不依赖正则：按空白/标点切 token 再校验形状）。
pub fn path_refs(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '`' | '(' | ')' | '，' | '。' | '、' | '|')) {
        let tok = raw.trim_matches(|c: char| matches!(c, ',' | ';' | '：' | ':'));
        if let Some((path, line)) = split_path_line(tok) {
            out.push((path, line));
        }
    }
    out
}

fn split_path_line(tok: &str) -> Option<(String, u32)> {
    let (path, line) = tok.rsplit_once(':')?;
    if !line.chars().all(|c| c.is_ascii_digit()) || line.is_empty() {
        return None;
    }
    if !path.contains('.') || path.starts_with('/') {
        return None;
    }
    let n: u32 = line.parse().ok()?;
    Some((path.to_string(), n))
}

/// 某块（到下一个标题为止）的内容行。
fn section_lines<'a>(plan: &'a str, title: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in plan.lines() {
        let is_heading = line.trim_start().starts_with('#');
        if is_heading {
            inside = line.contains(title);
            continue;
        }
        if inside {
            out.push(line);
        }
    }
    out
}

/// 某块里出现的文件路径（按行解析反引号或裸路径 token）。
fn section_paths(plan: &str, title: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in section_lines(plan, title) {
        for raw in line.split(|c: char| c.is_whitespace() || matches!(c, '`' | '|' | '，' | '、' | '）')) {
            let tok = raw.trim_matches(|c: char| matches!(c, ',' | ';' | '：' | '。' | '(' | ')'));
            if looks_like_file(tok) && !out.iter().any(|p| p == tok) {
                out.push(tok.to_string());
            }
        }
    }
    out
}

fn looks_like_file(tok: &str) -> bool {
    // 自审 major（unit-068）：扩展名单与 inventory::SOURCE_EXTENSIONS 对齐
    //（单一事实源），另补配置类扩展与无扩展名的知名构建文件——旧名单只有 12 个
    // 扩展名，白名单被静默削窄：计划里合法登记的非标准扩展文件（如 .yml/.conf）
    // 被边界门恒判越界且无法自救。
    const EXTRA_EXTS: [&str; 12] = [
        ".toml", ".json", ".yaml", ".yml", ".xml", ".sql", ".ini", ".conf", ".env",
        ".sh", ".proto", ".graphql",
    ];
    const KNOWN_FILES: [&str; 4] = ["Makefile", "Dockerfile", ".gitignore", ".env.example"];
    let named = KNOWN_FILES.iter().any(|k| tok.ends_with(k));
    // 注意：SOURCE_EXTENSIONS 存的是裸扩展名（无点，如 "rs"），必须补点再比对——
    // 否则 "3c"/"panic"（尾巴碰上裸 "c"）、"chars"（碰上裸 "rs"）全被当文件。
    let ext_ok = crate::docgen::inventory::SOURCE_EXTENSIONS
        .iter()
        .any(|(e, _)| tok.ends_with(&format!(".{e}")))
        || EXTRA_EXTS.iter().any(|e| tok.ends_with(e));
    !tok.is_empty()
        && !tok.starts_with('/')
        && (named || ext_ok)
        && !tok.chars().any(|c| c.is_whitespace())
}

/// 从计划文本抽「验收标准」与「白名单」（编排层进入 coding/testing 前要用）。
///
/// - 验收标准：`验收标准` 块里的**列表项**（行首 `-`/`*` 才算——表格行、普通段落、
///   小标题一律不收：对账表是一对一机械匹配，多抽一条 = test agent 永远对不齐）；
/// - 白名单：`白名单` 块里的文件路径（去重）。
pub fn plan_facts(plan: &str) -> (Vec<String>, Vec<String>) {
    let criteria = section_lines(plan, "验收标准")
        .into_iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with('-') || t.starts_with('*')
        })
        .map(|l| {
            l.trim()
                .trim_start_matches(['-', '*'])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let whitelist = section_paths(plan, "白名单");
    (criteria, whitelist)
}

/// 审查指令：语义判据 + **计划路径与全文** + 注入式引擎审计（审查官不得重算）。
///
/// 计划文件在目标仓，而审查子进程的 cwd 是黑板——不给路径和全文，审查官根本读不到计划。
pub fn review_instruction(
    change_id: &str,
    plan_path: &Path,
    plan_text: &str,
    audit: &PlanAudit,
) -> String {
    let verdicts = audit_verdicts(plan_text);
    format!(
        "你是 change-plan-review。审的是**计划**（change-id `{change_id}`），不是代码：\
不看 diff、不跑验收命令、不写任何文件。\n\n\
计划文件：`{}`（全文附在下面；你也可用该路径为主张取证）\n\n\
判据（逐条给结论）：\n\
1. 影响面完整性：用 codegraph 核对需求涉及的入口/调用方有没有漏；隐式耦合（配置键/表/错误码/序列化）有没有点到。漏关键调用方 = FAIL。\n\
2. 证据真实：每条影响面带 path:line 且指向真实位置。\n\
3. 白名单自洽：要改的 ⊆ 白名单，白名单每个文件有理由。\n\
4. 验收标准可观测：每条能写成「给定输入 → 期望输出」；写「功能正常」这类不可判定的 = FAIL。\n\
5. 风险诚实：已知耦合、文档与代码不一致、缓存/构建产物有没有写进去。\n\
6. 留痕核对：歧义块里标了「{VERDICT_MUST_ASK}」的条目必须带非空的「{VERDICT_ANSWER_FIELD}」字段（答复原文逐字抄）；缺字段 = FAIL。\n\
7. 分类恰当性：需求与代码都定不下来、且影响验收口径的歧义被标成「{VERDICT_SELF}」（带默认值自决放行）= FAIL。\n\
8. 结构缺失：歧义块缺失，或块内条目既没标「{VERDICT_MUST_ASK}」也没标「{VERDICT_SELF}」= FAIL。\n\n\
## 引擎审计（确定性，勿质疑、勿重算）\n\n{}\n\n\
## 裁决门事实（确定性，勿质疑、勿重算）\n\n{}\n\n\
## 计划全文（`{}`）\n\n```markdown\n{}\n```\n\n\
末尾输出协议块：<stage-verdict>PASS|FAIL</stage-verdict> + <next>advance|fix</next> + <issues>（FAIL 时逐条，带 path:line）。",
        plan_path.display(),
        audit.render(),
        verdicts.render_facts(),
        plan_path.display(),
        plan_text
    )
}

/// 解析计划文本并审计（供编排层调用）。
pub fn audit_plan_file(plan_path: &Path, repo: &Path) -> Result<PlanAudit> {
    audit_plan_file_with_contract(plan_path, repo, None)
}

/// 契约感知版（P012 挂账①）：编排层把 task 契约传进来，提案越界在 plan 审查即打回。
pub fn audit_plan_file_with_contract(
    plan_path: &Path,
    repo: &Path,
    contract: Option<&crate::domain::contract::WorkItemContract>,
) -> Result<PlanAudit> {
    let text = std::fs::read_to_string(plan_path).map_err(|e| {
        crate::error::DevforgeError::System(format!("读计划 {} 失败：{e}", plan_path.display()))
    })?;
    Ok(audit_with_contract(&text, repo, contract))
}

#[cfg(test)]
#[path = "plan_audit_tests.rs"]
mod tests;
