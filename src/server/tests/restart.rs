//! 重启语义（复验 N1）：已完成任务再 start 显式拒绝；fresh 完整重置 change 黑板。

use super::{body_json, fixture, send};
use crate::domain::store::FinishOutcome;
use crate::domain::task::TaskDeps;
use crate::domain::SopRegistry;

#[tokio::test]
async fn completed_change_task_start_without_fresh_is_rejected() {
    let f = fixture();
    {
        let mut store = f.state.store.lock().unwrap();
        store
            .create_project(crate::domain::project::Project::new(
                "p",
                std::path::PathBuf::from("/tmp/restart-fixture"),
                1,
            ))
            .unwrap();
        store
            .create_task(&SopRegistry::builtin(), "p", "t", "change", None, TaskDeps::default())
            .unwrap();
        // 跑完一整轮 run → 任务 Completed
        let run = store.begin_run("p", "t").unwrap();
        store.finish_run(&run.id, FinishOutcome::Completed).unwrap();
    }
    // 无 fresh → 409 拒绝（不再静默空转）
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 409, "change 已完成任务无 fresh 必须拒绝");
    let body = body_json(resp).await;
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("fresh:true"),
        "错误要指路 fresh：{body}"
    );
}

#[tokio::test]
async fn fresh_start_on_change_rotates_change_blackboard() {
    let f = fixture();
    {
        let mut store = f.state.store.lock().unwrap();
        store
            .create_project(crate::domain::project::Project::new(
                "p",
                std::path::PathBuf::from("/tmp/restart-fixture"),
                1,
            ))
            .unwrap();
        store
            .create_task(&SopRegistry::builtin(), "p", "t", "change", None, TaskDeps::default())
            .unwrap();
    }
    // 预置 change 黑板：current 标记 + 旧 change-id 目录（含基线文件）
    let change_root = f.state.plans_root.join("p").join("t").join("change");
    std::fs::create_dir_all(change_root.join("change-20261020-old")).unwrap();
    std::fs::write(change_root.join("current"), "change-20261020-old\n").unwrap();
    std::fs::write(
        change_root.join("change-20261020-old").join("baseline.txt"),
        "old\n",
    )
    .unwrap();

    // fresh:true → 黑板整个轮换（current 与旧号目录一并消失，冷启动重分配）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks/t/start",
        Some(serde_json::json!({"fresh": true})),
    )
    .await;
    assert_eq!(resp.status(), 202, "fresh 重跑应被接受");
    assert!(
        !change_root.join("current").exists(),
        "current 标记必须被轮换删除"
    );
    assert!(
        !change_root.join("change-20261020-old").exists(),
        "旧 change-id 目录必须被清（不得残留旧产物）"
    );
}
