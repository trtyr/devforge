//! 调度器单测（P012 M4）：就绪派工 / 依赖失败阻塞 / 容量拒绝 / 预算熔断 / 自动入队 / 独占窗口。

use super::{body_json, fixture, send};
use crate::domain::store::FinishOutcome;
use crate::domain::task::{TaskDeps, TaskStatus};
use crate::domain::{now_ms, SopRegistry, Store};
use crate::server::scheduler::{
    apply_startup_concurrency, resolve_max_concurrent, tick, ConcurrencySource, SchedulerConfig,
};
use serde_json::json;

/// 建任务（绕过 API，直接操作 store——tick 单测只关心状态面）。
fn make_task(f: &super::Fixture, project: &str, id: &str, deps: TaskDeps) {
    let mut store = f.state.store.lock().unwrap();
    if store.get_project(project).is_none() {
        store
            .create_project(crate::domain::project::Project::new(
                project,
                std::path::PathBuf::from("/tmp/sched-fixture"),
                1,
            ))
            .unwrap();
    }
    store
        .create_task(&SopRegistry::builtin(), project, id, "planning", None, deps)
        .unwrap();
}

fn status_of(f: &super::Fixture, project: &str, id: &str) -> TaskStatus {
    f.state
        .store
        .lock()
        .unwrap()
        .get_task(project, id)
        .unwrap()
        .status
}

#[tokio::test]
async fn ready_tasks_are_dispatched_in_dependency_order() {
    let f = fixture();
    make_task(&f, "devforge", "dag-a", TaskDeps::default());
    make_task(
        &f,
        "devforge",
        "dag-b",
        TaskDeps { depends_on: vec!["dag-a".into()], exclusive: false },
    );
    // a 跑完（Completed），b 就绪
    {
        let mut store = f.state.store.lock().unwrap();
        let run = store.begin_run("devforge", "dag-a").unwrap();
        store.finish_run(&run.id, FinishOutcome::Completed).unwrap();
    }
    let cfg = SchedulerConfig::default();
    let report = tick(&f.state, &cfg);
    // a 已 Completed 不再派；b 依赖全 Completed → 派工
    assert!(
        report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "dag-b"),
        "b 应就绪派工：{report:?}"
    );
    assert!(
        !report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "dag-a"),
        "已完成的 a 不应再派"
    );
}

#[tokio::test]
async fn failed_dependency_blocks_with_reason() {
    let f = fixture();
    make_task(&f, "devforge", "dep-a", TaskDeps::default());
    make_task(
        &f,
        "devforge",
        "dep-b",
        TaskDeps { depends_on: vec!["dep-a".into()], exclusive: false },
    );
    {
        let mut store = f.state.store.lock().unwrap();
        let run = store.begin_run("devforge", "dep-a").unwrap();
        store
            .finish_run(&run.id, FinishOutcome::Failed { reason: "炸了".into() })
            .unwrap();
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    let entry = report
        .blocked
        .iter()
        .find(|(t, _)| t == "devforge/dep-b")
        .expect("dep-b 应被阻塞");
    assert_eq!(entry.1, "dependency_failed");
    assert_eq!(status_of(&f, "devforge", "dep-b"), TaskStatus::Blocked);
}

#[tokio::test]
async fn capacity_saturation_blocks_and_recovers() {
    let f = fixture();
    // store 硬上限默认 1：注入一个 Running run 占满
    make_task(&f, "devforge", "busy", TaskDeps::default());
    make_task(&f, "devforge", "queued", TaskDeps::default());
    {
        let mut store = f.state.store.lock().unwrap();
        store.begin_run("devforge", "busy").unwrap();
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    let entry = report
        .blocked
        .iter()
        .find(|(t, _)| t == "devforge/queued")
        .expect("queued 应因容量被阻塞");
    assert_eq!(entry.1, "capacity");
    assert_eq!(status_of(&f, "devforge", "queued"), TaskStatus::Blocked);

    // 恢复联动：busy 收尾 → 再 tick → queued 回 Pending 并进入派工计划
    {
        let mut store = f.state.store.lock().unwrap();
        let run_id = store
            .runs
            .iter()
            .find(|r| r.task_id == "busy" && r.status == crate::domain::run::RunStatus::Running)
            .unwrap()
            .id
            .clone();
        store.finish_run(&run_id, FinishOutcome::Completed).unwrap();
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "queued"),
        "容量释放后 queued 应恢复派工：{report:?}"
    );
}

#[tokio::test]
async fn over_budget_run_is_fused() {
    let f = fixture();
    make_task(&f, "devforge", "long-runner", TaskDeps::default());
    // 模拟真实派工：干预面已在派发前登记（自审 minor 223 后的口径），
    // 这样熔断才能置位到真正被 run 线程看见的那个 control 上。
    let control = f.state.register_control("devforge", "long-runner");
    {
        let mut store = f.state.store.lock().unwrap();
        let run = store.begin_run("devforge", "long-runner").unwrap();
        // 人为老化 started_at_ms（300 分钟前 > 预算 240 分钟）
        if let Some(r) = store.runs.iter_mut().find(|r| r.id == run.id) {
            r.started_at_ms = now_ms().saturating_sub(300 * 60_000);
        }
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        report.fused.contains(&"devforge/long-runner".to_string()),
        "超预算 run 应熔断：{report:?}"
    );
    // 自审 minor 228：旧口径把断言包在 `if let Some(c)` 里——在册与否都跳过，
    // 删掉熔断实现仍全绿。这里 control 由测试自己登记，断言必须是硬断言。
    assert!(control.is_cancelled(), "熔断应置位取消（在册 control 必须被置位）");
}

#[tokio::test]
async fn over_budget_run_without_a_control_handle_is_closed_outright() {
    let f = fixture();
    make_task(&f, "devforge", "lost-run", TaskDeps::default());
    let run_id = {
        let mut store = f.state.store.lock().unwrap();
        let run = store.begin_run("devforge", "lost-run").unwrap();
        if let Some(r) = store.runs.iter_mut().find(|r| r.id == run.id) {
            r.started_at_ms = now_ms().saturating_sub(300 * 60_000);
        }
        run.id
    };
    // 不登记 control：模拟「run 线程已经不在，但 store 里仍是 Running」（占着并发槽）
    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(report.fused.contains(&"devforge/lost-run".to_string()), "{report:?}");
    let store = f.state.store.lock().unwrap();
    let row = store.runs.iter().find(|r| r.id == run_id).expect("run 应在库里");
    assert_eq!(
        row.status,
        crate::domain::run::RunStatus::Failed,
        "无控制句柄且 kille=0 时必须升级结案，否则并发槽永久泄漏"
    );
}

#[tokio::test]
async fn settled_contract_task_auto_enqueues() {
    let f = fixture();
    let plans_root = f.state.plans_root.clone();
    // 契约走 create_task 提交（get_task_mut 是 store 私有——外部从创建时给）
    {
        let mut store = f.state.store.lock().unwrap();
        store
            .create_project(crate::domain::project::Project::new(
                "devforge",
                std::path::PathBuf::from("/tmp/sched-fixture"),
                1,
            ))
            .unwrap();
        store
            .create_task(
                &SopRegistry::builtin(),
                "devforge",
                "auto-int",
                "planning",
                Some(crate::domain::contract::WorkItemContract {
                    owned_paths: vec!["src/**".into()],
                    forbidden_paths: vec![],
                    base_rev: Some("deadbeef".into()),
                    isolation: crate::domain::contract::Isolation::None,
                }),
                TaskDeps::default(),
            )
            .unwrap();
        let run = store.begin_run("devforge", "auto-int").unwrap();
        store.finish_run(&run.id, FinishOutcome::Completed).unwrap();
    }
    // frozen-commit.txt（M1 产物）落盘
    let frozen_dir = plans_root.join("devforge").join("auto-int");
    std::fs::create_dir_all(&frozen_dir).unwrap();
    std::fs::write(frozen_dir.join("frozen-commit.txt"), "cafebabe00000000000000000000000000000000\n").unwrap();

    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        report
            .auto_enqueued
            .contains(&"devforge/auto-int".to_string()),
        "Settled 带契约任务应自动入队：{report:?}"
    );
    let queue =
        crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    let cand = queue
        .candidates
        .iter()
        .find(|c| c.task_id == "auto-int")
        .expect("候选应在队列");
    assert_eq!(cand.frozen_sha, "cafebabe00000000000000000000000000000000");
    assert_eq!(cand.base_rev, "deadbeef");
    assert_eq!(cand.owned_paths, vec!["src/**".to_string()]);
}

