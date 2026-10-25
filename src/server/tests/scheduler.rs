//! 调度器单测（P012 M4）：就绪派工 / 依赖失败阻塞 / 容量拒绝 / 预算熔断 / 自动入队 / 独占窗口。

use super::{body_json, fixture, send};
use crate::domain::store::FinishOutcome;
use crate::domain::task::{TaskDeps, TaskStatus};
use crate::domain::{now_ms, SopRegistry};
use crate::server::scheduler::{tick, SchedulerConfig};

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
        report.dispatched.contains(&"devforge/dag-b".to_string()),
        "b 应就绪派工：{report:?}"
    );
    assert!(
        !report.dispatched.contains(&"devforge/dag-a".to_string()),
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
        report.dispatched.contains(&"devforge/queued".to_string()),
        "容量释放后 queued 应恢复派工：{report:?}"
    );
}

#[tokio::test]
async fn over_budget_run_is_fused() {
    let f = fixture();
    make_task(&f, "devforge", "long-runner", TaskDeps::default());
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
    // cancel 置位后 control 应存在且已取消
    let control = f
        .state
        .runs_control
        .lock()
        .unwrap()
        .get("devforge/long-runner")
        .cloned();
    if let Some(c) = control {
        assert!(c.is_cancelled(), "熔断应置位取消");
    }
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
    std::fs::write(frozen_dir.join("frozen-commit.txt"), "cafebabe\n").unwrap();

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
    assert_eq!(cand.frozen_sha, "cafebabe");
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
        !report.dispatched.iter().any(|t| t != "devforge/solo"),
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
        .filter(|t| t.contains("ex-"))
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
        !report.dispatched.iter().any(|t| t == "devforge/waiting"),
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
