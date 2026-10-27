//! `doc_level` 档位在引擎侧的落点（P009 D002）。
//!
//! 口径：文档完整度是**调用方的决定**，引擎不自动探测「仓里有没有 docs/」。
//! - `none`（默认）：零文档动作，直接进 plan；
//! - `essential`：只产轻量事实层（清单 + 词表）后进 plan；
//! - `complete` / `detailed`：完整文档链（由 doc-from-zero SOP 执行，本模块只声明该动作）。
//!
//! 这里只做确定性部分——真跑文档链需要 agent，不在这层。

use std::path::{Path, PathBuf};

// 自审 minor（61）：直接依赖定义处。旧口径经 `change::gate0` 的重导出取 DocLevel，
// 形成 docs → gate0 → docgen::audit 的链式依赖，「门 0 基线」模块成了档位类型的事实入口。
use crate::docgen::audit::DocLevel;
use crate::error::{Context as _, Result};

/// 引擎该做的文档动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocAction {
    /// `none`：什么都不做。
    Skipped,
    /// `essential`：已产出轻量事实层（`_meta/inventory.json` + `glossary.md`）。
    EssentialDone,
    /// 需要调用方/编排层去跑 doc-from-zero（本层不跑）。
    ///
    /// 自审 minor（63）：**带上档位**——旧口径把 complete 与 detailed 折进同一个无参变体，
    /// 档位信息在归因文案里丢失，detailed 配置的用户被告知跑的是 "complete"。
    FullChainRequired(DocLevel),
}

/// 按档位执行文档动作。
///
/// - `essential` 会真的写 `docs/contexts/_meta/inventory.json` 与 `glossary.md`；
/// - `complete` / `detailed` 不写任何东西，返回 [`DocAction::FullChainRequired`]——文档由
///   agent 写，引擎不能假装自己写完了。
pub fn apply(repo: &Path, level: DocLevel) -> Result<DocAction> {
    match level {
        DocLevel::None => Ok(DocAction::Skipped),
        // 档位定义归一后（全仓审计 #4）多出的 detailed：也是「整条文档链 + 额外约定面」，
        // change SOP 同样不代跑 → 交调用方去跑 doc-from-zero（不因档位更严就静默降级）
        DocLevel::Complete | DocLevel::Detailed => Ok(DocAction::FullChainRequired(level)),
        DocLevel::Essential => {
            // 自审 minor（64）：幂等——事实层已就绪且**两份产物都在**才跳过重写
            // （旧口径只看 inventory.json，glossary 写失败留下的半成品会被当成「已就绪」）。
            if essential_ready(repo) {
                tracing::info!(
                    component = "change-engine",
                    event = "change.docs_essential_reused",
                    repo = %repo.display(),
                    "轻量事实层已存在（inventory + glossary），复用"
                );
                return Ok(DocAction::EssentialDone);
            }
            let inv = crate::docgen::inventory::build_inventory(repo)?;
            let meta = repo.join("docs/contexts/_meta");
            std::fs::create_dir_all(&meta)
                .ctx(format!("建元数据目录 {}", meta.display()))?;
            inv.save(&meta.join("inventory.json"))?;
            std::fs::write(
                meta.join("glossary.md"),
                crate::docgen::inventory::export_glossary(&inv),
            )
            .ctx("写词表")?;
            // 自审 minor（62）：写入用户仓是对外副作用——旧口径零事件零日志，
            // 用户无法从 run log 得知引擎改了他们的仓。
            tracing::info!(
                component = "change-engine",
                event = "change.docs_written",
                repo = %repo.display(),
                inventory = %meta.join("inventory.json").display(),
                glossary = %meta.join("glossary.md").display(),
                entries = inv.entries.len(),
                "已向目标仓写入轻量事实层（docs/contexts/_meta）"
            );
            Ok(DocAction::EssentialDone)
        }
    }
}

/// 轻量事实层是否已经存在（`essential` 幂等判断用）。
///
/// 自审 minor（64）：**两份产物都必须在**——`apply` 先写 inventory 后写 glossary，
/// 旧口径只看前者，glossary 写失败留下的半成品会被判为「已就绪」。
pub fn essential_ready(repo: &Path) -> bool {
    inventory_path(repo).is_file() && glossary_path(repo).is_file()
}

pub fn inventory_path(repo: &Path) -> PathBuf {
    repo.join("docs/contexts/_meta/inventory.json")
}

pub fn glossary_path(repo: &Path) -> PathBuf {
    repo.join("docs/contexts/_meta/glossary.md")
}

#[cfg(test)]
#[path = "docs_tests.rs"]
mod tests;