#[tokio::test]
async fn exclusive_running_window_blocks_dispatch() {
    let f = fixture();
    make_task(
        &f,
        "devforge",
        "solo",
        TaskDeps { depends_on: vec![], exclusive: true },
    );
    make_task(&f, "devforge", "other", TaskDeps::default());
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 4; // 放开容量，只验独占语义
        store.begin_run("devforge", "solo").unwrap();
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    let entry = report
        .blocked
        .iter()
        .find(|(t, _)| t == "devforge/other")
        .expect("独占窗口期 other 应停派");
    assert_eq!(entry.1, "exclusive_window");
    assert!(
        report
            .dispatched
            .iter()
            .all(|(p, t)| p == "devforge" && t == "solo"),
        "独占窗口期不应有任何派工：{report:?}"
    );
}

#[tokio::test]
async fn scheduler_tick_survives_empty_store() {
    let f = fixture();
    let report = tick(&f.state, &SchedulerConfig::default());
    assert_eq!(report, crate::server::scheduler::TickReport::default());
}

#[tokio::test]
async fn scheduler_status_endpoint_reports_snapshot() {
    // M5 · T5.1：纯 API 回答「在飞/卡在哪/队列多长」——status 快照零副作用
    let f = fixture();
    make_task(&f, "devforge", "flying", TaskDeps::default());
    make_task(
        &f,
        "devforge",
        "stuck",
        TaskDeps { depends_on: vec!["flying".into()], exclusive: false },
    );
    {
        let mut store = f.state.store.lock().unwrap();
        store.begin_run("devforge", "flying").unwrap(); // flying 在飞；stuck 依赖未就绪（静默等待）
    }
    let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["running"][0]["task"], "flying");
    assert!(
        body["running"][0]["elapsed_ms"].is_u64(),
        "在飞应带墙钟：{body}"
    );
    // stuck 依赖未完成 → 既不 runnable 也不 blocked（静默等待）；队列空仓 0
    assert_eq!(body["runnable"].as_array().unwrap().len(), 0);
    assert_eq!(body["queue"][0]["project"], "devforge");
    assert_eq!(body["queue"][0]["total"], 0);

    // 队列有货场景：入队一个候选 → queue 计数可见
    {
        let mut q =
            crate::integration::MergeQueue::load(&f.state.plans_root, "devforge").unwrap();
        q.enqueue(crate::integration::Candidate {
            task_id: "flying".into(),
            frozen_sha: "beef".into(),
            base_rev: "dead".into(),
            owned_paths: vec![],
            status: crate::integration::CandidateStatus::Pending,
            evidence_paths: vec![],
        });
        q.save().unwrap();
    }
    let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["queue"][0]["total"], 1);
    assert_eq!(body["queue"][0]["pending"], 1);
    assert_eq!(body["queue"][0]["needs_repair"], 0, "无冲突时冲突面为 0");
}

