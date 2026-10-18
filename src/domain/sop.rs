//! SOP 层（P004 + decisions/002）：无管理面，全部内置写死。
//!
//! SOP 的本质是编排器代码里显式接线的产物流（见 P001 状态机）；
//! 本模块只维护「内置 SOP 注册表」——任务创建时引用其中一个标识。

use serde::{Deserialize, Serialize};

use crate::error::{DevforgeError, Result};

/// 内置 SOP：规划流水线（P001 设计的第一个 SOP：Plan Agent + Review 循环）。
pub const PLANNING_SOP: Sop = Sop {
    id: "planning",
    name: "规划流水线",
    description: "Plan Agent 与用户沟通产出 plan tree，Review Agent 循环检查语义一致性直至通过",
};

/// 内置 SOP：编码流水线（P005 · 第二个 SOP：Coding Agent 分阶段开发 + Coding Review 循环）。
pub const CODING_SOP: Sop = Sop {
    id: "coding",
    name: "编码流水线",
    description: "Coding Agent 读 plan tree 自定阶段开发（代码+文档伴写+单测），Coding Review 逐轮审查 Report 驱动修复/推进/收工",
};

/// 内置 SOP：文档修补流水线（P007 D004 · 写文档模式：按 findings 补档 + doc-review 内置审查轮）。
pub const DOC_FIX_SOP: Sop = Sop {
    id: "doc-fix",
    name: "文档修补流水线",
    description: "Doc Fix Agent 按测试阶段 doc-side 发现补齐文档，Doc Review 内置审查（一致性/自洽/Runbook 契约）通过后流回测试环节",
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sop {
    /// 内置标识（任务绑定用，如 `"planning"`）。
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// 内置 SOP 注册表。新增 SOP = 在此登记 + 编排器代码接线（改代码，不是加数据）。
#[derive(Debug, Clone, Default)]
pub struct SopRegistry {
    sops: Vec<Sop>,
}

impl SopRegistry {
    /// 系统出厂内置的全部 SOP。
    pub fn builtin() -> Self {
        Self {
            sops: vec![PLANNING_SOP, CODING_SOP, DOC_FIX_SOP],
        }
    }

    pub fn get(&self, id: &str) -> Option<&Sop> {
        self.sops.iter().find(|s| s.id == id)
    }

    /// 校验任务引用的 SOP 是否已内置注册。
    pub fn validate(&self, id: &str) -> Result<()> {
        self.get(id).map(|_| ()).ok_or_else(|| {
            DevforgeError::Config(format!(
                "未知 SOP `{id}`——SOP 无管理面，只有内置注册的可用: {:?}",
                self.ids()
            ))
        })
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.sops.iter().map(|s| s.id).collect()
    }
}

/// Task 里持有的 SOP 引用（owned，序列化友好）。
pub type SopRef = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_sops_are_planning_coding_and_doc_fix() {
        let reg = SopRegistry::builtin();
        assert_eq!(reg.ids(), vec!["planning", "coding", "doc-fix"]);
        assert!(reg.validate("planning").is_ok());
        assert!(reg.validate("coding").is_ok());
        assert!(reg.validate("doc-fix").is_ok());
    }

    #[test]
    fn unknown_sop_rejected() {
        let reg = SopRegistry::builtin();
        let err = reg.validate("ghost").unwrap_err();
        assert!(err.to_string().contains("未知 SOP"));
    }
}
