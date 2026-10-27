//! 服务器端点单测（内存态 oneshot）——ARC-R6 自单文件拆分：start 渲染闸门 / crud / retry / observe / http 六主题 mod。

//! 服务器端点单测（内存态，oneshot，不碰真 pi/网络）。

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::config::ConfigManifest;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::task::Task;
use crate::domain::run::Run;
use crate::error::Result;
use crate::server::{build_router, ServerState, UiRelay};
use tokio_stream::StreamExt as _;

use std::sync::{Arc, Mutex};

/// 测试引擎：立刻完成并记录调用；hang_ms>0 时模拟长 run（响应取消/超时干预）；
/// fail_system_times>0 时先抛 N 次基础设施故障（验证自动重启）；quality_fail 时直接抛质量失败；
/// panic_times>0 时先真 panic N 次（验证引擎 panic 兜底，全仓审计 #1）。
struct MockEngine {
    calls: Arc<Mutex<Vec<String>>>,
    hang_ms: u64,
    fail_system_times: Arc<Mutex<u32>>,
    quality_fail: bool,
    panic_times: Arc<Mutex<u32>>,
}
impl RunEngine for MockEngine {
    fn execute(&mut self, task: &Task, _run: &Run, control: &RunControl) -> Result<RunOutcome> {
        {
            // 先释放 calls 锁再 panic：否则锁被毒化，后续 run 也会连带 PoisonError
            let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
            calls.push(format!("{}/{}", task.project, task.id));
        }
        let should_panic = {
            let mut left = self.panic_times.lock().unwrap_or_else(|e| e.into_inner());
            if *left > 0 {
                *left -= 1;
                true
            } else {
                false
            }
        };
        if should_panic {
            panic!("模拟引擎 panic（测试注入）");
        }
        if self.quality_fail {
            return Err(crate::error::DevforgeError::Quality(
                "终审未通过（route=finish）：文档与代码不一致".into(),
            ));
        }
        {
            let mut left = self.fail_system_times.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return Err(crate::error::DevforgeError::System(
                    "模拟上游 500（测试注入）".into(),
                ));
            }
        }
        if self.hang_ms > 0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(self.hang_ms);
            while std::time::Instant::now() < deadline {
                if control.is_cancelled() {
                    return Ok(RunOutcome::Failed {
                        reason: "已取消（cancel 端点）".into(),
                    });
                }
                if control.timed_out(crate::domain::now_ms()) {
                    return Ok(RunOutcome::Failed {
                        reason: "已超时（run 超时，被 watchdog 终止）".into(),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        Ok(RunOutcome::Completed)
    }
}

struct Fixture {
    router: axum::Router,
    state: ServerStateHandle,
    calls: Arc<Mutex<Vec<String>>>,
    _dir: tempfile::TempDir,
}
type ServerStateHandle = Arc<ServerState>;

fn fixture() -> Fixture {
    fixture_with_token(None)
}

fn fixture_with_token(token: Option<&str>) -> Fixture {
    fixture_full(0, None, token, 0, false, 0)
}

/// 可注入长 run / 超时的夹具变体（取消与超时单测用）。
fn fixture_with_engine(hang_ms: u64, timeout_ms: Option<u64>, token: Option<&str>) -> Fixture {
    fixture_full(hang_ms, timeout_ms, token, 0, false, 0)
}

/// 可注入基础设施故障次数的夹具变体（续跑/自动重启单测用）。
fn fixture_with_failures(fail_system_times: u32) -> Fixture {
    fixture_full(0, None, None, fail_system_times, false, 0)
}

/// 质量失败（审查打回/终审不通过）的夹具：不得被自动重启。
fn fixture_with_quality_failure() -> Fixture {
    fixture_full(0, None, None, 0, true, 0)
}

/// 引擎 panic 的夹具（全仓审计 #1：panic 兜底后不得卡死串行槽位）。
fn fixture_with_panic(panic_times: u32) -> Fixture {
    fixture_full(0, None, None, 0, false, panic_times)
}

fn fixture_full(
    hang_ms: u64,
    timeout_ms: Option<u64>,
    token: Option<&str>,
    fail_system_times: u32,
    quality_fail: bool,
    panic_times: u32,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    // 观测根注入到该夹具的临时目录（Q4 路径 ①）：走真实 start 路径的单测把心跳写在这里，
    // 用户 HOME 的 `.devforge/heartbeat/` 不被写脏。
    // 注入到**本实例**（不是进程级全局）→ 与其它并行单测不抢 root（R13 的落点漂移）。
    let obs_root = dir.path().join("observability");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let failures = Arc::new(Mutex::new(fail_system_times));
    let panics = Arc::new(Mutex::new(panic_times));
    let engine_factory = Box::new(move || -> Box<dyn RunEngine + Send> {
        Box::new(MockEngine {
            calls: calls2.clone(),
            hang_ms,
            fail_system_times: failures.clone(),
            quality_fail,
            panic_times: panics.clone(),
        })
    });
    let bus = std::sync::Arc::new(crate::server::EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut state = ServerState::new(
        Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        engine_factory,
        bus,
        ui,
        std::sync::Arc::new(crate::config::installer::FakeInstaller),
        dir.path().join("profiles"),
        dir.path().join("plans"),
        dir.path().join("sessions"),
    );
    // 单测不真睡：重试退避归零
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .retry_policy = crate::engine::policy::RetryPolicy::fast();
    // 观测根钉在本夹具的临时目录（路由构建前注入，Arc 共享后不可再改）
    Arc::get_mut(&mut state)
        .expect("路由未构建前应独占")
        .obs_root = Some(obs_root);
    // token 在路由构建前注入（Arc 共享后不可再改）
    if let Some(t) = token {
        Arc::get_mut(&mut state).expect("路由未构建前应独占").token = Some(t.into());
    }
    if let Some(t) = timeout_ms {
        Arc::get_mut(&mut state).expect("路由未构建前应独占").run_timeout_ms = Some(t);
    }
    Fixture {
        router: build_router(state.clone()),
        state,
        calls,
        _dir: dir,
    }
}

async fn wait_settled(f: &Fixture, project: &str, task: &str) {
    for _ in 0..200 {
        let resp = send(
            &f.router,
            "GET",
            &format!("/api/v1/projects/{project}/tasks/{task}/runs"),
            None,
        )
        .await;
        let body = body_json(resp).await;
        // 等**最新**那个 run 进终态：列表按插入序（`store.list_runs`），最后一条才是刚起的那个。
        // 只看第一条会在「同一任务起第二次 run」时误判已终态 → 下一次 start 撞 409（串行槽）。
        let st = body["runs"]
            .as_array()
            .and_then(|runs| runs.last())
            .and_then(|r| r["status"].as_str())
            .unwrap_or("");
        if st == "Settled" || st == "Failed" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("run 未在上限内进入终态：{project}/{task}");
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let builder = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    router.clone().oneshot(req).await.unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("响应应为 JSON")
}


mod contract;
mod crud;
mod hardening;
mod http;
mod integration_api;
mod observe;
mod panic_safety;
mod restart;
mod retry;
mod scheduler;
mod start;
mod boundary_entry;