/// 复验 #3 回归：同一 tick 两个就绪的独占任务，最多派工一个（后台 begin_run 前集合不变，
/// 不追踪本 tick 派工就会双独占并行）。
#[tokio::test]
async fn same_tick_double_exclusive_dispatches_at_most_one() {
    let f = fixture();
    make_task(
        &f,
        "devforge",
        "ex-a",
        TaskDeps { depends_on: vec![], exclusive: true },
    );
    make_task(
        &f,
        "devforge",
        "ex-b",
        TaskDeps { depends_on: vec![], exclusive: true },
    );
    let cfg = SchedulerConfig::default();
    let report = tick(&f.state, &cfg);
    let dispatched_ex = report
        .dispatched
        .iter()
        .filter(|(_, t)| t.contains("ex-"))
        .count();
    assert_eq!(dispatched_ex, 1, "同 tick 双独占只能派一个：{report:?}");
    assert!(
        report
            .blocked
            .iter()
            .any(|(t, r)| t.contains("ex-") && r == "exclusive_window"),
        "另一个应阻塞在独占窗口：{report:?}"
    );
}

/// 复验 #3 回归：容量为 1 且已有一个 run 在跑时，同 tick 不得再派工（预留计数）。
#[tokio::test]
async fn capacity_one_with_running_run_dispatches_nothing() {
    let f = fixture();
    make_task(&f, "devforge", "busy", TaskDeps::default());
    make_task(&f, "devforge", "waiting", TaskDeps::default());
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 1;
        // busy：置 Running（模拟已派工且 begin_run 完成）
        let run = store.begin_run("devforge", "busy").unwrap();
        let _ = run;
    }
    let cfg = SchedulerConfig::default();
    let report = tick(&f.state, &cfg);
    assert!(
        !report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "waiting"),
        "容量 1 已占满，waiting 不得派工：{report:?}"
    );
    assert!(
        report
            .blocked
            .iter()
            .any(|(t, r)| t.contains("waiting") && r == "capacity"),
        "waiting 应阻塞在容量：{report:?}"
    );
}

/// 建「带契约 + 已完成 + frozen-commit 落盘」的任务（自动入队用例的公共前置）。
fn settled_contract_task(f: &super::Fixture, task: &str, frozen_sha: &str) {
    let plans_root = f.state.plans_root.clone();
    {
        let mut store = f.state.store.lock().unwrap();
        if store.get_project("devforge").is_none() {
            store
                .create_project(crate::domain::project::Project::new(
                    "devforge",
                    std::path::PathBuf::from("/tmp/sched-fixture"),
                    1,
                ))
                .unwrap();
        }
        store
            .create_task(
                &SopRegistry::builtin(),
                "devforge",
                task,
                "planning",
                Some(crate::domain::contract::WorkItemContract {
                    owned_paths: vec!["src/**".into()],
                    forbidden_paths: vec![],
                    base_rev: Some("deadbeef".into()),
                    isolation: crate::domain::contract::Isolation::None,
                }),
                TaskDeps::default(),
            )
            .unwrap();
        let run = store.begin_run("devforge", task).unwrap();
        store.finish_run(&run.id, FinishOutcome::Completed).unwrap();
    }
    let dir = plans_root.join("devforge").join(task);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("frozen-commit.txt"), format!("{frozen_sha}\n")).unwrap();
}

/// 全仓审计 #2 回归：NeedsRebase 不得把任务永久卡死——
/// 错实现形态：next_pending 只挑 Pending + scheduler 任意状态都跳过入队 → 一次 CAS
/// 失败后该任务既不被消费也不再入队，静默停摆。
#[tokio::test]
async fn needs_rebase_candidate_is_rearmed_and_not_stuck() {
    let f = fixture();
    let plans_root = f.state.plans_root.clone();
    settled_contract_task(&f, "rebased", "cafebabe00000000000000000000000000000000");

    // 先入队，再模拟 CAS 失败后的状态
    let mut queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    queue.enqueue(crate::integration::Candidate {
        task_id: "rebased".into(),
        frozen_sha: "cafebabe00000000000000000000000000000000".into(),
        base_rev: "deadbeef".into(),
        owned_paths: vec!["src/**".into()],
        status: crate::integration::CandidateStatus::NeedsRebase {
            target_moved_to: "beefcafe".into(),
        },
        evidence_paths: vec![],
    });
    queue.save().unwrap();

    let report = tick(&f.state, &SchedulerConfig::default());
    let _ = report;
    let queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    let mine: Vec<_> = queue
        .candidates
        .iter()
        .filter(|c| c.task_id == "rebased")
        .collect();
    assert_eq!(mine.len(), 1, "不得重复入队：{queue:?}");
    assert_eq!(
        mine[0].status,
        crate::integration::CandidateStatus::Pending,
        "NeedsRebase 应放回 Pending 重验（否则永久停滞）"
    );
}

