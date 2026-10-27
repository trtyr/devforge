//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, fixture::*, milestone::*, panic::*, runlog::*, support::*};

/// 从源码里取某个函数的函数体（按花括号配平）。
pub(super) fn fn_body(text: &str, name: &str) -> Option<String> {
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(name) {
        let at = from + pos;
        let prev = text[..at].chars().next_back().unwrap_or(' ');
        let prev_ok = !prev.is_alphanumeric() && prev != '_';
        let after = text[at + name.len()..].chars().next();
        if prev_ok && matches!(after, Some('(') | Some('<')) {
            let open = text[at..].find('{')? + at;
            let mut depth = 0i32;
            for (i, c) in text[open..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(text[open..open + i + 1].to_string());
                        }
                    }
                    _ => {}
                }
            }
            return None;
        }
        from = at + name.len();
    }
    None
}

#[test]
pub(super) fn the_heartbeat_answers_which_phase_the_process_died_in() {
    // 错实现形态：进程在两阶段之间异常死亡后心跳被删 / 不含最后阶段 → 答不出「死在哪」
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let root = dir.path().to_path_buf();
    let run_id = "run-f6-death";
    obs::begin_run_in(root.clone(), run_id, "plan");
    obs::set_phase("coding");
    // 模拟异常死亡：不清位（没有 end_run），也不删文件
    let hb = obs::heartbeat::read(&root, run_id).expect("心跳应能回答死在哪个阶段");
    assert_eq!(hb.phase, "coding", "心跳要停在死亡前最后到达的阶段");
    obs::end_run(run_id);
}

#[test]
pub(super) fn the_heartbeat_and_tombstone_land_in_fixed_subdirectories_named_by_the_run_id() {
    // 错实现形态：落点目录名不对（不在 heartbeat/ 与 tombstones/ 下）或文件名不含 run id
    // → 本用例必须变红
    let hb = obs::heartbeat::heartbeat_path(Path::new("/root"), "run-9");
    assert_eq!(hb, PathBuf::from("/root/heartbeat/run-9.json"));
    let tomb = obs::tombstone::tombstone_path(Path::new("/root"), 1_700_000_000_000, "run-9");
    assert_eq!(tomb, PathBuf::from("/root/tombstones/1700000000000-run-9.txt"));
}

