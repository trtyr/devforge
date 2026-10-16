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
            sops: vec![PLANNING_SOP],
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
    fn planning_sop_is_builtin() {
        let reg = SopRegistry::builtin();
        assert_eq!(reg.ids(), vec!["planning"]);
        assert!(reg.validate("planning").is_ok());
    }

    #[test]
    fn unknown_sop_rejected() {
        let reg = SopRegistry::builtin();
        let err = reg.validate("coding").unwrap_err();
        assert!(err.to_string().contains("未知 SOP"));
    }
}