/// 全仓审计 #2 回归的另一面：NeedsRepair 是终态，但**任务重跑出新证据**（新 frozen_sha）
/// 必须能再入队——旧口径「任意状态跳过」会让修复后的任务永远进不了集成。
#[tokio::test]
async fn needs_repair_allows_reentry_with_new_frozen_commit() {
    let f = fixture();
    let plans_root = f.state.plans_root.clone();
    settled_contract_task(&f, "repaired", "1111111111111111111111111111111111111111");

    let mut queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    queue.enqueue(crate::integration::Candidate {
        task_id: "repaired".into(),
        frozen_sha: "1111111111111111111111111111111111111111".into(),
        base_rev: "deadbeef".into(),
        owned_paths: vec!["src/**".into()],
        status: crate::integration::CandidateStatus::NeedsRepair {
            reason: "组合验证失败".into(),
        },
        evidence_paths: vec![],
    });
    queue.save().unwrap();

    // 同一 sha：不重复入队（同一批失败重投只会再失败一次）
    tick(&f.state, &SchedulerConfig::default());
    let queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    assert_eq!(
        queue.candidates.iter().filter(|c| c.task_id == "repaired").count(),
        1,
        "同一 frozen_sha 不应重复入队"
    );

    // 修复后重跑 → 新 frozen_sha → 允许再入队
    std::fs::write(
        plans_root.join("devforge").join("repaired").join("frozen-commit.txt"),
        "2222222222222222222222222222222222222222\n",
    )
    .unwrap();
    tick(&f.state, &SchedulerConfig::default());
    let queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    let shas: Vec<_> = queue
        .candidates
        .iter()
        .filter(|c| c.task_id == "repaired")
        .map(|c| c.frozen_sha.clone())
        .collect();
    assert!(
        shas.contains(&"2222222222222222222222222222222222222222".to_string()),
        "新证据应允许再入队：{shas:?}"
    );
}

// ================= 并发优先级链（F13） =================

/// 需求1：env 合法值最高优先（`DEVFORGE_MAX_CONCURRENT=4` 压过持久值 2）。
/// 错实现 A（持久值优先）→ 这条红。
#[test]
fn an_env_value_wins_over_the_persisted_value() {
    let r = resolve_max_concurrent(Some("4"), Some(2), 2);
    assert_eq!(r.value, 4);
    assert_eq!(r.source, ConcurrencySource::Env);
    assert_eq!(r.ignored_env, None);
}

/// 需求1/2：env 可以**压低**持久值——串行（1）由此真实可达。
/// 错实现 A'（env 只能抬高）→ 这条红。
#[test]
fn an_env_value_can_lower_the_persisted_value() {
    let r = resolve_max_concurrent(Some("1"), Some(4), 2);
    assert_eq!(r.value, 1);
    assert_eq!(r.source, ConcurrencySource::Env);
}

/// 需求1：env 值 trim 后合法即采用（" 3 " → 3）。
/// 错实现 B'（不 trim → 解析失败回退）→ 这条红。
#[test]
fn an_env_value_is_trimmed_before_parsing() {
    let r = resolve_max_concurrent(Some(" 3 "), Some(4), 2);
    assert_eq!(r.value, 3);
    assert_eq!(r.source, ConcurrencySource::Env);
}

/// 需求1：非法 env（0 / 负数 / 非数字）回退下一优先级，原值留痕。
/// 错实现 C'（非法 env 直接拒启动 / 静默采用）→ 这条红。
#[test]
fn an_illegal_env_value_falls_back_and_keeps_the_raw_value() {
    for raw in ["0", "-2", "abc"] {
        let r = resolve_max_concurrent(Some(raw), Some(4), 2);
        assert_eq!(r.value, 4, "非法 env 应回退持久值：{raw:?}");
        assert_eq!(
            r.source,
            ConcurrencySource::Store,
            "非法 env 应回退持久值：{raw:?}"
        );
        assert_eq!(
            r.ignored_env.as_deref(),
            Some(raw),
            "非法 env 原值须留痕（WARN 的触发条件）：{raw:?}"
        );
    }
}

/// 需求1：空串与纯空白同样非法（触发 WARN 条件），原样留痕。
#[test]
fn a_blank_env_value_is_illegal_and_keeps_the_raw_value() {
    for raw in ["", "   "] {
        let r = resolve_max_concurrent(Some(raw), Some(4), 2);
        assert_eq!(r.value, 4);
        assert_eq!(r.source, ConcurrencySource::Store);
        assert_eq!(r.ignored_env.as_deref(), Some(raw));
    }
}

/// 需求1：没有 env 时按持久值走（3 → 3；1 → 1，串行不被抬高）。
#[test]
fn without_an_env_value_the_persisted_value_is_used() {
    let r = resolve_max_concurrent(None, Some(3), 2);
    assert_eq!(r.value, 3);
    assert_eq!(r.source, ConcurrencySource::Store);
    let r = resolve_max_concurrent(None, Some(1), 2);
    assert_eq!(r.value, 1, "持久串行选择不得被抬高");
    assert_eq!(r.source, ConcurrencySource::Store);
}

/// 需求1：持久值 0 按 1 处理（与 `Store::begin_run` 的 cap 同口径）。
#[test]
fn a_persisted_zero_counts_as_one() {
    let r = resolve_max_concurrent(None, Some(0), 2);
    assert_eq!(r.value, 1);
    assert_eq!(r.source, ConcurrencySource::Store);
}

/// 需求1：没有任何配置时取缺省 2（无 env、无可信持久值）。
#[test]
fn without_any_configuration_the_default_is_used() {
    let r = resolve_max_concurrent(None, None, 2);
    assert_eq!(r.value, 2);
    assert_eq!(r.source, ConcurrencySource::Default);
}

/// 需求1：缺省常量仍是 2（`SchedulerConfig::default()` 是链末位）。
/// 错实现 D'（把缺省改成 1）→ 这条红。
#[test]
fn the_default_config_still_provides_the_fallback_of_two() {
    assert_eq!(SchedulerConfig::default().max_concurrent_mutation, 2);
}

