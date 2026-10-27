//! （2026-10-26 测试分离：自 src/domain/store.rs 内联平移，纯移动零改动。）
use super::*;
use crate::domain::sop::PLANNING_SOP;

fn setup() -> Store {
    let mut s = Store::new();
    s.create_project(Project::new(
        "devforge",
        std::path::PathBuf::from("/home/me/devforge"),
        1,
    ))
    .unwrap();
    s.create_task(&SopRegistry::builtin(), "devforge", "mvp", PLANNING_SOP.id, None, Default::default())
        .unwrap();
    s
}

#[test]
fn duplicate_project_rejected() {
    let mut s = setup();
    let err = s
        .create_project(Project::new("devforge", std::path::PathBuf::from("/x"), 2))
        .unwrap_err();
    assert!(err.to_string().contains("已存在"));
}

#[test]
fn create_task_validates_project_and_sop() {
    let mut s = Store::new();
    s.create_project(Project::new("p", std::path::PathBuf::from("/p"), 1))
        .unwrap();
    let err = s
        .create_task(&SopRegistry::builtin(), "ghost", "t1", "planning", None, Default::default())
        .unwrap_err();
    assert!(err.to_string().contains("不存在"));

    // P005：coding 已入注册表（不再是未知 SOP），且任务快照工作区
    let t = s
        .create_task(&SopRegistry::builtin(), "p", "t1", "coding", None, Default::default())
        .unwrap();
    assert_eq!(t.sop, "coding");
    assert_eq!(t.workspace, std::path::PathBuf::from("/p"), "快照项目 local_path");

    let err = s
        .create_task(&SopRegistry::builtin(), "p", "t2", "ghost", None, Default::default())
        .unwrap_err();
    assert!(err.to_string().contains("未知 SOP"));

    // P005 跨 SOP：set_task_sop 校验注册 + 状态可发起
    s.set_task_sop(&SopRegistry::builtin(), "p", "t1", "planning").unwrap();
    assert_eq!(s.get_task("p", "t1").unwrap().sop, "planning");
    let err = s
        .set_task_sop(&SopRegistry::builtin(), "p", "t1", "ghost")
        .unwrap_err();
    assert!(err.to_string().contains("未知 SOP"));
}

#[test]
fn system_level_serial_guard() {
    let mut s = setup();
    let r1 = s.begin_run("devforge", "mvp").unwrap();
    // 全局已有一个 Running run → 拒绝第二个（decisions/003 上限化：默认 max=1 兼容串行）
    let err = s.begin_run("devforge", "mvp").unwrap_err();
    assert!(err.to_string().contains("并发上限"), "{err}");
    s.finish_run(&r1.id, FinishOutcome::Completed).unwrap();
    assert!(!s.has_running_run());
}

#[test]
fn task_status_machine_guards() {
    let mut s = setup();
    s.begin_run("devforge", "mvp").unwrap();
    // 任务 Running 中不可归档重入——状态机守卫体现在 finish 后才能再跑
    let task = s.get_task("devforge", "mvp").unwrap();
    assert_eq!(task.status, TaskStatus::Running);
}

#[test]
fn rerun_keeps_history_and_updates_current() {
    let mut s = setup();
    // run1: 失败
    let r1 = s.begin_run("devforge", "mvp").unwrap();
    s.finish_run(&r1.id, FinishOutcome::Failed { reason: "引擎炸了".into() })
        .unwrap();
    assert_eq!(s.get_task("devforge", "mvp").unwrap().status, TaskStatus::Failed);
    // run2: 重跑成功
    let r2 = s.begin_run("devforge", "mvp").unwrap();
    s.finish_run(&r2.id, FinishOutcome::Completed).unwrap();

    let task = s.get_task("devforge", "mvp").unwrap();
    assert_eq!(task.runs.len(), 2, "重跑应留档全部 run");
    assert_eq!(task.current_run, Some(r2.id.clone()), "current 指向最新");
    // 旧 run 留档且终态不变
    assert_eq!(s.get_run(&r1.id).unwrap().status, RunStatus::Failed);
    assert_eq!(s.get_run(&r2.id).unwrap().status, RunStatus::Settled);
    assert_eq!(task.status, TaskStatus::Completed);
}

