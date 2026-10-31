//! A5 统计聚合端点单测：构造 store 验分桶 / 成功率 / 时长均值 / 任务态分布。
//!
//! 测试直接向 store.runs/tasks 推记录（pub 字段，内存态），构造确定性日期。

#[allow(unused_imports)]
use super::*;

use crate::domain::run::{Run, RunId, RunStatus};
use crate::domain::task::TaskStatus;

/// 固定「今天」：now_ms 实际值取整到当日 UTC 13:00（桶内锚点，避开边界抖动）。
fn today_anchor() -> u64 {
    let now = crate::domain::now_ms();
    let day_start = now - (now % 86_400_000);
    day_start + 13 * 3_600_000
}

fn run_with(status: RunStatus, started: u64, ended: Option<u64>) -> Run {
    Run {
        id: RunId::generate(),
        task_project: "p".into(),
        task_id: "t".into(),
        status,
        started_at_ms: started,
        ended_at_ms: ended,
        reason: None,
    }
}

#[tokio::test]
async fn stats_buckets_daily_success_rate_and_distribution() {
    let f = fixture();
    let anchor = today_anchor();
    {
        let mut store = f.state.store.lock().unwrap();
        // 今天：2 settled（3min/5min）+ 1 failed → 成功率 2/3，均值 4min
        let mut s1 = run_with(RunStatus::Settled, anchor - 180_000, Some(anchor));
        s1.id = RunId::generate();
        let mut f1 = run_with(RunStatus::Failed, anchor - 60_000, Some(anchor - 30_000));
        f1.id = RunId::generate();
        let mut s2 = run_with(RunStatus::Settled, anchor - 300_000, Some(anchor));
        s2.id = RunId::generate();
        // 昨天：1 settled
        let mut s3 = run_with(RunStatus::Settled, anchor - 86_400_000 - 60_000, Some(anchor - 86_400_000));
        s3.id = RunId::generate();
        // 窗口外（20 天前 > 缺省 14 天窗）：不计入
        let mut old = run_with(RunStatus::Failed, anchor - 20 * 86_400_000, Some(anchor - 20 * 86_400_000));
        old.id = RunId::generate();
        // 无终态：不进日桶，进 totals.running
        let mut r = run_with(RunStatus::Running, anchor - 1_000, None);
        r.id = RunId::generate();
        store.runs.extend([s1, f1, s2, s3, old, r]);

        // 任务态分布：pending×2 / running×1 / completed×1 / blocked×1
        for (id, status) in [
            ("t1", TaskStatus::Pending),
            ("t2", TaskStatus::Pending),
            ("t3", TaskStatus::Running),
            ("t4", TaskStatus::Completed),
            ("t5", TaskStatus::Blocked),
        ] {
            let task_id: String = id.to_string();
            let mut t = crate::domain::task::Task::new(task_id, "p".to_string(), "planning".to_string(), anchor);
            t.status = status;
            store.tasks.push(t);
        }
    }

    let resp = send(&f.router, "GET", "/api/v1/stats", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;

    // totals：4 终态 + 1 running（窗口外 failed 计入 totals——总量口径是全时段）
    assert_eq!(body["totals"]["runs"].as_u64(), Some(6));
    assert_eq!(body["totals"]["settled"].as_u64(), Some(3));
    assert_eq!(body["totals"]["failed"].as_u64(), Some(2));
    assert_eq!(body["totals"]["running"].as_u64(), Some(1));

    // 日桶：两条（今天 + 昨天），日期有序
    let daily = body["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 2, "窗口外与无终态不进日桶：{body}");
    assert!(
        daily[0]["date"].as_str().unwrap() < daily[1]["date"].as_str().unwrap(),
        "日期升序（BTreeMap 有序）"
    );

    // 今天桶（最后一条）：3 runs，成功率 2/3，均值 (180_000+300_000)/2 = 240_000
    let today = daily.last().unwrap();
    assert_eq!(today["runs_total"].as_u64(), Some(3));
    assert_eq!(today["settled"].as_u64(), Some(2));
    assert_eq!(today["failed"].as_u64(), Some(1));
    let rate = today["success_rate"].as_f64().unwrap();
    assert!((rate - 2.0 / 3.0).abs() < 1e-9, "成功率 = settled/(settled+failed)：{rate}");
    assert_eq!(today["avg_duration_ms"].as_u64(), Some(240_000));

    // 昨天桶：1 settled，成功率 1.0
    let yesterday = &daily[0];
    assert_eq!(yesterday["settled"].as_u64(), Some(1));
    assert_eq!(yesterday["success_rate"].as_f64(), Some(1.0));

    // 任务态分布：六态键稳定 + 计数正确
    let dist = &body["task_status_distribution"];
    for key in ["pending", "running", "completed", "failed", "archived", "blocked"] {
        assert!(dist[key].is_u64(), "六态键必须全出现：{key}");
    }
    assert_eq!(dist["pending"].as_u64(), Some(2));
    assert_eq!(dist["running"].as_u64(), Some(1));
    assert_eq!(dist["completed"].as_u64(), Some(1));
    assert_eq!(dist["blocked"].as_u64(), Some(1));
    assert_eq!(dist["archived"].as_u64(), Some(0), "零也列出（图表键稳定）");

    // days 参数生效：窗口收窄到 1 天后，昨天的桶消失
    let resp = send(&f.router, "GET", "/api/v1/stats?days=1", None).await;
    let body = body_json(resp).await;
    let daily = body["daily"].as_array().unwrap();
    assert!(daily.len() <= 1, "1 天窗口只剩今天：{daily:?}");
}

/// 空仓统计：全零与 null（成功率无分母 = null，不假装 100%）。
#[tokio::test]
async fn stats_on_empty_store_is_all_zero_with_null_rate() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/stats", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["daily"].as_array().unwrap().len(), 0);
    assert_eq!(body["totals"]["runs"].as_u64(), Some(0));
    assert_eq!(body["task_status_distribution"]["pending"].as_u64(), Some(0));
}
