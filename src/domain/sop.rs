//! SOP 层（P004 + decisions/002）：无管理面，全部内置写死。
//!
//! SOP 的本质是编排器代码里显式接线的产物流（见 P001 状态机）；
//! 本模块只维护「内置 SOP 注册表」——任务创建时引用其中一个标识。

use serde::Serialize;

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

/// 内置 SOP：测试流水线（P007 · 第三个 SOP：Test Agent 按文档写集成/e2e 测试 + Test Review 循环）。
pub const TESTING_SOP: Sop = Sop {
    id: "testing",
    name: "测试流水线",
    description: "Test Agent 不看代码只看文档写集成/e2e 测试并自起自跑，Test Review 对文档三判据审查，doc-side 判定移交 doc-fix",
};

/// 内置 SOP：文档修补流水线（P007 D004 · 写文档模式：按 findings 补档 + doc-review 内置审查轮）。
pub const DOC_FIX_SOP: Sop = Sop {
    id: "doc-fix",
    name: "文档修补流水线",
    description: "Doc Fix Agent 按测试阶段 doc-side 发现补齐文档，Doc Review 内置审查（一致性/自洽/Runbook 契约）通过后流回测试环节",
};

/// 内置 SOP：从零写文档（P008 D001/D009 · 第二条产物流：doc-agent + doc-review，四道门滚动）。
pub const DOC_FROM_ZERO_SOP: Sop = Sop {
    id: "doc-from-zero",
    name: "从零写文档",
    description: "对陌生代码库：引擎侧建清单与单元（门0）→ doc-agent 逐单元写 docs/contexts，doc-review 逐单元审（覆盖/完整性/真实性/置信度/可跑）→ 终审",
};

/// 内置 SOP：变更流水线（P009 · 既有项目加功能/修 bug：基线 + 白名单 + 越界二分 + 回归优先）。
pub const CHANGE_SOP: Sop = Sop {
    id: "change",
    name: "变更流水线",
    description: "既有仓 + 一句需求：门0（基线/doc_level/change-id）→ change-plan（codegraph 影响面 + 白名单 + 验收标准）→ 按步提交的 coding（越界必需申报）→ 回归优先的 testing（验收逐条对账）→ 引擎写报告",
};

/// 内置 SOP：全量代码审计（P013 · 从零审整仓，非 PR/diff：盘点→切片→四阶段审查→核验→分级报告）。
pub const CODE_REVIEW_SOP: Sop = Sop {
    id: "code-review",
    name: "全量代码审计",
    description: "对任意项目整仓审计（非 diff）：引擎盘点清单与切片（门0）→ audit-agent 逐批次综合六视角审查（正确性/错误处理/可观测/架构 AI-friendly/一致性/安全）→ 引擎核验过滤与去重分级 → 审计报告（JSON+MD）",
};

/// SOP 描述。**只用于展示与校验，不参与持久化**——自审 minor（59）：旧口径派生了
/// `Deserialize`，可 `&'static str` 字段只能从 `&'static` 源反序列化，公开 API 表面
/// 宣称的能力与实际可用性不符（消费方写 `serde_json::from_str::<Sop>` 必然报错）。
/// 序列化保留（对外快照/日志可读），反序列化删除。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sop {
    /// 内置标识（任务绑定用，如 `"planning"`）。
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// 内置 SOP 注册表。新增 SOP = 在此登记 + 编排器代码接线（改代码，不是加数据）。
///
/// **不自带 `Default`**：自审 minor（68）——`SopRegistry::default()` 得到的是**空注册表**，
/// 与 `builtin()` 语义相反（空表 = 任何 SOP 都「未知」）；作为公开构造路径可静默致错，
/// 要空表请显式写 `SopRegistry::empty()`。
#[derive(Debug, Clone)]
pub struct SopRegistry {
    sops: Vec<Sop>,
}

impl SopRegistry {
    /// 空注册表（仅限测试/显式「什么都不认识」的场景；生产一律 [`SopRegistry::builtin`]）。
    pub fn empty() -> Self {
        Self { sops: Vec::new() }
    }

    /// 系统出厂内置的全部 SOP。
    pub fn builtin() -> Self {
        Self {
            sops: vec![
                PLANNING_SOP,
                CODING_SOP,
                TESTING_SOP,
                DOC_FIX_SOP,
                DOC_FROM_ZERO_SOP,
                CHANGE_SOP,
                CODE_REVIEW_SOP,
            ],
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
#[path = "sop_tests.rs"]
mod tests;
