//! BuiltinRpcEngine 行为测试（ARC-R2 自 server/mod.rs 内嵌 coding_engine_tests 迁入）：
//! 分发路由 / 门 0 preflight / F1 门卫 / per-SOP profile 表 / map_intervention 错误种类。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::domain::task::Task;
use crate::engine::builtin::{preflight_codegraph, BuiltinRpcEngine};
use crate::observability::events::EventBus;
use crate::server::UiRelay;

#[test]
fn preflight_inits_when_no_index() {
    // decisions/013：工作区无索引 → 引擎调用 init（假脚本被调时建 .codegraph 标记）
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let bin = dir.path().join("fake-codegraph.sh");
    std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    preflight_codegraph(&ws, &bin);
    assert!(ws.join(".codegraph").exists(), "预检应调用 init 建索引");
}

#[test]
fn preflight_skips_when_index_exists() {
    // 已有索引 → 不调 init（指向不存在的可执行，若被调即报错暴露）
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(ws.join(".codegraph")).unwrap();
    preflight_codegraph(&ws, Path::new("/nonexistent/codegraph"));
    assert!(ws.join(".codegraph").exists());
}

#[test]
fn dispatch_routes_change_to_its_engine() {
    // 注册表与引擎同步：change 已接线，不应再落到「未接线」分支
    let mut engine = BuiltinRpcEngine {
        profile_root: PathBuf::from("/tmp/devforge-nonexistent-profiles"),
        session_root: PathBuf::from("/tmp"),
        plans_root: PathBuf::from("/tmp"),
        ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let mut task = Task::new("t", "p", "change", 1);
    task.workspace = PathBuf::from("/tmp");
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    let msg = err.to_string();
    assert!(!msg.contains("未接线"), "change 必须已接线：{msg}");
    assert!(
        msg.contains("change-plan-agent"),
        "应卡在 profile 门卫（说明真的进了 run_change）：{msg}"
    );
}

#[test]
fn change_without_requirement_file_is_quality_failure() {
    // 门 0 前先要需求：黑板里没 requirement.md 不得开跑
    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles");
    for p in crate::server::change_engine::CHANGE_PROFILES {
        let d = profiles.join(p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
    }
    let plans = dir.path().join("plans");
    let mut engine = BuiltinRpcEngine {
        profile_root: profiles,
        session_root: dir.path().join("sessions"),
        plans_root: plans,
        ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let ws = dir.path().join("repo");
    std::fs::create_dir_all(&ws).unwrap();
    let mut task = Task::new("t", "p", "change", 1);
    task.workspace = ws;
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("requirement.md"), "{err}");
    assert!(err.is_quality_failure());
}

#[test]
fn change_run_preflights_codegraph_before_plan() {
    // change-plan-agent 被要求 codegraph-first：run_change 必须先建索引（与其他代码类 SOP 同口径）
    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles");
    for p in crate::server::change_engine::CHANGE_PROFILES {
        let d = profiles.join(p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
    }
    let ws = dir.path().join("repo");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/a.rs"), "// a\n").unwrap();
    // 假 codegraph：init 时在当前工作区建 .codegraph 标记
    let bin = dir.path().join("fake-codegraph");
    std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let plans = dir.path().join("plans");
    let mut engine = BuiltinRpcEngine {
        profile_root: profiles,
        session_root: dir.path().join("sessions"),
        plans_root: plans.clone(),
        ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
        codegraph_bin: bin,
        idle_ms: None,
    };
    // 需求故意缺失：preflight 先于需求门，能验到预检且不会 spawn 真 pi
    let task_root = plans.join("p").join("t");
    std::fs::create_dir_all(&task_root).unwrap();
    let mut task = Task::new("t", "p", "change", 1);
    task.workspace = ws.clone();
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("requirement.md"), "{err}");
    assert!(
        ws.join(".codegraph").exists(),
        "run_change 必须先做 codegraph 预检（plan agent 依赖图）"
    );
}

#[test]
fn dispatch_rejects_unwired_sop() {
    // 注册表与引擎同步性：未知/未接线 SOP 在 execute 入口即拒，不 spawn 任何进程
    let mut engine = BuiltinRpcEngine {
        profile_root: PathBuf::from("/tmp"),
        session_root: PathBuf::from("/tmp"),
        plans_root: PathBuf::from("/tmp"),
        ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let task = Task::new("t", "p", "ghost", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("未接线"));
}

// ---------- F1：spawn 前 profile 门卫（e2e F1-MISSING-PROFILE-HANG） ----------

fn f1_engine(profile_root: std::path::PathBuf, plans_root: std::path::PathBuf) -> BuiltinRpcEngine {
    BuiltinRpcEngine {
        profile_root,
        session_root: PathBuf::from("/tmp"),
        plans_root,
        ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    }
}

#[test]
fn profile_gate_passes_when_rendered() {
    // 已渲染（目录 + settings.json 标记）→ 放行
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("coding-agent");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
    engine.require_profile("coding-agent").unwrap();
}

#[test]
fn profile_gate_fails_naming_missing_path() {
    // 缺失 → 报错含角色目录与标记文件路径，指明先渲染
    let dir = tempfile::tempdir().unwrap();
    let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
    let err = engine.require_profile("coding-agent").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
    assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
    assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
}

#[test]
fn run_coding_fails_before_side_effects_when_profiles_missing() {
    // 编码 run 在 profile 缺失时立即失败（不再真起 pi 挂 Running），
    // 且校验先于一切副作用：黑板骨架不应被创建
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
    assert!(
        !plans.join("p").join("t").exists(),
        "校验应先于副作用：黑板不应被创建"
    );
}

#[test]
fn run_planning_fails_before_side_effects_when_profiles_missing() {
    // 规划路径同样受门卫保护：缺失即失败，plan root 不落盘
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
    let task = Task::new("t", "p", "planning", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
    assert!(
        !plans.join("p").join("t").exists(),
        "校验应先于副作用：plan root 不应被创建"
    );
}

// ---------- P007 T006：testing / doc-fix 引擎接线 ----------

#[test]
fn run_testing_fails_before_side_effects_when_profiles_missing() {
    // 测试 run 在 profile 缺失时立即失败，且 testing/ 骨架不应被创建
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
    let task = Task::new("t", "p", "testing", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
    assert!(
        !plans.join("p").join("t").exists(),
        "校验应先于副作用：黑板不应被创建"
    );
}

#[test]
fn run_doc_fix_fails_before_side_effects_when_profiles_missing() {
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
    let task = Task::new("t", "p", "doc-fix", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
    assert!(!plans.join("p").join("t").exists(), "校验应先于副作用");
}

#[test]
fn testing_start_requires_workspace_after_profile_gate() {
    // profile 已渲染但无工作区 → workspace 门（测试需要项目地址写测/起被测系统）
    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles");
    for role in ["test-agent", "test-review"] {
        let p = profiles.join(role);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("settings.json"), "{}").unwrap();
    }
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(profiles, plans.clone());
    let task = Task::new("t", "p", "testing", 1); // workspace 默认空
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("无代码工作区"), "应报 workspace 门: {err}");
    assert!(
        !plans.join("p").join("t").join("testing").exists(),
        "workspace 门也应先于骨架创建"
    );
}

/// 第三方 r2 M2：code-review 空工作区闸——与其他代码类 SOP 同口径（spawn 前快速失败）。
#[test]
fn code_review_start_requires_workspace_after_profile_gate() {
    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles");
    std::fs::create_dir_all(profiles.join("code-review")).unwrap();
    std::fs::write(profiles.join("code-review/settings.json"), "{}").unwrap();
    let plans = dir.path().join("plans");
    let mut engine = f1_engine(profiles, plans.clone());
    let task = Task::new("t", "p", "code-review", 1); // workspace 默认空
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(err.to_string().contains("无代码工作区"), "应报 workspace 门: {err}");
    assert!(
        !plans.join("p").join("t").exists(),
        "workspace 门也应先于黑板创建"
    );
}

// ---------- 010 F6 靶：per-SOP profile 表（门卫与闸门的唯一事实源） ----------

#[test]
fn profiles_for_sop_is_the_single_source_for_the_gate_and_the_door() {
    // 错实现形态：闸门另抄一份 profile 清单（改了 SOP 的需求，闸门还在等旧清单）
    // → 本用例必须变红
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("planning"),
        ["plan-agent", "plan-review"].as_slice()
    );
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("coding"),
        ["coding-agent", "coding-review"].as_slice()
    );
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("testing"),
        ["test-agent", "test-review"].as_slice()
    );
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("doc-from-zero"),
        ["doc-agent", "doc-review"].as_slice()
    );
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("doc-fix"),
        ["doc-fix", "doc-review"].as_slice()
    );
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("change"),
        crate::server::change_engine::CHANGE_PROFILES.as_slice()
    );
    // 引擎审计修复 #5：锁住 code-review 表项——删表项走空切片，门卫会放行未渲染 profile
    assert_eq!(
        BuiltinRpcEngine::profiles_for_sop("code-review"),
        ["code-review"].as_slice()
    );
    assert!(
        BuiltinRpcEngine::profiles_for_sop("unknown-sop").is_empty(),
        "未知 SOP 没有 profile 需求"
    );
}

