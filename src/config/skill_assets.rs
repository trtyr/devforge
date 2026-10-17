//! 内置 skill 资产（编译期嵌入）：skills_source 未提供时的兜底内容源。
//!
//! P2-ISSUE-05 修复：渲染器此前只给占位 README，导致 Agent 为找规范发起
//! 全盘文件系统扫描（单次工具执行 10-30 分钟）。内置资产保证 skill 永远有真正文。

/// 内置 plan-tree skill 正文（assets/ 随仓库走，编译期嵌入）。
pub const PLAN_TREE_SKILL_MD: &str = include_str!("../../assets/skills/plan-tree/SKILL.md");

/// 返回内置 skill 的文件清单（相对 skill 目录的路径 → 文件内容）。
/// 未收录的 skill 返回 None（调用方回落占位行为）。
pub fn builtin_skill_files(skill: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match skill {
        "plan-tree" => Some(vec![("SKILL.md", PLAN_TREE_SKILL_MD)]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_tree_skill_has_frontmatter_and_body() {
        let files = builtin_skill_files("plan-tree").expect("plan-tree 应有内置资产");
        assert!(files.iter().any(|(name, _)| *name == "SKILL.md"));
        let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
        assert!(body.starts_with("---"), "应有 frontmatter");
        assert!(body.contains("# plan-tree"));
        assert!(body.contains("目录约定"), "应有结构约定正文");
    }

    #[test]
    fn unknown_skill_returns_none() {
        assert!(builtin_skill_files("ghost").is_none());
    }
}
