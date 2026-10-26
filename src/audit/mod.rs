//! 全量代码审计 SOP（P013）：对任意项目整仓审计——非 PR/diff。
//!
//! 设计：`docs/plantree/plans/013-code-review-audit/`（五阶段：盘点→切片→
//! 四阶段审查→核验→汇总分级）。M0 范围：盘点（复用 docgen inventory）+
//! 语义连贯切片器 + 骨架接线（run 走到切片完成即收）。
//!
//! 阶段（roadmap 定稿）：
//! ```text
//! Stage 0 规划（元数据→ReviewPlan）→ Stage 1 批次审查（综合六视角）→
//! Stage 2 跨单元分析 → Stage 3 汇总分级
//! ```

use std::path::Path;

use crate::error::{Context as _, Result};

pub mod finding;
pub mod pipeline;
pub mod report;
pub mod runner;
pub mod verify;

/// 一个审查单元：语义连贯的文件组（≤`MAX_UNIT_FILES`），一次批次审查的消费单位。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AuditUnit {
    /// 单元号（`unit-001` 起顺排）。
    pub id: String,
    /// 单元内源码文件（仓库相对路径）。
    pub files: Vec<String>,
    /// 人类可读标签（聚合依据：inventory 单元名或目录名）。
    pub label: String,
    /// 超大文件分片的重点符号（`name（第 a–b 行）`）；空 = 整文件审查。
    /// 第三方 r3 m5：单文件超行数预算后不可再切，只能把「看哪段」写进批次指令，
    /// 否则 1000+ 行的编排内核会整包丢给一个会话。
    pub focus: Vec<String>,
    /// 分片行区间（1-based 闭区间；`None` = 整文件）——预查只数本段，
    /// 否则同一文件的多个分片看到相同计数（r3 m5 遗留）。
    pub focus_range: Option<(usize, usize)>,
}

/// 切片产物：全部审查单元 + 覆盖对账计数（全仓审计 M6）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AuditPlan {
    pub units: Vec<AuditUnit>,
    /// 盘点到并进入切片的源文件数（去重后）。
    pub source_files: usize,
    /// 未进切片的条目数（Config/Data/Behavior 等非源码面）。
    pub excluded_entries: usize,
}

/// 每单元文件数上限（CodeCrow：批次按文件数限不按 token——≤7 可预测）。
pub const MAX_UNIT_FILES: usize = 7;

/// 每单元源码行数上限（第三方 r2 M3）：文件数上限挡不住同目录多个大文件——
/// 本仓 src/audit 六个源文件（含 1073 行 runner.rs）会整包成一批 ≈2300 行，
/// 超出单会话能可靠审查的量。超限按顺序切批；单文件自身超限独占一批（文件不可再分）。
pub const MAX_UNIT_LINES: usize = 800;

