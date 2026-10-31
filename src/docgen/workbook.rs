//! AF-1 单元作业本（P008 / ai-friendly-findings 2026-10-24 拍板定稿 v2）：
//! 引擎事实落盘，kickoff 只留指针——「上下文 = 寄存器（易失），文件 = 可信内存（可重读）」。
//!
//! 同源纪律：作业本与审查官的确定性审计出自**同一份** inventory 与**同一个**档位
//! 过滤口径（[`DocLevel::requires`]）——作业本即评分标准，无版本漂移（AF-2 被本
//! 形态吸收，不再单独注入）。
//!
//! 降级纪律：作业本是上下文优化，不是正确性依赖——落盘失败时 kickoff 回退清单
//! 指针（宁降级不炸），机械门照旧独立强校验。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::docgen::audit::{DocLevel, kind_str};
use crate::docgen::inventory::Inventory;
use crate::error::{Context, Result};

/// 单元作业本落盘路径：{docs_root}/_meta/units/{unit}.md。
pub fn workbook_path(docs_root: &Path, unit: &str) -> PathBuf {
    docs_root.join("_meta").join("units").join(format!("{unit}.md"))
}

/// 清单版戳：inventory 序列化原文的短指纹（DefaultHasher 64bit → 16 hex）。
/// 版戳只需「变了能察觉」（作业本与重算清单对不上的话，重跑会重新生成本文件），
/// 不需要密码学强度；serde_json 键序确定（本仓未启用 preserve_order）⇒ 同一清单
/// 序列化稳定，指纹稳定。
pub fn inventory_stamp(inv: &Inventory) -> String {
    let text = serde_json::to_string(inv).unwrap_or_default();
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 渲染单元作业本。过滤口径与 `coverage_for_unit` 逐字同源：
/// `entries.filter(unit_id == unit)` × `level.requires(kind)`。
/// 该单元档位内零条目 → `None`（不落盘，kickoff 走降级）。
pub fn render_unit_workbook(
    inv: &Inventory,
    level: DocLevel,
    unit: &str,
) -> Option<String> {
    let entries: Vec<_> = inv
        .entries
        .iter()
        .filter(|e| e.unit_id == unit)
        .collect();
    if entries.is_empty() {
        return None;
    }
    let required: Vec<_> = entries.iter().filter(|e| level.requires(e.kind)).collect();

    let mut out = String::new();
    out.push_str(&format!("# 单元作业本 · {unit}\n\n"));
    out.push_str("> 本文件由引擎在开工前生成（幂等，可随时重读）。**先读完本文件再动笔**——\
                  这就是本单元要写的全部范围与评分标准。\n\n");
    out.push_str(&format!(
        "- 清单版戳：`{}`（inventory 指纹；与本文件不一致 = 清单已重算，重跑会重新生成）\n",
        inventory_stamp(inv)
    ));
    out.push_str(&format!(
        "- 覆盖评分标准：本单元档位 `{}` 内要求覆盖 **{}** 条，缺一条即零覆盖\
          （必须补文档或登记缺口 docs/contexts/_meta/gaps.md）；\
          条目以 canonical_name 被任一文档正文提及即算覆盖（与引擎审计同口径）\n",
        level.as_str(),
        required.len()
    ));
    out.push_str(&format!(
        "\n## 本单元条目（{} 条，其中档位内要求覆盖 {} 条）\n\n",
        entries.len(),
        required.len()
    ));
    out.push_str("| 类别 | 名称 | 位置 | 档位内要求 |\n|---|---|---|---|\n");
    for e in &entries {
        out.push_str(&format!(
            "| {} | `{}` | {}:{} | {} |\n",
            kind_str(e.kind),
            e.canonical_name,
            e.path,
            e.line,
            if level.requires(e.kind) { "要求覆盖" } else { "不要求（可写进相关篇章）" }
        ));
    }
    Some(out)
}

/// 生成并落盘单元作业本（幂等：同清单重写内容一致）。
/// 返回 `Ok(None)` = 该单元档位外零条目（未落盘）。
pub fn write_workbook(
    docs_root: &Path,
    inv: &Inventory,
    level: DocLevel,
    unit: &str,
) -> Result<Option<PathBuf>> {
    let Some(text) = render_unit_workbook(inv, level, unit) else {
        return Ok(None);
    };
    let path = workbook_path(docs_root, unit);
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&parent).ctx(format!("建作业本目录 {}", parent.display()))?;
    std::fs::write(&path, text).ctx(format!("写作业本 {}", path.display()))?;
    Ok(Some(path))
}

/// 审查注入段（AF-1 同源改读）：读已落盘的作业本渲染成注入小节。
/// 读不到 → `None`（审查注入降级为现场计算，调用方不感知）。
pub fn workbook_section(docs_root: &Path, unit: &str) -> Option<String> {
    let path = workbook_path(docs_root, unit);
    let text = std::fs::read_to_string(&path).ok()?;
    Some(format!(
        "## 本单元作业本（评分标准，与写手同源——分母以它为准）\n\n{text}\n"
    ))
}

#[cfg(test)]
#[path = "workbook_tests.rs"]
mod tests;
