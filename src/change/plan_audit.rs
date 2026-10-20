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
        if !repo.join(&path).exists() {
            a.issues.push(format!("影响面引用了不存在的路径：{path}:{line}"));
        }
    }

    // 2) 白名单：路径必须存在
    let whitelist = section_paths(plan, "白名单");
    let modify = section_paths(plan, "要改的");
    for f in &whitelist {
        if !repo.join(f).exists() {
            a.issues.push(format!("白名单里的文件不存在：{f}"));
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
    const EXTS: [&str; 12] = [
        ".rs", ".py", ".ts", ".tsx", ".js", ".go", ".java", ".kt", ".md", ".toml", ".json", ".yaml",
    ];
    !tok.is_empty()
        && !tok.starts_with('/')
        && EXTS.iter().any(|e| tok.ends_with(e))
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
    format!(
        "你是 change-plan-review。审的是**计划**（change-id `{change_id}`），不是代码：\
不看 diff、不跑验收命令、不写任何文件。\n\n\
计划文件：`{}`（全文附在下面；你也可用该路径为主张取证）\n\n\
判据（逐条给结论）：\n\
1. 影响面完整性：用 codegraph 核对需求涉及的入口/调用方有没有漏；隐式耦合（配置键/表/错误码/序列化）有没有点到。漏关键调用方 = FAIL。\n\
2. 证据真实：每条影响面带 path:line 且指向真实位置。\n\
3. 白名单自洽：要改的 ⊆ 白名单，白名单每个文件有理由。\n\
4. 验收标准可观测：每条能写成「给定输入 → 期望输出」；写「功能正常」这类不可判定的 = FAIL。\n\
5. 风险诚实：已知耦合、文档与代码不一致、缓存/构建产物有没有写进去。\n\n\
## 引擎审计（确定性，勿质疑、勿重算）\n\n{}\n\n\
## 计划全文（`{}`）\n\n```markdown\n{}\n```\n\n\
末尾输出协议块：<stage-verdict>PASS|FAIL</stage-verdict> + <next>advance|fix</next> + <issues>（FAIL 时逐条，带 path:line）。",
        plan_path.display(),
        audit.render(),
        plan_path.display(),
        plan_text
    )
}

/// 解析计划文本并审计（供编排层调用）。
pub fn audit_plan_file(plan_path: &Path, repo: &Path) -> Result<PlanAudit> {
    let text = std::fs::read_to_string(plan_path).map_err(|e| {
        crate::error::DevforgeError::System(format!("读计划 {} 失败：{e}", plan_path.display()))
    })?;
    Ok(audit(&text, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "// a\n").unwrap();
        std::fs::write(tmp.path().join("src/b.rs"), "// b\n").unwrap();
        tmp
    }

    fn good_plan() -> String {
        "# 变更计划\n\n\
## 1 需求复述\n把 a 改成 b。\n\n\
## 2 影响面\n- 直接命中：`src/a.rs:1`\n- 调用方：`src/b.rs:1`\n\n\
## 3 要改的\n| 文件 | 理由 |\n| --- | --- |\n| `src/a.rs` | 需求点 |\n\n\
## 4 白名单\n`src/a.rs`\n`tests/a_test.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
            .to_string()
    }

    #[test]
    fn plan_facts_ignores_table_rows_and_keeps_bullets() {
        // t26 v3 实证：plan 把验收标准写成 markdown 表格，旧行为连表头都抽成条款（23 条鬼条款）
        let plan = "## 5 验收标准\n\
            | # | 给定 | 期望 |\n\
            |---|---|---|\n\
            | AC1 | count_words(a b) | == 2 |\n\
            普通段落不算条款。\n\
            - 给定 x=1 → 返回 2\n\
            * 给定空串 → 返回 0\n";
        let (criteria, _) = plan_facts(plan);
        assert_eq!(
            criteria,
            vec![
                "给定 x=1 → 返回 2".to_string(),
                "给定空串 → 返回 0".to_string()
            ],
            "只有列表行算条款：{criteria:?}"
        );
    }

    #[test]
    fn clean_plan_passes_with_no_issues() {
        let r = repo();
        // 测试文件在白名单里但还没建——按「新增文件」豁免：先建出来模拟真实流程
        std::fs::create_dir_all(r.path().join("tests")).unwrap();
        std::fs::write(r.path().join("tests/a_test.rs"), "").unwrap();
        let a = audit(&good_plan(), r.path());
        assert!(a.is_clean(), "审计应无问题：{:?}", a.issues);
        assert!(a.path_refs >= 2);
        assert!(a.missing_blocks.is_empty());
    }

    #[test]
    fn nonexistent_path_is_listed() {
        let r = repo();
        let plan = good_plan().replace("`src/b.rs:1`", "`src/ghost.rs:42`");
        let a = audit(&plan, r.path());
        assert!(
            a.issues.iter().any(|i| i.contains("src/ghost.rs:42")),
            "{:?}",
            a.issues
        );
    }

    #[test]
    fn missing_block_is_reported() {
        let r = repo();
        let plan = good_plan().replace("## 6 风险", "## 六 其他");
        let a = audit(&plan, r.path());
        assert!(a.missing_blocks.contains(&"风险".to_string()));
        assert!(a.issues.iter().any(|i| i.contains("六块不全")));
    }

    #[test]
    fn file_outside_whitelist_is_flagged() {
        let r = repo();
        let plan = good_plan().replace(
            "| `src/a.rs` | 需求点 |",
            "| `src/a.rs` | 需求点 |\n| `src/b.rs` | 顺手改 |",
        );
        let a = audit(&plan, r.path());
        assert!(
            a.issues.iter().any(|i| i.contains("src/b.rs") && i.contains("不在白名单")),
            "{:?}",
            a.issues
        );
    }

    #[test]
    fn vague_acceptance_is_flagged() {
        let r = repo();
        let plan = good_plan().replace("- 给定 x=1 → 返回 2", "- 功能正常");
        let a = audit(&plan, r.path());
        assert!(
            a.issues.iter().any(|i| i.contains("不可观测")),
            "{:?}",
            a.issues
        );
    }

    #[test]
    fn empty_whitelist_is_flagged() {
        let r = repo();
        let plan = good_plan().replace("## 4 白名单\n`src/a.rs`\n`tests/a_test.rs`", "## 4 白名单\n");
        let a = audit(&plan, r.path());
        assert!(a.issues.iter().any(|i| i.contains("白名单为空")));
    }

    #[test]
    fn review_instruction_carries_audit_plan_text_and_forbids_rerun() {
        let r = repo();
        let plan = good_plan();
        let a = audit(&plan, r.path());
        let plan_path = r.path().join("docs/changes/x/change-plan.md");
        let text = review_instruction("change-20261020-x", &plan_path, &plan, &a);
        for anchor in [
            "不看 diff",
            "不跑验收命令",
            "勿质疑、勿重算",
            "影响面完整性",
            "验收标准可观测",
            "<stage-verdict>PASS|FAIL</stage-verdict>",
        ] {
            assert!(text.contains(anchor), "审查指令缺锚点：{anchor}");
        }
        // 审查子进程 cwd 是黑板：不给路径与全文，审查官读不到计划
        assert!(
            text.contains(&plan_path.display().to_string()),
            "审查指令必须给计划路径：{text}"
        );
        assert!(text.contains("## 计划全文"), "{text}");
        assert!(
            text.contains("给定 x=1 → 返回 2"),
            "审查指令必须附计划全文：{text}"
        );
    }
}