/// 需求1/4：首启（无可信持久值）且未设 env → 取缺省 2 并回写 store。
#[test]
fn a_first_boot_without_an_env_value_falls_back_to_two() {
    let mut store = Store::new();
    let r = apply_startup_concurrency(&mut store, None, false);
    assert_eq!(store.max_concurrent_runs, 2);
    assert_eq!(r.value, 2);
    assert_eq!(r.source, ConcurrencySource::Default);
}

/// 需求1/4：可信持久值既不抬也不压。
#[test]
fn a_trusted_persisted_value_is_neither_raised_nor_lowered() {
    let mut store = Store::new();
    store.max_concurrent_runs = 4;
    let r = apply_startup_concurrency(&mut store, None, true);
    assert_eq!(store.max_concurrent_runs, 4);
    assert_eq!(r.source, ConcurrencySource::Store);
}

/// 需求1/4：env 覆盖持久值并回写 store（用户裁决：回写生效值）。
/// 错实现 E'（env 不回写 → store 仍是旧值）→ 这条红。
#[test]
fn an_env_value_overrides_the_persisted_value_and_is_written_back() {
    let mut store = Store::new();
    store.max_concurrent_runs = 4;
    let r = apply_startup_concurrency(&mut store, Some("1"), true);
    assert_eq!(store.max_concurrent_runs, 1);
    assert_eq!(r.value, 1);
    assert_eq!(r.source, ConcurrencySource::Env);
}

/// 需求1/4：可信持久串行值不被抬高。
#[test]
fn a_trusted_serial_value_is_not_raised() {
    let mut store = Store::new();
    store.max_concurrent_runs = 1;
    let r = apply_startup_concurrency(&mut store, None, true);
    assert_eq!(store.max_concurrent_runs, 1);
    assert_eq!(r.source, ConcurrencySource::Store);
}

/// 需求1/4：首启也能靠 env 定值。
#[test]
fn an_env_value_sets_the_value_on_a_first_boot() {
    let mut store = Store::new();
    let r = apply_startup_concurrency(&mut store, Some("3"), false);
    assert_eq!(store.max_concurrent_runs, 3);
    assert_eq!(r.source, ConcurrencySource::Env);
}

/// 需求1/4：老 `store.json` 缺 `max_concurrent_runs` 字段（serde 缺省 1）→ 生效 1。
/// 错实现 F'（按缺省 2 抬起来）→ 这条红。
#[test]
fn a_store_file_without_the_field_keeps_the_serial_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    std::fs::write(&path, r#"{"projects":[],"tasks":[],"runs":[]}"#).unwrap();
    let mut store = Store::load(&path).expect("老 store.json 应可加载");
    assert_eq!(store.max_concurrent_runs, 1, "serde 缺省应为 1");
    let r = apply_startup_concurrency(&mut store, None, true);
    assert_eq!(store.max_concurrent_runs, 1, "老库升级后按持久值 1（串行）");
    assert_eq!(r.source, ConcurrencySource::Store);
}

/// 需求1/4：回写即落盘——落地后 save → load 回读到生效值（重启不丢）。
#[test]
fn the_effective_value_is_written_back_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let mut store = Store::new();
    store.max_concurrent_runs = 2;
    store.save(&path).unwrap();
    let mut store = Store::load(&path).unwrap();
    apply_startup_concurrency(&mut store, Some("1"), true);
    assert_eq!(store.max_concurrent_runs, 1);
    store.save(&path).unwrap();
    let reloaded = Store::load(&path).unwrap();
    assert_eq!(reloaded.max_concurrent_runs, 1, "回写必须落盘（重启不丢）");
}

/// 需求1/4：生效值未变 → `previous == value`（调用方据此不写盘）。
#[test]
fn an_unchanged_value_reports_previous_equal_to_the_value() {
    let mut store = Store::new();
    store.max_concurrent_runs = 4;
    let r = apply_startup_concurrency(&mut store, None, true);
    assert_eq!(r.previous, r.value);
    assert_eq!(r.previous, 4);
}

/// 需求1/6.6：落地必须留痕——INFO 含 `resolved` / `source` / `previous` 三字段。
/// 错实现 G'（漏 INFO 或漏字段 → 事后无从复盘「为什么并发变成 1」）→ 这条红。
#[test]
fn the_startup_resolution_is_logged_with_value_source_and_previous() {
    let mut store = Store::new();
    store.max_concurrent_runs = 4;
    let capture = EventCapture::default();
    let r = tracing::subscriber::with_default(capture.clone(), || {
        // tracing 的 callsite 兴趣缓存是进程级的：此前若在「无订阅者」状态下被缓存成
        // never，事件就永不再派发 → 这里显式重建，让本次订阅者参与缓存计算。
        tracing::callsite::rebuild_interest_cache();
        apply_startup_concurrency(&mut store, Some("1"), true)
    });
    assert_eq!(r.value, 1);
    let fields = capture
        .event_named("scheduler.max_concurrent_resolved")
        .expect("落地必须落一条 INFO");
    assert_eq!(field(&fields, "resolved"), Some("1"));
    assert_eq!(field(&fields, "source"), Some("env"));
    assert_eq!(field(&fields, "previous"), Some("4"));
}

/// 需求1/6.6：env 非法 → WARN（`scheduler.max_concurrent_env_invalid`）带原值与 source。
/// 错实现 H'（非法 env 静默回退、不留痕）→ 这条红。
#[test]
fn an_illegal_env_value_is_logged_as_a_warning_with_the_raw_value() {
    let mut store = Store::new();
    store.max_concurrent_runs = 4;
    let capture = EventCapture::default();
    let r = tracing::subscriber::with_default(capture.clone(), || {
        tracing::callsite::rebuild_interest_cache();
        apply_startup_concurrency(&mut store, Some("0"), true)
    });
    assert_eq!(r.value, 4);
    assert_eq!(r.ignored_env.as_deref(), Some("0"));
    let fields = capture
        .event_named("scheduler.max_concurrent_env_invalid")
        .expect("非法 env 必须落一条 WARN");
    assert_eq!(field(&fields, "invalid_env"), Some("0"));
    assert_eq!(field(&fields, "source"), Some("store"));
}

