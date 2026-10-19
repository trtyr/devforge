//! 内置 skill 资产（编译期嵌入）：skills_source 未提供时的兜底内容源。
//!
//! P2-ISSUE-05 修复：渲染器此前只给占位 README，导致 Agent 为找规范发起
//! 全盘文件系统扫描（单次工具执行 10-30 分钟）。内置资产保证 skill 永远有真正文。

/// 内置 plan-tree skill 正文（assets/ 随仓库走，编译期嵌入）。
pub const PLAN_TREE_SKILL_MD: &str = include_str!("../../assets/skills/plan-tree/SKILL.md");

/// 内置 doc-contract skill 正文（P008 T000a：跨 SOP 共用的文档契约）。
pub const DOC_CONTRACT_SKILL_MD: &str = include_str!("../../assets/skills/doc-contract/SKILL.md");

/// 返回内置 skill 的文件清单（相对 skill 目录的路径 → 文件内容）。
/// 未收录的 skill 返回 None（调用方回落占位行为）。
pub fn builtin_skill_files(skill: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match skill {
        "plan-tree" => Some(vec![("SKILL.md", PLAN_TREE_SKILL_MD)]),
        "doc-contract" => Some(vec![("SKILL.md", DOC_CONTRACT_SKILL_MD)]),
        other => builtin_language_files(other),
    }
}

// ===== P005 T003：四语言最佳实践 skills（外部 vendor 原样嵌入；来源与许可证登记见
// docs/plantree/plans/005-coding-stage/evidence/skills-sources.md）=====
const RUSTBESTPRACTICES_0: (&str, &str) = ("SKILL.md", include_str!("../../assets/skills/rust-best-practices/SKILL.md"));

const GOBESTPRACTICES_0: (&str, &str) = ("SKILL.md", include_str!("../../assets/skills/go-best-practices/SKILL.md"));

const TSJSBESTPRACTICES_0: (&str, &str) = ("SKILL.md", include_str!("../../assets/skills/ts-js-best-practices/SKILL.md"));
const TSJSBESTPRACTICES_1: (&str, &str) = ("javascript-strict.md", include_str!("../../assets/skills/ts-js-best-practices/javascript-strict.md"));

