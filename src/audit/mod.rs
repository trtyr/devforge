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
mod tests {
    use super::*;

    /// 夹具：8 个源码文件分两个目录（模拟两个模块）。
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (i, dir) in ["src/alpha", "src/beta"].iter().enumerate() {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
            for j in 1..=4 {
                std::fs::write(
                    tmp.path().join(format!("{dir}/m{j}.rs")),
                    format!("// module {i}-{j}\npub fn f{i}_{j}() {{}}\n"),
                )
                .unwrap();
            }
        }
        tmp
    }

    #[test]
    fn slice_groups_by_directory_and_caps_unit_size() {
        let r = repo();
        let plan = build_plan(r.path()).unwrap();
        // 8 文件 / 上限 7 → 至少 2 个单元；alpha 与 beta 不混在同一个 7 文件单元里
        assert!(plan.units.len() >= 2, "应至少切成两单元：{plan:?}");
        let all: Vec<&String> = plan.units.iter().flat_map(|u| u.files.iter()).collect();
        assert_eq!(all.len(), 8, "全部源码文件都应被切片覆盖");
        for u in &plan.units {
            assert!(u.files.len() <= MAX_UNIT_FILES, "单元超限：{u:?}");
            let dirs: std::collections::BTreeSet<String> = u.files.iter().map(|f| {
                Path::new(f).parent().unwrap().to_string_lossy().to_string()
            }).collect();
            assert_eq!(dirs.len(), 1, "同单元不应跨目录（M0 目录聚合语义）：{u:?}");
        }
    }

    /// 第三方 r2 M3：同目录大文件不得整包进同一批——行数预算必须生效。
    #[test]
    fn line_budget_splits_same_directory_big_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/audit")).unwrap();
        // 三个各 600 行的同目录文件：总 1800 行 > MAX_UNIT_LINES(800) → 必切 ≥3 批
        for i in 1..=3 {
            let body: String = (0..600).map(|n| format!("// line {n}\n")).collect();
            std::fs::write(
                tmp.path().join(format!("src/audit/m{i}.rs")),
                format!("{body}pub fn f{i}() {{}}\n"),
            )
            .unwrap();
        }
        let plan = build_plan(tmp.path()).unwrap();
        assert!(
            plan.units.len() >= 3,
            "行数预算未生效（应 ≥3 批）：{:?}",
            plan.units
                .iter()
                .map(|u| u.files.clone())
                .collect::<Vec<_>>()
        );
        for u in &plan.units {
            let lines: usize = u.files.iter().map(|f| line_count(tmp.path(), f)).sum();
            assert!(
                lines <= MAX_UNIT_LINES || u.files.len() == 1,
                "单元行数超限且非单文件：{} 行 {u:?}",
                lines
            );
        }
    }

    /// 第三方 r3 M3：V5 依赖 codegraph——必须强制带 projectPath（本机多仓索引共存时
    /// 不带路径的 explore 可能返回别的仓；官方设计就是每调用传 projectPath）。
    /// 同时钉住 C1 的「只报读过的代码」（它是审查官侧唯一的行为约束）。
    #[test]
    fn prompt_pins_codegraph_project_path_and_read_first() {
        let prompt = include_str!("../../assets/prompts/code-review-agent.md");
        assert!(
            prompt.contains("projectPath"),
            "V5 必须要求 codegraph 调用带 projectPath"
        );
        assert!(
            prompt.contains("没打开过文件就不报"),
            "「只报读过的代码」约束不得被删"
        );
        assert!(
            prompt.contains("代码原文"),
            "证据必须引原文（核验器逐字比对）也要写进提示词"
        );
    }

    /// 第三方 r3 m5：单文件超行数预算 → 按符号边界分片（同文件多单元 + focus 非空），
    /// 不再整包丢给一个会话。
    #[test]
    fn oversized_file_is_split_by_symbol_boundaries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let mut body = String::new();
        // 每 100 行一个函数，共 1800 行（单个文件，不可再按文件切）
        for i in 0..18 {
            for n in 0..99 {
                body.push_str(&format!("// filler {i}-{n}\n"));
            }
            body.push_str(&format!("pub fn f{i}() {{}}\n"));
        }
        std::fs::write(tmp.path().join("src/big.rs"), body).unwrap();
        let plan = build_plan(tmp.path()).unwrap();
        let big: Vec<&AuditUnit> = plan
            .units
            .iter()
            .filter(|u| u.files == vec!["src/big.rs".to_string()])
            .collect();
        assert!(
            big.len() >= 2,
            "超 800 行单文件必须分片（实际 {} 片）",
            big.len()
        );
        for u in &big {
            assert!(!u.focus.is_empty(), "分片必须带 focus（否则等于没切）：{u:?}");
        }
    }

    /// 自审 major 回归：单片行预算必须在分片出口兑现——符号聚集在前部的超大文件，
    /// 旧实现一个组一路划到 total_lines，单片可达几千行，预算完全失效。
    #[test]
    fn oversized_pieces_never_exceed_the_line_budget() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let mut body = String::new();
        // 符号全部聚集在前 100 行，后面 1900 行都是无符号正文
        for i in 0..5 {
            body.push_str(&format!("pub fn g{i}() {{}}\n// pad\n"));
        }
        for n in 0..1900 {
            body.push_str(&format!("// tail filler {n}\n"));
        }
        std::fs::write(tmp.path().join("src/front.rs"), body).unwrap();
        let plan = build_plan(tmp.path()).unwrap();
        let pieces: Vec<&AuditUnit> = plan
            .units
            .iter()
            .filter(|u| u.files == vec!["src/front.rs".to_string()])
            .collect();
        assert!(
            pieces.len() >= 3,
            "2000 行文件必须切成 ≥3 片（实际 {} 片）",
            pieces.len()
        );
        for u in &pieces {
            let (s, e) = u.focus_range.expect("每片必须带行区间");
            assert!(
                e - s + 1 <= MAX_UNIT_LINES,
                "单片 {s}..{e} 共 {} 行，超预算 {MAX_UNIT_LINES}",
                e - s + 1
            );
        }
    }

    /// 自审 major 回归：只落在本片行区间内的符号才进 focus——硬切出的尾片可能
    /// 一个符号都不含，旧实现把整组符号复制给每片，批次指令宣称的「重点符号」
    /// 指向不在本片的行。
    #[test]
    fn single_piece_split_clears_the_focus_claim() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let mut body = String::new();
        body.push_str("pub fn only() {}\n");
        for n in 0..900 {
            body.push_str(&format!("// filler {n}\n"));
        }
        std::fs::write(tmp.path().join("src/solo.rs"), body).unwrap();
        let plan = build_plan(tmp.path()).unwrap();
        let pieces: Vec<&AuditUnit> = plan
            .units
            .iter()
            .filter(|u| u.files == vec!["src/solo.rs".to_string()])
            .collect();
        assert_eq!(pieces.len(), 2, "符号聚首 + 900 行正文 → 预算硬切成 2 片");
        let (s1, e1) = pieces[0].focus_range.unwrap();
        let (s2, e2) = pieces[1].focus_range.unwrap();
        assert!((s1, e1) == (1, 800) && (s2, e2) == (801, e2), "硬切边界要对");
        assert!(!pieces[0].focus.is_empty(), "首片含符号，focus 非空");
        assert!(
            pieces[1].focus.is_empty(),
            "尾片无符号，不得宣称重点符号：{:?}",
            pieces[1].focus
        );
    }

    #[test]
    fn slice_is_deterministic() {
        let r = repo();
        let a = build_plan(r.path()).unwrap();
        let b = build_plan(r.path()).unwrap();
        assert_eq!(a, b, "同输入切片必须确定性一致");
    }

    #[test]
    fn non_source_entries_are_not_sliced() {
        // Behavior/Config 条目（docgen 也会收）不进切片面
        let r = repo();
        let plan = build_plan(r.path()).unwrap();
        let all: Vec<&String> = plan.units.iter().flat_map(|u| u.files.iter()).collect();
        assert!(
            all.iter().all(|f| f.ends_with(".rs")),
            "切片只含源码：{all:?}"
        );
    }
}
