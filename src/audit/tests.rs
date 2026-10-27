//! （2026-10-26 测试分离：自 src/audit/mod.rs 内联平移，纯移动零改动。）
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

// ── 2026-10-27 自审 audit/mod 批回归 ──

/// 自审 minor（4）：首片必须从第 1 行起——文件头（模块文档/import）不得落在所有片之外。
#[test]
fn first_piece_starts_at_line_one_covering_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("src");
    std::fs::create_dir_all(&dir).unwrap();
    // 2000 行文件：头部 50 行注释 + 若干符号
    let mut body = String::new();
    for i in 0..50 {
        body.push_str(&format!("// header line {i}\n"));
    }
    for f in 0..5 {
        body.push_str(&format!("fn f{f}() {{\n"));
        for _ in 0..380 {
            body.push_str("    let _x = 1;\n");
        }
        body.push_str("}\n");
    }
    std::fs::write(dir.join("big.rs"), body).unwrap();
    let plan = build_plan(tmp.path()).unwrap();
    let pieces: Vec<_> = plan
        .units
        .iter()
        .filter(|u| u.files.iter().any(|f| f.ends_with("big.rs")))
        .collect();
    assert!(!pieces.is_empty(), "big.rs 必须进切片");
    // 契约：**文件头不得落在所有片之外**。两种合法形态：
    // ① 整文件一片（range = 1..=n 或 None = 整文件）；
    // ② 多片时最小起点必须是第 1 行。
    let starts: Vec<usize> = pieces
        .iter()
        .map(|u| u.focus_range.map(|(a, _)| a).unwrap_or(1))
        .collect();
    assert_eq!(
        starts.iter().copied().min(),
        Some(1),
        "首片必须从第 1 行起（旧口径从首个符号行起，文件头不属于任何片）：{starts:?}"
    );
}

/// 自审 minor（7）：单元 id 超过三位数时排序不得错序（字典序会让 unit-1000 排在 unit-999 前）。
#[test]
fn unit_ids_sort_numerically_beyond_three_digits() {
    let tmp = tempfile::tempdir().unwrap();
    // 造 1010 个小文件，每个独立目录 → 每个目录一批，id 会超过 999
    for i in 0..1010 {
        let dir = tmp.path().join(format!("m{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), format!("fn f{i}() {{}}\n")).unwrap();
    }
    let plan = build_plan(tmp.path()).unwrap();
    let ids: Vec<String> = plan.units.iter().map(|u| u.id.clone()).collect();
    let nums: Vec<usize> = ids
        .iter()
        .map(|i| i.trim_start_matches("unit-").parse().unwrap())
        .collect();
    assert_eq!(nums.len(), 1010, "每个目录一批");
    assert!(
        nums.windows(2).all(|w| w[0] < w[1]),
        "id 必须数值递增（旧口径字典序会错序）"
    );
}

/// 自审 minor（5）：排除计数只算**整份文件从未进切片**的路径，不得把已切片文件的
/// 其它条目也算成「没进切片」。
#[test]
fn excluded_entries_counts_files_not_entries() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    // 一个普通源文件（会有 Module + Symbol 等多条目）
    std::fs::write(
        tmp.path().join("src/a.rs"),
        "pub fn a() {}\npub fn b() {}\n",
    )
    .unwrap();
    let plan = build_plan(tmp.path()).unwrap();
    assert!(!plan.units.is_empty());
    assert!(
        plan.excluded_entries <= plan.source_files,
        "排除计数（{}）不得大于源文件数（{}）——旧口径按条目计会越界",
        plan.excluded_entries,
        plan.source_files
    );
}