/// 需求2：生效并发 1 时，一次 tick 只派一个，另一个阻塞在 `capacity` 且**未 begin_run**。
/// 错实现 I'（容量不看 store）→ 这条红。
#[tokio::test]
async fn a_single_slot_dispatches_one_task_and_blocks_the_other() {
    let f = fixture();
    make_task(&f, "devforge", "serial-a", TaskDeps::default());
    make_task(&f, "devforge", "serial-b", TaskDeps::default());
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 1;
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    assert_eq!(report.dispatched.len(), 1, "单槽一次只派一个：{report:?}");
    assert_eq!(report.blocked.len(), 1, "另一个必须被阻塞：{report:?}");
    assert_eq!(report.blocked[0].1, "capacity");
    let blocked_key = report.blocked[0].0.clone();
    let store = f.state.store.lock().unwrap();
    let runs_of_blocked = store
        .runs
        .iter()
        .filter(|r| format!("{}/{}", r.task_project, r.task_id) == blocked_key)
        .count();
    assert_eq!(runs_of_blocked, 0, "被阻塞的任务不得 begin_run：{blocked_key}");
}

/// 需求2：并发上限 1 时直接 `begin_run` 被拒（错误文案点并发上限）。
#[tokio::test]
async fn a_second_run_is_rejected_when_the_effective_slot_is_one() {
    let f = fixture();
    make_task(&f, "devforge", "holder", TaskDeps::default());
    make_task(&f, "devforge", "second", TaskDeps::default());
    let mut store = f.state.store.lock().unwrap();
    store.max_concurrent_runs = 1;
    store.begin_run("devforge", "holder").unwrap();
    let err = store
        .begin_run("devforge", "second")
        .expect_err("并发上限 1 时第二个 run 必须被拒");
    assert!(
        err.to_string().contains("已达并发上限 1"),
        "错误文案须点并发上限：{err}"
    );
}

/// 需求2：槽释放后被阻塞的任务恢复派工，随后能 begin_run 且在飞 run 恰好 1 个。
#[tokio::test]
async fn a_blocked_task_is_dispatched_after_the_slot_is_freed() {
    let f = fixture();
    make_task(&f, "devforge", "holder", TaskDeps::default());
    make_task(&f, "devforge", "waiting", TaskDeps::default());
    let run_id = {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 1;
        store.begin_run("devforge", "holder").unwrap().id
    };
    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        report
            .blocked
            .iter()
            .any(|(t, r)| t == "devforge/waiting" && r == "capacity"),
        "槽占满时 waiting 应阻塞在容量：{report:?}"
    );
    {
        let mut store = f.state.store.lock().unwrap();
        store.finish_run(&run_id, FinishOutcome::Completed).unwrap();
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "waiting"),
        "槽释放后 waiting 应恢复派工：{report:?}"
    );
    let mut store = f.state.store.lock().unwrap();
    store
        .begin_run("devforge", "waiting")
        .expect("恢复派工后应能 begin_run");
    assert_eq!(store.running_run_count(), 1);
}

/// 需求2：连续 tick 不得突破生效并发 1（确定性状态推进，不用 sleep 猜时序）。
/// 错实现 J'（容量判定放宽）→ 这条红。
#[tokio::test]
async fn repeated_ticks_never_exceed_the_effective_concurrency() {
    let f = fixture();
    for id in ["busy-a", "busy-b", "busy-c"] {
        make_task(&f, "devforge", id, TaskDeps::default());
    }
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 1;
    }
    for _ in 0..3 {
        let report = tick(&f.state, &SchedulerConfig::default());
        for (project, task) in &report.dispatched {
            // 调度器口径：派工后由后台线程 begin_run → 这里同步模拟一次，槽位才真实占用。
            let mut store = f.state.store.lock().unwrap();
            store
                .begin_run(project, task)
                .expect("派工任务应能起 run");
        }
        let store = f.state.store.lock().unwrap();
        assert!(
            store.running_run_count() <= 1,
            "生效并发 1 时在飞 run 不得超过 1"
        );
    }
}

/// 需求2：生效并发 4 时四个就绪任务一次 tick 全派（上调入口真能开闸）。
/// 错实现 K'（仍被缺省 2 / 容量恒 1 压住）→ 这条红。
#[tokio::test]
async fn a_wider_effective_concurrency_dispatches_more_tasks() {
    let f = fixture();
    for id in ["wide-a", "wide-b", "wide-c", "wide-d"] {
        make_task(&f, "devforge", id, TaskDeps::default());
    }
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 4;
    }
    let report = tick(&f.state, &SchedulerConfig::default());
    assert_eq!(
        report.dispatched.len(),
        4,
        "生效并发 4 时应一次派 4 个：{report:?}"
    );
}

/// 需求3：status 两字段同报生效值（同一把锁内读出的同一个数）。
/// 错实现 L'（两字段各报各的）→ 这条红。
#[tokio::test]
async fn the_status_endpoint_reports_the_effective_concurrency_in_both_fields() {
    // 0 视为 1（与 `Store::begin_run` 的 cap 同口径）→ 两字段都报 1，不报 0。
    for (stored, expected) in [(0usize, 1usize), (1, 1), (3, 3)] {
        let f = fixture();
        {
            let mut store = f.state.store.lock().unwrap();
            store.max_concurrent_runs = stored;
        }
        let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        assert_eq!(
            body["max_concurrent_runs"], expected,
            "max_concurrent_runs 须报生效值：{body}"
        );
        assert_eq!(
            body["effective_max_concurrent"], expected,
            "effective_max_concurrent 须报生效值：{body}"
        );
    }
}