#[test]
fn json_roundtrip() {
    let mut s = setup();
    s.begin_run("devforge", "mvp").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.json");
    s.save(&path).unwrap();
    let loaded = Store::load(&path).unwrap();
    assert_eq!(s, loaded);
}

// ---------- 变更 DAG（P012 T4.1） ----------

use crate::domain::task::TaskDeps;

#[test]
fn dag_rejects_missing_dependency() {
    let mut s = setup();
    let err = s
        .create_task(
            &SopRegistry::builtin(),
            "devforge",
            "c",
            "planning",
            None,
            TaskDeps { depends_on: vec!["ghost".into()], exclusive: false },
        )
        .unwrap_err();
    assert!(format!("{err}").contains("不存在"), "应报依赖不存在：{err}");
}

#[test]
fn dag_rejects_self_dependency() {
    let mut s = setup();
    let err = s
        .create_task(
            &SopRegistry::builtin(),
            "devforge",
            "mvp2",
            "planning",
            None,
            TaskDeps { depends_on: vec!["mvp2".into()], exclusive: false },
        )
        .unwrap_err();
    assert!(format!("{err}").contains("自身"), "应报自依赖：{err}");
}

#[test]
fn dag_chain_creates_with_edges() {
    let mut s = setup();
    // a（无依赖）→ b 依赖 a → c 依赖 b：链式落库 + 字段回读
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "dag-a",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "dag-b",
        "planning",
        None,
        TaskDeps { depends_on: vec!["dag-a".into()], exclusive: true },
    )
    .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "dag-c",
        "planning",
        None,
        TaskDeps { depends_on: vec!["dag-b".into()], exclusive: false },
    )
    .unwrap();
    let b = s.get_task("devforge", "dag-b").unwrap();
    assert_eq!(b.depends_on, vec!["dag-a".to_string()]);
    assert!(b.exclusive, "独占标记应落库");
    let c = s.get_task("devforge", "dag-c").unwrap();
    assert_eq!(c.depends_on, vec!["dag-b".to_string()]);
}

#[test]
fn persist_roundtrip_survives_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let mut s = Store::new().with_persist(path.clone());
    s.create_project(Project::new("p", std::path::PathBuf::from("/x"), 1))
        .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "p",
        "t",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    let run = s.begin_run("p", "t").unwrap();
    s.finish_run(&run.id, FinishOutcome::Failed { reason: "x".into() })
        .unwrap();
    drop(s);
    let s2 = Store::load(&path).unwrap();
    assert_eq!(s2.projects.len(), 1, "项目应持久化");
    assert_eq!(s2.tasks.len(), 1, "任务应持久化");
    assert_eq!(s2.runs.len(), 1, "run 应持久化");
    assert_eq!(s2.tasks[0].status, TaskStatus::Failed, "终态应持久化");
}

#[test]
fn zombie_running_run_released_by_startup_reconcile() {
    // 自审 critical 回归：崩溃后遗留的 Running run 占死并发槽、任务停在 Running
    // ——启动对账后必须能再次 begin_run（无自愈即整机拒绝服务）。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let mut s = Store::new().with_persist(path.clone());
    s.create_project(Project::new("p", std::path::PathBuf::from("/x"), 1))
        .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "p",
        "t",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    let run = s.begin_run("p", "t").unwrap();
    drop(s); // 模拟崩溃：不 finish_run 直接丢掉进程
    let mut s2 = Store::load(&path).unwrap();
    assert_eq!(s2.running_run_count(), 1, "崩溃现场：run 留在 Running");
    assert_eq!(s2.reconcile_zombie_runs(), 2, "run + 任务各对账一条");
    assert_eq!(s2.running_run_count(), 0, "对账后并发槽应释放");
    let again = s2.begin_run("p", "t").expect("对账后必须能再次发起 run");
    assert_ne!(again.id, run.id, "重跑 = 新 run");
    // 对账必须落盘：重开一份不再看到僵尸
    let s3 = Store::load(&path).unwrap();
    assert_eq!(s3.running_run_count(), 1, "只有新 run 在跑");
}