#[tokio::test]
pub(super) async fn the_server_run_writes_the_heartbeat_into_the_fixture_dir_not_the_user_home() {
    // 错实现形态：仓内走真实 start 路径的 run 把心跳写进用户 HOME（用户 HOME 被写脏）
    // → 本用例必须变红
    let _cap = Capture::new();
    let home_heartbeat = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".devforge").join("heartbeat"));
    let before = home_heartbeat
        .as_ref()
        .map(|d| std::fs::read_dir(d).map(|it| it.count()).unwrap_or(0))
        .unwrap_or(0);
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "planning").await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t1").await;
    // 心跳落在夹具的临时目录里，且 run 结束后**保留**最后一次状态
    // （F18 后首写异步化：有界轮询等落盘，不假设 Settled 瞬间文件已在）
    let hb_dir = f.obs_root.join("heartbeat");
    let files: Vec<PathBuf>;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        let current: Vec<PathBuf> = std::fs::read_dir(&hb_dir)
            .map(|it| it.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        if !current.is_empty() || std::time::Instant::now() > deadline {
            files = current;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(files.len(), 1, "run 结束后心跳应保留（不删）：{files:?}");
    assert!(
        files[0].file_name().unwrap().to_string_lossy().ends_with(".json"),
        "心跳文件名带 run id：{files:?}"
    );
    // 用户 HOME 的 .devforge/heartbeat 不新增文件
    if let Some(dir) = &home_heartbeat {
        let after = std::fs::read_dir(dir).map(|it| it.count()).unwrap_or(0);
        assert_eq!(
            after, before,
            "用户 HOME 的 .devforge/heartbeat/ 不该被仓内单测写脏：{dir:?}"
        );
    }
}

// ============================================================ 需求 4 / 边界口径

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(super) const CHANGE_ID: &str = "change-20261022-c7-1-start-src-server-tests-rs-145-fixtu";
pub(super) const PLAN_REL: &str =
    "changes/change-20261022-c7-1-start-src-server-tests-rs-145-fixtu/change-plan.md";
pub(super) const TEST_STAGE_FILE: &str = "tests/f6_observability/main.rs";

/// 计划 §4 白名单（从计划文件里抽反引号列表项 → 数据驱动，不另抄一份）。
pub(super) fn plan_whitelist() -> Vec<String> {
    let text = std::fs::read_to_string(
        devforge::change::prepare::default_plantree_root("devforge").join(PLAN_REL),
    )
    .expect("读计划");
    let section = text
        .split("## 4 白名单")
        .nth(1)
        .expect("计划里应有「## 4 白名单」")
        .split("\n## ")
        .next()
        .expect("白名单小节");
    let mut out = Vec::new();
    for line in section.lines() {
        let t = line.trim();
        if !t.starts_with("- `") {
            continue;
        }
        let Some(rest) = t.strip_prefix("- `") else {
            continue;
        };
        let Some(end) = rest.find('`') else { continue };
        out.push(rest[..end].to_string());
    }
    out
}

/// 从测试文件里抽 `const NAME: &[&str] = &[ ... ];` 的条目。
pub(super) fn const_list(text: &str, name: &str) -> Vec<String> {
    let start = text
        .find(&format!("const {name}:"))
        .unwrap_or_else(|| panic!("文件里应有常量 {name}"));
    let body = &text[start..];
    let open = body
        .find("= &[")
        .map(|i| i + 4)
        .unwrap_or_else(|| panic!("常量 {name} 应是数组"));
    let close = body[open..]
        .find("];")
        .map(|i| i + open)
        .expect("数组有结尾");
    body[open..close]
        .lines()
        // 注释行不是常量条目（守卫常量里允许写注释说明）→ 跳过，否则解析结果会把
        // 注释当成白名单文件，失败信息失真
        .filter(|l| !l.trim().starts_with("//"))
        .map(|l| l.trim().trim_matches(',').trim().trim_matches('"').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// 从测试文件里抽 `const NAME: &str = "..."` 的值。
pub(super) fn const_str(text: &str, name: &str) -> String {
    let marker = format!("const {name}: &str = \"");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("文件里应有常量 {name}"));
    let rest = &text[start + marker.len()..];
    let end = rest.find('"').expect("字符串有结尾");
    rest[..end].to_string()
}

/// 本场基线（台账口径：`plantree/<project>/changes/<change-id>/baseline.txt`；
/// 台账已移出仓 2026-10-25）。
pub(super) fn baseline_of_record() -> String {
    std::fs::read_to_string(
        devforge::change::prepare::default_plantree_root("devforge")
            .join(format!("changes/{CHANGE_ID}/baseline.txt")),
    )
    .expect("读基线记录")
    .trim()
    .to_string()
}

#[test]
pub(super) fn the_guard_constants_point_at_this_change() {
    // 错实现形态：守卫常量没刷新（BASELINE 还指着上一场 / WHITELIST 少文件或多文件）
    // → 本用例必须变红
    let baseline = baseline_of_record();
    assert!(!baseline.is_empty(), "基线记录不该为空");
    let plan = plan_whitelist();
    let coding: Vec<String> = plan
        .iter()
        .filter(|p| *p != TEST_STAGE_FILE)
        .cloned()
        .collect();
    assert_eq!(plan.len(), 24, "计划 §4 白名单 = 24 个文件：{plan:?}");
    assert_eq!(coding.len(), 23, "coding 步白名单 = 23 个文件：{coding:?}");

    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("读守卫文件");
    let failure = std::fs::read_to_string(repo_root().join("tests/change_failure_context/support.rs"))
        .expect("读验收文件");
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol/main.rs"))
        .expect("读验收文件");

    for (name, text) in [
        ("守卫", guard.as_str()),
        ("失败上下文验收", failure.as_str()),
        ("协议验收", protocol.as_str()),
    ] {
        assert_eq!(
            const_str(text, "BASELINE"),
            baseline,
            "{name} 的 BASELINE 必须等于本场基线 sha"
        );
    }
    // 守卫的 WHITELIST = coding 步白名单（23 个，逐个列出）
    let listed = const_list(&guard, "WHITELIST");
    assert_eq!(listed.len(), 23, "守卫 WHITELIST 必须有 23 个文件：{listed:?}");
    for f in &coding {
        assert!(listed.iter().any(|x| x == f), "守卫 WHITELIST 缺 {f}");
    }
    // 三份副本逐字一致
    assert_eq!(
        const_list(&failure, "WHITELIST"),
        listed,
        "两份 WHITELIST 常量必须逐字一致"
    );
    assert_eq!(
        const_list(&protocol, "CODING_WHITELIST"),
        listed,
        "上一场验收文件的 CODING_WHITELIST 必须逐字一致"
    );
}

/// 取某个声明**正上方**的文档注释块（连续的 `///` 行）。
pub(super) fn doc_comment_above(text: &str, marker: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        // 可见性前缀兼容（ARC-R6 mod 化后声明常带 pub(super)）：同声明即命中
        let bare = line.trim_start();
        let bare = bare.strip_prefix("pub(super) ").unwrap_or(bare);
        let bare = bare.strip_prefix("pub(crate) ").unwrap_or(bare);
        let bare = bare.strip_prefix("pub ").unwrap_or(bare);
        if bare.starts_with(marker) {
            break;
        }
        if line.trim_start().starts_with("///") {
            out.push(line.trim_start().trim_start_matches("///").trim());
        } else if !line.trim().is_empty() {
            out.clear(); // 非注释行 → 注释块断开
        }
    }
    out.join("\n")
}

#[test]
pub(super) fn the_guard_comments_match_their_constant_values() {
    // 错实现形态：注释里留着陈旧的基线 sha / 白名单个数（读者按注释复核会数出旧值）
    //   → 本用例必须变红（R-4）
    let baseline = baseline_of_record();
    let short = &baseline[..7];
    for rel in [
        "tests/change_ask_ring.rs",
        "tests/change_failure_context/support.rs",
        "tests/change_review_protocol/main.rs",
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        for stale in ["18ff755", "22 个", "a0be947"] {
            assert!(
                !text.contains(stale),
                "{rel} 残留陈旧口径「{stale}」（注释要与常量的实际值一致）"
            );
        }
        let baseline_comment = doc_comment_above(&text, "const BASELINE");
        assert!(
            baseline_comment.contains(short),
            "{rel} 的 BASELINE 注释要写本场基线 {short}：{baseline_comment}"
        );
    }
    // 白名单常量的注释要写白名单个数（coding 步 23 个 / 计划 24 个）
    for (rel, marker, needle) in [
        ("tests/change_ask_ring.rs", "const WHITELIST", "23 个"),
        (
            "tests/change_failure_context/support.rs",
            "const WHITELIST",
            "23 个",
        ),
        (
            "tests/change_review_protocol/main.rs",
            "const CODING_WHITELIST",
            "23 个",
        ),
        (
            "tests/change_review_protocol/main.rs",
            "const PLAN_WHITELIST",
            "24 个",
        ),
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        let comment = doc_comment_above(&text, marker);
        assert!(
            comment.contains(needle),
            "{rel} 的 {marker} 注释要写「{needle}」：{comment}"
        );
    }
}

#[test]
pub(super) fn the_test_stage_file_is_excluded_from_the_coding_step_whitelist() {
    // 错实现形态：测试阶段文件被塞进 coding 步白名单常量（两场之间恒红的来源）
    // → 本用例必须变红
    let plan = plan_whitelist();
    assert!(
        plan.iter().any(|p| p == TEST_STAGE_FILE),
        "计划白名单要含唯一的测试阶段文件"
    );
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("读守卫文件");
    let listed = const_list(&guard, "WHITELIST");
    assert!(
        !listed.iter().any(|x| x == TEST_STAGE_FILE),
        "测试阶段文件不得进 coding 步白名单常量：{listed:?}"
    );
    // a29 的排除项断言写的就是这个测试阶段文件
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol/main.rs"))
        .expect("读验收文件");
    assert!(
        protocol.contains(TEST_STAGE_FILE),
        "a29 的排除项断言要指向本场测试阶段文件"
    );
}

#[test]
pub(super) fn the_plan_whitelist_has_twenty_four_files_and_the_coding_whitelist_twenty_three() {
    // 错实现形态：白名单数目写错（22/25）或测试阶段文件缺失 → 本用例必须变红
    // （10-22 操作员补录：§4 扩容 +1（coding_gate.rs）后应为 24/23）
    let protocol = std::fs::read_to_string(repo_root().join("tests/change_review_protocol/main.rs"))
        .expect("读验收文件");
    let plan_list = const_list(&protocol, "PLAN_WHITELIST");
    let coding_list = const_list(&protocol, "CODING_WHITELIST");
    assert_eq!(plan_list.len(), 24, "计划白名单 = 24 个文件：{plan_list:?}");
    assert_eq!(coding_list.len(), 23, "coding 步白名单 = 23 个文件：{coding_list:?}");
    assert_eq!(
        plan_list,
        plan_whitelist(),
        "验收文件里的计划白名单必须与计划 §4 一致"
    );
    assert!(
        plan_list.iter().any(|f| f == TEST_STAGE_FILE),
        "计划白名单要含唯一的测试阶段文件"
    );
    assert_eq!(
        plan_list
            .iter()
            .filter(|f| *f != TEST_STAGE_FILE)
            .cloned()
            .collect::<Vec<String>>(),
        coding_list,
        "coding 白名单 = 计划白名单去掉测试阶段文件"
    );
}

#[test]
pub(super) fn the_guard_keeps_its_ignore_between_two_changes() {
    // 错实现形态：两场之间的守卫被摘掉 ignore（守卫恒红 → 变成噪音）或 ignore 被抹掉
    // → 本用例必须变红
    for rel in [
        "tests/change_ask_ring.rs",
        "tests/change_failure_context/guards.rs",
        "tests/change_review_protocol/guards.rs",
    ] {
        let text = std::fs::read_to_string(repo_root().join(rel)).expect("读守卫文件");
        assert!(
            text.contains("#[ignore"),
            "{rel} 的守卫应保持 ignore（两场之间无 step 提交）"
        );
    }
}

#[test]
pub(super) fn the_decision_ledger_has_the_d006_section() {
    // 错实现形态：D006 缺三件交付物 / 缺渲染闸门口径 / 缺分级口径，或 D001–D005 被改动
    // → 本用例必须变红
    let rel = "plans/010-run-resume/decisions/README.md";
    let current = std::fs::read_to_string(
        devforge::change::prepare::default_plantree_root("devforge").join(rel),
    )
    .expect("读决策台账");
    assert!(current.contains("## D006 ·"), "决策台账要有 D006 一节");
    for needle in [
        "原始错误",
        "里程碑",
        "墓碑",
        "心跳",
        "渲染闸门",
        "ERROR",
        "WARN",
        "配置错误",
    ] {
        assert!(
            current.contains(needle),
            "D006 要写明「{needle}」这一口径"
        );
    }
    // 既有 D001–D005 一字不动（与本变更基线版本逐字比对）
    let before_d006 = current
        .split("## D006 ·")
        .next()
        .expect("应有 D006 之前的部分")
        .trim_end()
        .to_string();
    let baseline_version = git_out(
        &repo_root(),
        &[
            "show",
            &format!(
                "{}:docs/plantree/plans/010-run-resume/decisions/README.md",
                baseline_of_record()
            ),
        ],
    );
    assert_eq!(
        before_d006,
        baseline_version.trim_end(),
        "既有 D001–D005 条目一字不动"
    );
}

#[test]
pub(super) fn the_plan_audit_has_no_findings() {
    // 错实现形态：计划里出现别的机械门 finding（六块不全 / path:line 不存在 / 白名单外改动 /
    //   「白名单里的文件不存在」）→ 本用例必须变红
    // （计划期预期的 4 条「白名单里的文件不存在」已随四个新增文件落盘消解 → 现状必须是 0 条）
    let plan = std::fs::read_to_string(
        devforge::change::prepare::default_plantree_root("devforge").join(PLAN_REL),
    )
    .expect("读计划");
    let audit = devforge::change::plan_audit::audit_with_plantree(
        &plan,
        &repo_root(),
        &devforge::change::prepare::default_plantree_root("devforge"),
    );
    assert!(
        audit.issues.is_empty(),
        "机械门审计的 finding 必须为 0 条：{audit:?}"
    );
    assert!(audit.missing_blocks.is_empty(), "六块必须齐全：{audit:?}");
}

#[test]
#[ignore = "F10：守卫随场次烙死——c7b 已 Settled，基线后移后本守卫仅在下一场 run 内生效（本注释代 ignore 复贴，10-23 收官）"]
pub(super) fn the_commits_stay_inside_the_plan_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来
    // → 本用例必须变红
    let baseline = baseline_of_record();
    let plan = plan_whitelist();
    let changed = git_out(
        &repo_root(),
        &["diff", "--name-only", &format!("{baseline}..HEAD")],
    );
    for f in changed.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            plan.iter().any(|p| p == f),
            "提交内容越界（计划 §4 白名单外）：{f}"
        );
    }
    for artifact in [
        ".pi/goals/",
        "tests/e2e/results/",
        "docs/plantree/changes/",
    ] {
        let touched = git_out(
            &repo_root(),
            &[
                "log",
                "--format=%H",
                "--name-only",
                &format!("{baseline}..HEAD"),
                "--",
                artifact,
            ],
        );
        assert!(
            touched.trim().is_empty(),
            "未跟踪产物 {artifact} 被提交进来了：{touched}"
        );
    }
}