/// 需求3：status 顶层字段集合不增不减（既有 status 用例保持绿）。
///
/// gap-B 起新增 `channel_breakers`（运维不进机器就能看见哪条渠道在冷却）——
/// 本断言同步纳入该字段，防止将来再有字段被悄悄塞进或拿掉。
#[tokio::test]
async fn the_status_endpoint_keeps_its_top_level_field_set() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let obj = body.as_object().expect("status 响应须是对象");
    let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "blocked",
            "channel_breakers",
            "effective_max_concurrent",
            // gap-E：自动集成面（开关态 + 窗口态 + 目标 ref + 可选门）
            "integration",
            // P020：issues 面板源（open 总数 + 按严重度分布）
            "issues",
            "max_concurrent_runs",
            "queue",
            "runnable",
            "running",
        ],
        "status 顶层字段集合不得增删：{body}"
    );
}

/// 需求2：直接 start 端点与调度器同口径——生效并发 1 时第二个 start 当场 409。
/// 错实现 M'（直接 start 绕过并发上限）→ 这条红。
#[tokio::test]
async fn a_direct_start_is_rejected_when_the_effective_slot_is_one() {
    let f = fixture();
    send(
        &f.router,
        "POST",
        "/api/v1/projects",
        Some(json!({"name": "p", "local_path": "/tmp/ws-serial"})),
    )
    .await;
    for id in ["t1", "t2"] {
        send(
            &f.router,
            "POST",
            "/api/v1/projects/p/tasks",
            Some(json!({"id": id, "sop": "planning"})),
        )
        .await;
    }
    {
        let mut store = f.state.store.lock().unwrap();
        store.max_concurrent_runs = 1;
        store.begin_run("p", "t1").expect("占位 run 应能起");
    }
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t2/start",
        Some(json!({})),
    )
    .await;
    assert_eq!(resp.status(), 409, "生效并发 1 时直接 start 必须当场拒");
}

/// 需求4：`src/server/scheduler.rs` 的并发口径注释与新优先级链一致（旧口径清零）。
/// 错实现 N'（残留 min(cfg) / 原子字段读写）→ 这条红。
#[test]
fn the_scheduler_source_carries_no_stale_concurrency_wording() {
    let src = include_str!("../scheduler.rs");
    for stale in ["min(cfg", "store.max_concurrent_runs.max(cfg"] {
        assert!(!src.contains(stale), "调度器源码仍带旧并发口径：{stale}");
    }
    // 原子并发字段（`ServerState.effective_max_concurrent`）在本文件零读写：
    // 代码行里只剩 JSON 键那一个出现（注释里的「已废弃」说明不算读写）。
    let code_hits = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter(|l| l.contains("effective_max_concurrent"))
        .count();
    assert_eq!(
        code_hits, 1,
        "原子并发字段必须零读写（代码行里只剩 JSON 键）：{src}"
    );
    // 优先级链的三级必须在注释里写明（谁提供生效值）。
    assert!(src.contains("DEVFORGE_MAX_CONCURRENT"), "注释须点名 env 键");
    assert!(src.contains("store.max_concurrent_runs"), "注释须点名 store 真相源");
    // status 注释须写明「两字段同报生效值」与「原子字段已废弃」（计划 6.4 的缓解口径）。
    assert!(
        src.contains("同报生效值") && src.contains("已废弃"),
        "status 注释须写明两字段同报生效值 + 原子字段已废弃"
    );
}

/// 一次事件采集到的字段（字段名 → 值）。
type CapturedFields = Vec<(String, String)>;

/// 收集 tracing 事件（线程局部默认订阅者，只在本用例作用域内生效——不与并行用例串味）。
#[derive(Clone, Default)]
struct EventCapture(std::sync::Arc<std::sync::Mutex<Vec<CapturedFields>>>);

impl EventCapture {
    /// 取第一条 `event` 字段等于给定名字的事件的全部字段。
    fn event_named(&self, name: &str) -> Option<Vec<(String, String)>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|fields| fields.iter().any(|(k, v)| k == "event" && v == name))
            .cloned()
    }
}

/// 从事件字段里取一个值（找不到 → None）。
fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// 字段收集器（把 tracing 的字段值抓成可断言的字符串）。
#[derive(Default)]
struct FieldCapture(Vec<(String, String)>);

impl tracing::field::Visit for FieldCapture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_string(), format!("{value:?}")));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_string(), value.to_string()));
    }
}