/// 自审 major 回归：project/task 名是目录名，不安全名在建入 store 时就被拒。
#[test]
fn unsafe_project_or_task_names_are_rejected() {
    let mut s = Store::new();
    let p = Project::new("../evil", std::path::PathBuf::from("/x"), 1);
    let err = s.create_project(p).unwrap_err();
    assert!(
        err.to_string().contains("非法路径字符"),
        "穿越项目名应拒：{err}"
    );
    s.create_project(Project::new("p", std::path::PathBuf::from("/x"), 1))
        .unwrap();
    for bad in ["a/b", "..", ".", ""] {
        let err = s
            .create_task(
                &SopRegistry::builtin(),
                "p",
                bad,
                "planning",
                None,
                TaskDeps::default(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("非法路径字符"), "{bad} 应拒：{err}");
    }
}

/// 自审 major 回归：DAG 契约在领域层强制——依赖未完成时 begin_run 必须拒，
/// 不再依赖调度器自觉；依赖完成后放行。
#[test]
fn begin_run_requires_dependencies_completed() {
    let mut s = setup();
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "dep",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "worker",
        "planning",
        None,
        TaskDeps {
            depends_on: vec!["dep".into()],
            exclusive: false,
        },
    )
    .unwrap();
    let err = s.begin_run("devforge", "worker").unwrap_err();
    assert!(
        err.to_string().contains("未就绪"),
        "依赖未完成必须拒：{err}"
    );
    // 依赖跑完（Completed）→ 放行
    let dep_run = s.begin_run("devforge", "dep").unwrap();
    s.finish_run(&dep_run.id, FinishOutcome::Completed).unwrap();
    s.begin_run("devforge", "worker").expect("依赖完成后应放行");
}

/// 自审 major 回归：失败原因必须落进 run 记录（重启后可复盘）。
#[test]
fn finish_run_persists_the_failure_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let mut s = Store::new().with_persist(path.clone());
    s.create_project(Project::new("p", std::path::PathBuf::from("/x"), 1))
        .unwrap();
    s.create_task(
        &SopRegistry::builtin(),
        "p",
        "t",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    let run = s.begin_run("p", "t").unwrap();
    s.finish_run(
        &run.id,
        FinishOutcome::Failed {
            reason: "终审未通过".into(),
        },
    )
    .unwrap();
    let s2 = Store::load(&path).unwrap();
    assert_eq!(
        s2.get_run(&run.id).unwrap().reason.as_deref(),
        Some("终审未通过"),
        "失败原因必须落库"
    );
}

/// 自审 major 回归：任务状态机守卫——Running/Archived 拒，Pending/Failed 放。
#[test]
fn begin_run_state_machine_guard_is_enforced() {
    let mut s = setup();
    s.max_concurrent_runs = 5; // 绕开并发上限，让状态机守卫当拦路人
    s.create_task(
        &SopRegistry::builtin(),
        "devforge",
        "t",
        "planning",
        None,
        TaskDeps::default(),
    )
    .unwrap();
    let run = s.begin_run("devforge", "t").unwrap();
    let err = s.begin_run("devforge", "t").unwrap_err();
    assert!(err.to_string().contains("不可发起"), "Running 必须拒：{err}");
    s.finish_run(&run.id, FinishOutcome::Failed { reason: "x".into() })
        .unwrap();
    s.begin_run("devforge", "t").expect("Failed 可重跑");
}