const PYTHONBESTPRACTICES_0: (&str, &str) = ("SKILL.md", include_str!("../../assets/skills/python-best-practices/SKILL.md"));
const PYTHONBESTPRACTICES_1: (&str, &str) = ("assets/templates/.env.example", include_str!("../../assets/skills/python-best-practices/assets/templates/.env.example"));
const PYTHONBESTPRACTICES_2: (&str, &str) = ("assets/templates/.gitignore", include_str!("../../assets/skills/python-best-practices/assets/templates/.gitignore"));
const PYTHONBESTPRACTICES_3: (&str, &str) = ("assets/templates/.project-root", include_str!("../../assets/skills/python-best-practices/assets/templates/.project-root"));
const PYTHONBESTPRACTICES_4: (&str, &str) = ("assets/templates/CLAUDE.md", include_str!("../../assets/skills/python-best-practices/assets/templates/CLAUDE.md"));
const PYTHONBESTPRACTICES_5: (&str, &str) = ("assets/templates/Makefile", include_str!("../../assets/skills/python-best-practices/assets/templates/Makefile"));
const PYTHONBESTPRACTICES_6: (&str, &str) = ("assets/templates/README.md", include_str!("../../assets/skills/python-best-practices/assets/templates/README.md"));
const PYTHONBESTPRACTICES_7: (&str, &str) = ("assets/templates/ci.sh", include_str!("../../assets/skills/python-best-practices/assets/templates/ci.sh"));
const PYTHONBESTPRACTICES_8: (&str, &str) = ("assets/templates/configs/settings.yaml", include_str!("../../assets/skills/python-best-practices/assets/templates/configs/settings.yaml"));
const PYTHONBESTPRACTICES_9: (&str, &str) = ("assets/templates/docs/adr/0001-record-architecture-decisions.md", include_str!("../../assets/skills/python-best-practices/assets/templates/docs/adr/0001-record-architecture-decisions.md"));
const PYTHONBESTPRACTICES_10: (&str, &str) = ("assets/templates/pyproject.toml", include_str!("../../assets/skills/python-best-practices/assets/templates/pyproject.toml"));
const PYTHONBESTPRACTICES_11: (&str, &str) = ("assets/templates/scripts/check_drift.py", include_str!("../../assets/skills/python-best-practices/assets/templates/scripts/check_drift.py"));
const PYTHONBESTPRACTICES_12: (&str, &str) = ("assets/templates/src_pkg/__init__.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/__init__.py"));
const PYTHONBESTPRACTICES_13: (&str, &str) = ("assets/templates/src_pkg/adapters/__init__.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/adapters/__init__.py"));
const PYTHONBESTPRACTICES_14: (&str, &str) = ("assets/templates/src_pkg/adapters/mock.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/adapters/mock.py"));
const PYTHONBESTPRACTICES_15: (&str, &str) = ("assets/templates/src_pkg/adapters/openai_llm.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/adapters/openai_llm.py"));
const PYTHONBESTPRACTICES_16: (&str, &str) = ("assets/templates/src_pkg/cli.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/cli.py"));
const PYTHONBESTPRACTICES_17: (&str, &str) = ("assets/templates/src_pkg/core/__init__.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/core/__init__.py"));
const PYTHONBESTPRACTICES_18: (&str, &str) = ("assets/templates/src_pkg/core/logging.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/core/logging.py"));
const PYTHONBESTPRACTICES_19: (&str, &str) = ("assets/templates/src_pkg/core/prompts.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/core/prompts.py"));
const PYTHONBESTPRACTICES_20: (&str, &str) = ("assets/templates/src_pkg/core/settings.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/core/settings.py"));
const PYTHONBESTPRACTICES_21: (&str, &str) = ("assets/templates/src_pkg/ports/__init__.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/ports/__init__.py"));
const PYTHONBESTPRACTICES_22: (&str, &str) = ("assets/templates/src_pkg/ports/exceptions.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/ports/exceptions.py"));
const PYTHONBESTPRACTICES_23: (&str, &str) = ("assets/templates/src_pkg/ports/factory.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/ports/factory.py"));
const PYTHONBESTPRACTICES_24: (&str, &str) = ("assets/templates/src_pkg/ports/protocols.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/ports/protocols.py"));
const PYTHONBESTPRACTICES_25: (&str, &str) = ("assets/templates/src_pkg/prompts/rag/answer.md", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/prompts/rag/answer.md"));
const PYTHONBESTPRACTICES_26: (&str, &str) = ("assets/templates/src_pkg/schemas/__init__.py", include_str!("../../assets/skills/python-best-practices/assets/templates/src_pkg/schemas/__init__.py"));
const PYTHONBESTPRACTICES_27: (&str, &str) = ("assets/templates/tests/conftest.py", include_str!("../../assets/skills/python-best-practices/assets/templates/tests/conftest.py"));
const PYTHONBESTPRACTICES_28: (&str, &str) = ("assets/templates/tests/test_conformance.py", include_str!("../../assets/skills/python-best-practices/assets/templates/tests/test_conformance.py"));
const PYTHONBESTPRACTICES_29: (&str, &str) = ("assets/templates/tests/test_settings.py", include_str!("../../assets/skills/python-best-practices/assets/templates/tests/test_settings.py"));
const PYTHONBESTPRACTICES_30: (&str, &str) = ("assets/templates/tests/test_smoke.py", include_str!("../../assets/skills/python-best-practices/assets/templates/tests/test_smoke.py"));
const PYTHONBESTPRACTICES_31: (&str, &str) = ("ci.sh", include_str!("../../assets/skills/python-best-practices/ci.sh"));
const PYTHONBESTPRACTICES_32: (&str, &str) = ("references/standard.md", include_str!("../../assets/skills/python-best-practices/references/standard.md"));
const PYTHONBESTPRACTICES_33: (&str, &str) = ("scripts/check_appendix_sync.py", include_str!("../../assets/skills/python-best-practices/scripts/check_appendix_sync.py"));
const PYTHONBESTPRACTICES_34: (&str, &str) = ("scripts/check_conformance.py", include_str!("../../assets/skills/python-best-practices/scripts/check_conformance.py"));
const PYTHONBESTPRACTICES_35: (&str, &str) = ("scripts/scaffold.py", include_str!("../../assets/skills/python-best-practices/scripts/scaffold.py"));

/// 四语言内置资产：skill 名 → 文件清单（相对 skill 目录路径 → 内容）。
pub fn builtin_language_files(skill: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match skill {
        "rust-best-practices" => Some(vec![RUSTBESTPRACTICES_0]),
        "go-best-practices" => Some(vec![GOBESTPRACTICES_0]),
        "ts-js-best-practices" => Some(vec![TSJSBESTPRACTICES_0,
            TSJSBESTPRACTICES_1]),
        "python-best-practices" => Some(vec![PYTHONBESTPRACTICES_0,
            PYTHONBESTPRACTICES_1,
            PYTHONBESTPRACTICES_2,
            PYTHONBESTPRACTICES_3,
            PYTHONBESTPRACTICES_4,
            PYTHONBESTPRACTICES_5,
            PYTHONBESTPRACTICES_6,
            PYTHONBESTPRACTICES_7,
            PYTHONBESTPRACTICES_8,
            PYTHONBESTPRACTICES_9,
            PYTHONBESTPRACTICES_10,
            PYTHONBESTPRACTICES_11,
            PYTHONBESTPRACTICES_12,
            PYTHONBESTPRACTICES_13,
            PYTHONBESTPRACTICES_14,
            PYTHONBESTPRACTICES_15,
            PYTHONBESTPRACTICES_16,
            PYTHONBESTPRACTICES_17,
            PYTHONBESTPRACTICES_18,
            PYTHONBESTPRACTICES_19,
            PYTHONBESTPRACTICES_20,
            PYTHONBESTPRACTICES_21,
            PYTHONBESTPRACTICES_22,
            PYTHONBESTPRACTICES_23,
            PYTHONBESTPRACTICES_24,
            PYTHONBESTPRACTICES_25,
            PYTHONBESTPRACTICES_26,
            PYTHONBESTPRACTICES_27,
            PYTHONBESTPRACTICES_28,
            PYTHONBESTPRACTICES_29,
            PYTHONBESTPRACTICES_30,
            PYTHONBESTPRACTICES_31,
            PYTHONBESTPRACTICES_32,
            PYTHONBESTPRACTICES_33,
            PYTHONBESTPRACTICES_34,
            PYTHONBESTPRACTICES_35]),
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
    fn doc_contract_skill_carries_frontmatter_templates_and_rules() {
        // P008 T000a：契约必须自带四样东西，少一样就是四处漂移的起点
        let files = builtin_skill_files("doc-contract").expect("doc-contract 应有内置资产");
        let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
        assert!(body.starts_with("---"), "需带 frontmatter 供 skill 加载器识别");
        assert!(body.contains("name: doc-contract"));
        assert!(body.contains("必填"), "需含必填节规则");
        assert!(body.contains("N/A"), "需含 N/A 声明规则（省略不等于不适用）");
        assert!(body.contains("反模式"), "需含反模式表");
        assert!(body.contains("path:line"), "需含真实性回指要求");
        assert!(body.contains("Runbook 七件套"), "需含 Runbook 门禁条目");
    }

    #[test]
    fn unknown_skill_returns_none() {
        assert!(builtin_skill_files("ghost").is_none());
    }

    #[test]
    fn language_skills_vendor_coverage() {
        // P005 T003：四语言 vendor 完整性（来源登记见 P005 evidence/skills-sources.md）
        let rust = builtin_language_files("rust-best-practices").unwrap();
        assert_eq!(rust.len(), 1);
        assert!(rust[0].1.contains("Rust Strict Standard"));
        let go = builtin_language_files("go-best-practices").unwrap();
        assert!(go.iter().any(|(n, b)| *n == "SKILL.md" && b.contains("Go Strict Standard")));
        let ts = builtin_language_files("ts-js-best-practices").unwrap();
        assert_eq!(ts.len(), 2, "SKILL.md(TS) + javascript-strict.md");
        let py = builtin_language_files("python-best-practices").unwrap();
        assert!(py.iter().any(|(n, _)| *n == "SKILL.md"));
        assert!(py.iter().any(|(n, _)| *n == "references/standard.md"));
        assert!(py.iter().any(|(n, _)| n.starts_with("scripts/")), "scaffold/验收脚本随 skill 分发");
        // 原样入库：frontmatter 保留（pi 按 description 触发）
        assert!(rust[0].1.starts_with("---"));
    }
}