#[test]
fn the_door_checks_every_profile_of_the_sop() {
    // 错实现形态：门卫只查干活 agent（漏掉审查角色）→ 缺审查 profile 也能开跑
    // → 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let profiles = dir.path().join("profiles");
    let agent = profiles.join("coding-agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("settings.json"), "{}").unwrap();
    let mut engine = f1_engine(profiles, dir.path().join("plans"));
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine.execute(&task, &run, &control).unwrap_err();
    assert!(
        err.to_string().contains("coding-review"),
        "审查 profile 缺失也要被门卫拦住：{err}"
    );
}

// ---------- 010 F6 靶：原始错误可见 + profile 门卫的错误种类 ----------

#[test]
fn profile_gate_failure_is_a_config_error_not_an_infrastructure_failure() {
    // 错实现形态：仍用 `System`（会被 `is_infra_failure` 判成基础设施故障 → 盲重启 3 次）
    // → 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
    let err = engine.require_profile("coding-agent").unwrap_err();
    assert!(
        matches!(err, crate::error::DevforgeError::Config(_)),
        "profile 未就绪应是配置错误（不是基础设施故障）：{err:?}"
    );
    assert!(
        !crate::engine::policy::is_infra_failure(&Err(err)),
        "配置错误不得被判为基础设施故障（否则会盲重启）"
    );
}

#[test]
fn profile_gate_failure_keeps_its_text_unchanged() {
    // 错实现形态：改写错误文案（既有单测与 e2e 靠它指路）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
    let err = engine.require_profile("coding-agent").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
    assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
    assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
    assert!(
        msg.contains("先 PUT /api/v1/config/manifest"),
        "报错应带处置建议: {msg}"
    );
}

#[test]
fn quality_failure_in_map_intervention_is_logged_as_warn_with_the_retry_layer_fields() {
    // 错实现形态：①质量失败落 ERROR（应 WARN）②缺 `error_chain` / `attempt` / `retryable`
    // ③只留既有那条无字段的 WARN → 本用例必须变红
    let text = crate::observability::test_support::capture_jsonl(|| {
        let outcome = crate::engine::policy::map_intervention(
            &RunControl::new(),
            "change-engine",
            crate::error::DevforgeError::Quality("越界改动未申报".into()),
        )
        .unwrap();
        assert!(
            matches!(outcome, RunOutcome::Failed { .. }),
            "质量失败仍是明确的 Failed（不重启）"
        );
    });
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_failed"))
        .collect();
    assert_eq!(lines.len(), 1, "质量失败应落一条同形状条目：{text}");
    let line = lines[0];
    assert!(line.contains("\"level\":\"WARN\""), "质量失败应落 WARN：{line}");
    assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
    assert!(line.contains("\"component\":\"change-engine\""), "{line}");
    assert!(line.contains("\"attempt\":"), "缺 attempt 字段：{line}");
    assert!(line.contains("\"retryable\":false"), "质量失败不重启：{line}");
    assert!(line.contains("越界改动未申报"), "缺原始错误全文：{line}");
    assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
}

#[test]
fn the_quality_warning_of_a_later_attempt_carries_that_attempts_number() {
    // 错实现形态：attempt 写在**失败分支**里 → 第 2 次尝试期间上下文还是第 1 次的值
    //   → 这条质量 WARN 带 attempt=1（陈旧）→ 本用例必须变红
    let control = RunControl::new();
    let text = crate::observability::test_support::capture_jsonl(|| {
        // 第 1 次：基础设施故障（会被自动重启）；第 2 次：质量失败（走生产链路的落点 ②）
        let (result, attempts) = crate::engine::policy::run_with_retry(
            &crate::engine::policy::RetryPolicy::fast(),
            &control,
            |n| {
                if n == 1 {
                    Err(crate::error::DevforgeError::System(
                        "模拟上游 500（测试注入）".into(),
                    ))
                } else {
                    crate::engine::policy::map_intervention(
                        &control,
                        "change-engine",
                        crate::error::DevforgeError::Quality("终审未通过".into()),
                    )
                }
            },
        );
        assert_eq!(attempts, 2, "第 1 次基础设施故障后应重启到第 2 次");
        assert!(
            matches!(result, Ok(RunOutcome::Failed { .. })),
            "质量失败仍是明确的 Failed（不重启）"
        );
    });
    let quality: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_failed") && l.contains("\"level\":\"WARN\""))
        .collect();
    assert_eq!(quality.len(), 1, "质量失败应落一条 WARN 条目：{text}");
    assert!(
        quality[0].contains("\"attempt\":2"),
        "第 2 次尝试的质量 WARN 必须带**本次**尝试号（不是上一次的 1）：{}",
        quality[0]
    );
}

#[test]
fn testing_watch_dirs_cover_handoff_sessions() {
    // D011：testing run 的活性监视覆盖移交会话（doc-fix/doc-reviews 同 run 活性整体）
    let engine = f1_engine(PathBuf::from("/tmp/profiles"), PathBuf::from("/tmp"));
    let dirs = engine.testing_watch_dirs("run-x");
    let names: Vec<_> = dirs.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
    assert_eq!(names, vec!["testing", "testing-reviews", "doc-fix", "doc-reviews"]);
    // 引擎批次 #4：会话按 run-id 隔离，监视目录里必须带同一个 run-id
    assert!(
        dirs.iter().all(|p| p.parent().unwrap().file_name().unwrap() == "run-x"),
        "监视目录必须落在 sessions/<run-id>/：{dirs:?}"
    );
    let names = engine
        .docfix_watch_dirs("run-x")
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["doc-fix", "doc-reviews"]);
}