#[test]
fn tmp_paths_are_rejected_by_guard() {
    use std::path::Path;
    assert!(Store::tmp_path_violation(Path::new("/tmp/devforge")).is_some());
    assert!(Store::tmp_path_violation(Path::new("/tmp")).is_some());
    assert!(Store::tmp_path_violation(Path::new("/var/tmp/devforge-accept")).is_some());
    assert!(Store::tmp_path_violation(Path::new("/dev/shm/x")).is_some());
    assert!(Store::tmp_path_violation(Path::new("/home/u/devforge")).is_none());
    assert!(Store::tmp_path_violation(Path::new("/var/lib/devforge")).is_none());
}

// ── 2026-10-27 自审 store 批回归 ──

/// 依赖未完成的直接 start（绕过调度器）也必须被领域层拒绝。
#[test]
fn begin_run_rejects_unfinished_dependencies() {
    let mut s = Store::new();
    s.create_project(Project::new("p", std::path::PathBuf::from("/home/me/p"), 1))
        .unwrap();
    let reg = SopRegistry::builtin();
    s.create_task(&reg, "p", "a", PLANNING_SOP.id, None, TaskDeps::default())
        .unwrap();
    s.create_task(
        &reg,
        "p",
        "b",
        PLANNING_SOP.id,
        None,
        TaskDeps { depends_on: vec!["a".into()], exclusive: false },
    )
    .unwrap();
    let err = s.begin_run("p", "b").unwrap_err();
    assert!(err.to_string().contains("未就绪"), "{err}");
}

/// 自审 minor（118）：并发上限 0 不得等于「永久拒绝」（调度器侧有 .max(1)，两端口径统一）。
#[test]
fn zero_max_concurrent_behaves_as_one() {
    let mut s = Store::new();
    s.max_concurrent_runs = 0;
    s.create_project(Project::new("p", std::path::PathBuf::from("/home/me/p"), 1))
        .unwrap();
    let reg = SopRegistry::builtin();
    s.create_task(&reg, "p", "t", PLANNING_SOP.id, None, TaskDeps::default())
        .unwrap();
    assert!(s.begin_run("p", "t").is_ok(), "上限 0 应按 1 处理");
}

/// 自审 minor（115）：删项目必须级联任务与 run（否则孤儿任务仍占并发槽、仍在列表里）。
#[test]
fn remove_project_cascades_tasks_and_runs() {
    let mut s = Store::new();
    s.create_project(Project::new("p", std::path::PathBuf::from("/home/me/p"), 1))
        .unwrap();
    let reg = SopRegistry::builtin();
    s.create_task(&reg, "p", "t", PLANNING_SOP.id, None, TaskDeps::default())
        .unwrap();
    let _run = s.begin_run("p", "t").unwrap();
    assert_eq!(s.running_run_count(), 1);
    s.remove_project("p");
    assert!(s.list_projects().is_empty());
    assert!(s.list_tasks("p").is_empty(), "任务必须级联删除");
    assert!(s.list_runs().is_empty(), "run 必须级联删除");
    assert_eq!(s.running_run_count(), 0, "不得留下占并发槽的孤儿 run");
}

/// 自审 minor（119）：相对路径无法证明落点，一律按临时路径判违规；
/// `..` 绕行与符号链接也要能识破（存在时走 canonicalize）。
#[test]
fn tmp_path_violation_cannot_be_bypassed() {
    assert!(Store::tmp_path_violation(std::path::Path::new("/tmp/x")).is_some());
    assert!(Store::tmp_path_violation(std::path::Path::new("/var/tmp/y/z")).is_some());
    assert!(
        Store::tmp_path_violation(std::path::Path::new("relative/store.json")).is_some(),
        "相对路径不得放行"
    );
    assert!(Store::tmp_path_violation(std::path::Path::new("/home/u/.devforge/store.json")).is_none());
    // 经符号链接指到 /tmp：canonicalize 之后必须识破
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        assert!(
            Store::tmp_path_violation(&link.join("escape.json")).is_some(),
            "符号链接指向 /tmp 必须判违规"
        );
    }
}