impl tracing::Subscriber for EventCapture {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = FieldCapture::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

// ── 2026-10-31（autonomy-gaps gap-E）：集成窗口与开关面 ──

/// 集成窗口期：不派新 run、只记 `blocked{integration_window}`、**不改任务状态**。
///
/// 错实现形态：窗口期照常派工（窗口旁路漏了）→ 派工断言红；
/// 或窗口期把任务置 Blocked（拿状态当开关用）→ 状态断言红。
#[tokio::test]
async fn integration_window_blocks_dispatch_without_touching_task_status() {
    let f = fixture();
    make_task(&f, "devforge", "win-task", TaskDeps::default());
    let cfg = SchedulerConfig::default();
    let before = status_of(&f, "devforge", "win-task");
    assert_ne!(before, TaskStatus::Blocked, "前置：任务应是 Pending");

    let report = crate::server::scheduler::tick_with_integration_window(&f.state, &cfg, true);
    assert!(
        report.dispatched.is_empty(),
        "集成窗口期不得派工：{report:?}"
    );
    let entry = report
        .blocked
        .iter()
        .find(|(t, _)| t == "devforge/win-task")
        .expect("窗口期任务应被阻塞");
    assert_eq!(entry.1, "integration_window");
    assert_eq!(
        status_of(&f, "devforge", "win-task"),
        before,
        "窗口期不得改任务状态（瞬态现象不写进 store）"
    );
}

/// 对照组：窗口关掉 → 同一个任务恢复派工（证明上条的阻塞确实来自窗口）。
///
/// 错实现形态：窗口判定写死成「永远阻塞」→ 本用例红。
#[tokio::test]
async fn dispatch_resumes_when_integration_window_is_closed() {
    let f = fixture();
    make_task(&f, "devforge", "win-open", TaskDeps::default());
    let cfg = SchedulerConfig::default();

    let report = crate::server::scheduler::tick_with_integration_window(&f.state, &cfg, false);
    assert!(
        report
            .dispatched
            .iter()
            .any(|(p, t)| p == "devforge" && t == "win-open"),
        "窗口关时照常派工：{report:?}"
    );
}

/// 开关面与窗口面在 `scheduler/status` 里同源可见：缺省关、改文件即开。
///
/// 错实现形态：status 读的是进程内缓存（改了文件仍报旧值）→ 第三段断言红。
#[tokio::test]
async fn scheduler_status_reports_integration_switch_and_window_state() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(
        body["integration"]["auto_integrate_enabled"], false,
        "缺省开关必须关（自主合并需显式授权）：{body}"
    );
    assert_eq!(body["integration"]["window_in_flight"], false);
    // 2026-11-01 裁决①：目标 ref 消费时探测（目标仓当前检出分支），status 无静态值
    assert_eq!(
        body["integration"]["target_ref"],
        "detected-per-repo-at-consume-time"
    );

    // 写设置文件 → status 立刻看到开关开（热生效，与消费循环同一份口径）
    let settings_path = f.state.plans_root.join("integrations").join("settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    std::fs::write(&settings_path, r#"{"auto_integrate_enabled": true}"#).unwrap();
    let resp = send(&f.router, "GET", "/api/v1/scheduler/status", None).await;
    let body = body_json(resp).await;
    assert_eq!(
        body["integration"]["auto_integrate_enabled"], true,
        "改文件即生效（每轮重读）：{body}"
    );
}

/// R5-EX-9/R2-13 + R5-P55：进程级去重集超限对折——真实路径灌 SEEN_SET_CAP+100
/// 条，mark_fused 触发裁剪后集合回到约一半（有界）；未超限不动。
#[test]
fn seen_set_cap_folds_when_over_limit() {
    // 前置隔离：本测试独占 fused_runs 进程级集合——其它测试若依赖其初始为空
    // 会受影响（fused 集合只用于熔断去重，无别的断言依赖）。
    let cap = crate::server::scheduler::SEEN_SET_CAP;
    for i in 0..(cap + 100) {
        crate::server::scheduler::mark_fused(&format!("cap-test-{i}"));
    }
    let set = crate::server::scheduler::fused_runs().lock().unwrap();
    assert!(
        set.len() <= cap,
        "超限后必须有界（对折保留）：{} > {cap}",
        set.len()
    );
    assert!(
        set.contains("cap-test-0") || set.len() > cap / 3,
        "对折应保留约一半条目：{}",
        set.len()
    );
}

/// R4-P39：自动入队拒脏 frozen sha（非 40 位 hex）与缺 base_rev 契约——
/// 脏值不再静默入队后被归因为「候选自身缺陷」。
#[tokio::test]
async fn auto_enqueue_rejects_dirty_frozen_and_missing_base_rev() {
    let f = fixture();
    let plans_root = f.state.plans_root.clone();
    // ① 脏 frozen sha：公共前置后覆写为非 40 位 hex
    settled_contract_task(&f, "dirty-sha", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    std::fs::write(
        plans_root
            .join("devforge")
            .join("dirty-sha")
            .join("frozen-commit.txt"),
        "not-a-valid-sha\n",
    )
    .unwrap();
    // ② 缺 base_rev：契约 base_rev = None
    {
        let mut store = f.state.store.lock().unwrap();
        store
            .create_task(
                &SopRegistry::builtin(),
                "devforge",
                "no-base",
                "planning",
                Some(crate::domain::contract::WorkItemContract {
                    owned_paths: vec!["src/**".into()],
                    forbidden_paths: vec![],
                    base_rev: None,
                    isolation: crate::domain::contract::Isolation::None,
                }),
                TaskDeps::default(),
            )
            .unwrap();
        let run = store.begin_run("devforge", "no-base").unwrap();
        store.finish_run(&run.id, FinishOutcome::Completed).unwrap();
    }
    std::fs::create_dir_all(plans_root.join("devforge").join("no-base")).unwrap();
    std::fs::write(
        plans_root
            .join("devforge")
            .join("no-base")
            .join("frozen-commit.txt"),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
    )
    .unwrap();

    let report = tick(&f.state, &SchedulerConfig::default());
    assert!(
        !report
            .auto_enqueued
            .iter()
            .any(|e| e.contains("dirty-sha") || e.contains("no-base")),
        "脏 sha 与缺 base_rev 都不得入队：{report:?}"
    );
    let queue = crate::integration::MergeQueue::load(&plans_root, "devforge").unwrap();
    assert!(
        !queue
            .candidates
            .iter()
            .any(|c| c.task_id == "dirty-sha" || c.task_id == "no-base"),
        "两个候选都不得出现在队列"
    );
}