fn line_count(workspace: &Path, rel: &str) -> usize {
    std::fs::read_to_string(workspace.join(rel))
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

/// 分片：文件列表 / 重点符号 / 行区间（1-based 闭区间；None = 整文件）。
pub type Piece = (Vec<String>, Vec<String>, Option<(usize, usize)>);

/// 超大单文件按**符号边界**分片（第三方 r3 m5）：每片的符号跨度 ≤ MAX_UNIT_LINES；
/// 无符号信息时退回整文件一片（至少不丢文件）。
/// 行区间供预查只看本段（不然同一文件的多个分片看到相同计数）。
fn split_oversized_file(
    path: &str,
    symbols: &[(usize, String)],
    total_lines: usize,
) -> Vec<Piece> {
    let mut syms = symbols.to_vec();
    syms.sort_by_key(|(l, _)| *l);
    if syms.is_empty() {
        return vec![(vec![path.to_string()], Vec::new(), None)];
    }
    let mut groups: Vec<Vec<(usize, String)>> = Vec::new();
    for (line, name) in syms {
        let start = groups
            .last()
            .and_then(|g| g.first())
            .map(|(l, _)| *l)
            .unwrap_or(line);
        let need_new = groups
            .last()
            .map(|g| !g.is_empty() && line.saturating_sub(start) > MAX_UNIT_LINES)
            .unwrap_or(false);
        if need_new {
            groups.push(Vec::new());
        }
        match groups.last_mut() {
            Some(g) => g.push((line, name)),
            None => groups.push(vec![(line, name)]),
        }
    }
    let n = groups.len();
    let starts: Vec<usize> = groups
        .iter()
        .map(|g| g.first().map(|(l, _)| *l).unwrap_or(1))
        .collect();
    let ends: Vec<usize> = (0..n)
        .map(|i| {
            if i + 1 < n {
                starts[i + 1].saturating_sub(1)
            } else {
                total_lines.max(starts[i])
            }
        })
        .collect();
    // 自审 major：单片行数预算必须在分片出口兑现——旧实现只按符号分组，单个大符号
    //（如几千行的函数）或尾部符号到文件尾的跨度可以让一片远超 MAX_UNIT_LINES，
    // 「每片 ≤ 800 行」的预算在单文件场景完全失效。超宽片按 MAX_UNIT_LINES 硬切
    // 子区间（同一符号跨多片时 focus 重复列出，行区间预查各自只看本段）。
    let mut pieces: Vec<Piece> = Vec::new();
    for (i, g) in groups.into_iter().enumerate() {
        let (s, e) = (starts[i], ends[i]);
        // focus 只列**落在本片行区间内**的符号：超预算硬切出的尾片可能一个符号
        // 都不含——旧实现把整组符号复制给每片，批次指令宣称「本片重点符号」
        // 却指向不在本片的行，审查面被误导。
        let focus: Vec<String> = g
            .iter()
            .filter(|(l, _)| *l >= s && *l <= e)
            .map(|(l, name)| format!("{name}（第 {l} 行）"))
            .collect();
        if e.saturating_sub(s) + 1 <= MAX_UNIT_LINES {
            pieces.push((vec![path.to_string()], focus, Some((s, e))));
        } else {
            let mut cs = s;
            while cs <= e {
                let ce = (cs + MAX_UNIT_LINES - 1).min(e);
                let in_range: Vec<String> = g
                    .iter()
                    .filter(|(l, _)| *l >= cs && *l <= ce)
                    .map(|(l, name)| format!("{name}（第 {l} 行）"))
                    .collect();
                pieces.push((vec![path.to_string()], in_range, Some((cs, ce))));
                cs = ce + 1;
            }
        }
    }
    // 只产出一片 = 整文件一片（不存在「其它片由其它批次负责」），focus 清空。
    if pieces.len() == 1 {
        if let Some((_, focus, _)) = pieces.first_mut() {
            focus.clear();
        }
    }
    pieces
}

/// 按双预算顺序切批（不重排：同批文件语义仍连贯）。
fn split_by_budget(workspace: &Path, files: &[String]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_lines = 0usize;
    for f in files {
        let n = line_count(workspace, f);
        let full = cur.len() >= MAX_UNIT_FILES
            || (!cur.is_empty() && cur_lines + n > MAX_UNIT_LINES);
        if full {
            out.push(std::mem::take(&mut cur));
            cur_lines = 0;
        }
        cur.push(f.clone());
        cur_lines += n;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 盘点 + 切片（M0 核心，确定性零 LLM）：inventory 盘仓 → 按既有 unit_id /
/// 目录聚类 → 超限单元再二分。
pub fn build_plan(workspace: &Path) -> Result<AuditPlan> {
    let inventory = crate::docgen::inventory::build_inventory(workspace)?;

    // 聚类键：inventory 预分配的 unit_id 优先（门 0 已按模块分好），无则按父目录。
    let mut groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // 符号行号（超大文件分片用）：path → [(line, name)]
    let mut symbols: std::collections::BTreeMap<String, Vec<(usize, String)>> =
        std::collections::BTreeMap::new();
    // 全仓审计 M6：非源码面条目（Config/Data/Behavior）不再静默丢弃——计数进报告
    let mut excluded_entries = 0usize;
    for e in &inventory.entries {
        if e.kind == crate::docgen::inventory::EntryKind::Symbol {
            symbols
                .entry(e.path.clone())
                .or_default()
                .push((e.line, e.canonical_name.clone()));
        }
        if e.kind != crate::docgen::inventory::EntryKind::Module
            && e.kind != crate::docgen::inventory::EntryKind::Symbol
        {
            excluded_entries += 1;
            continue; // 只切源码面（Config/Data/Behavior 条目不是切片对象）
        }
        if !seen.insert(e.path.clone()) {
            continue;
        }
        let key = if e.unit_id.is_empty() {
            let dir = Path::new(&e.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());
            format!("dir:{dir}")
        } else {
            // unit 内再按目录分：docgen 的小仓 unit 启发式会把整个 src 给同一 unit，
            // 直接用会跨目录混批（审计批次要语义连贯 = 同目录/同模块）
            let dir = Path::new(&e.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());
            format!("unit:{}:{dir}", e.unit_id)
        };
        groups.entry(key).or_default().push(e.path.clone());
    }

    // 超限切分：文件数（MAX_UNIT_FILES）+ 行数（MAX_UNIT_LINES）双预算——
    // 只按文件数挡不住「同目录多个大文件」（第三方 r2 M3）。
    let mut units: Vec<AuditUnit> = Vec::new();
    let mut seq = 0usize;
    for (key, mut files) in groups {
        files.sort();
        let label = key
            .split_once(':')
            .map(|(_, v)| v.to_string())
            .unwrap_or(key.clone());
        // 审计修复 #10：分片标签用组内局部序号（全局 seq 当分子会出现「分片 4/2」）
        let chunks = split_by_budget(workspace, &files);
        let n_chunks = chunks.len();
        for (chunk_i, chunk) in chunks.into_iter().enumerate() {
            seq += 1;
            // r3 m5：单文件超行数预算时拆成多个「同文件不同符号区间」单元，
            // 否则 1000+ 行的编排内核整包丢给一个会话（并行只提吞吐，降不了装载量）。
            let multi: Vec<Piece> = if chunk.len() == 1
                && line_count(workspace, &chunk[0]) > MAX_UNIT_LINES
            {
                let syms = symbols.get(&chunk[0]).cloned().unwrap_or_default();
                split_oversized_file(&chunk[0], &syms, line_count(workspace, &chunk[0]))
            } else {
                vec![(chunk, Vec::new(), None)]
            };
            let multi_n = multi.len();
            for (piece_i, (piece, focus, focus_range)) in multi.into_iter().enumerate() {
                seq += 1;
                let mut piece_label = label.clone();
                if n_chunks > 1 {
                    piece_label.push_str(&format!("（分片 {}/{})", chunk_i + 1, n_chunks));
                }
                if multi_n > 1 {
                    piece_label.push_str(&format!("（符号段 {}/{})", piece_i + 1, multi_n));
                }
                units.push(AuditUnit {
                    id: format!("unit-{seq:03}"),
                    files: piece,
                    label: piece_label,
                    focus,
                    focus_range,
                });
            }
        }
    }
    units.sort_by(|a, b| a.id.cmp(&b.id));
    // 重排 id 保证连续（二分插入后序号乱）
    for (i, u) in units.iter_mut().enumerate() {
        u.id = format!("unit-{:03}", i + 1);
    }
    Ok(AuditPlan {
        units,
        source_files: seen.len(),
        excluded_entries,
    })
}

/// 批次文件清单落黑板（第三方 r2 m7）：报告只有 coverage 计数，没有「哪批审了哪些文件」，
/// 分母不可重建。run_audit 在切片后调用（旧的 run_skeleton 只在 M0 骨架期用过，生产无调用方，已删）。
pub fn write_units_file(blackboard: &Path, plan: &AuditPlan) -> Result<()> {
    let path = blackboard.join("audit-units.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(plan).ctx("序列化审计单元")?,
    )
    .ctx(format!("写 {}", path.display()))?;
    tracing::info!(
        component = "audit-engine",
        units = plan.units.len(),
        files = plan.units.iter().map(|u| u.files.len()).sum::<usize>(),
        "全量审计切片完成"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
