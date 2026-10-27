//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{change::*, fixture::*, guards::*, milestone::*, panic::*, support::*};

#[test]
pub(super) fn infra_failure_is_logged_as_error_with_the_full_text_and_the_source_chain() {
    // 错实现形态：①非质量类失败落 WARN（应 ERROR）②缺 error_chain（source 链看不见）
    // ③缺 attempt / component / event → 本用例必须变红
    let cap = Capture::new();
    let marker = "f6-infra-marker-1";
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            // 带上下文的包装错误：source 链有两层（上下文层 + 内层错误）——
            // 引擎审计修复 #3：分类看根因不看包装，根因 System → 照常重启耗尽
            let inner: Result<()> = Err(DevforgeError::System(format!("上游 500（{marker}）")));
            Err(inner.ctx("拉模型响应").unwrap_err())
        });
        assert_eq!(attempts, 4, "根因 System（剥包装后）应进重启分支直到耗尽");
        assert!(result.is_err());
    }
    let text = cap.lines();
    // 只认本用例 marker：Capture 的锁只串行「用了 Capture 的用例」，同二进制的其他用例
    // 也往同一个全局日志缓冲写 run.attempt_failed，按事件名计数会偶发多算
    //（2026-10-25 全量并行 4 跑红 1 的根因）。
    let lines: Vec<&str> = lines_with(&text, "run.attempt_failed")
        .into_iter()
        .filter(|l| l.contains(marker))
        .collect();
    assert_eq!(lines.len(), 4, "每次尝试一条：{text:?}");
    let first = lines[0];
    assert!(
        first.contains("\"level\":\"ERROR\""),
        "非质量类失败落 ERROR：{first}"
    );
    assert!(
        first.contains("\"component\":\"run-retry\""),
        "字段集要含 component：{first}"
    );
    assert!(
        first.contains("\"event\":\"run.attempt_failed\""),
        "字段集要含 event：{first}"
    );
    assert!(first.contains("\"attempt\":1"), "要带 attempt：{first}");
    // 原始错误全文：顶层 Display 逐字 + source 链每一层
    assert!(first.contains(marker), "缺原始错误全文：{first}");
    assert!(first.contains("拉模型响应"), "缺上下文层：{first}");
    assert!(first.contains("→"), "source 链要用 → 连接：{first}");
    assert!(first.contains("error_chain"), "缺 source 链字段：{first}");
    assert!(
        first.contains("\"retryable\":true"),
        // 引擎审计修复 #3：根因 System（剥包装后）可重启——retryable 翻 true
        "根因 System 应标记可重启：{first}"
    );
}

#[test]
pub(super) fn quality_failure_is_logged_as_warn_with_the_same_fields_and_is_not_retried() {
    // 错实现形态：①质量失败落 ERROR（应 WARN）②字段与基础类不同构
    // ③质量失败被当成基础设施故障重启 → 本用例必须变红
    let cap = Capture::new();
    let marker = "f6-quality-marker-1";
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            Err(DevforgeError::Quality(format!(
                "审查打回：缺验收标准（{marker}）"
            )))
        });
        assert_eq!(attempts, 1, "质量失败不得重启");
        assert!(result.is_err());
    }
    let text = cap.lines();
    // 同上：只认本用例 marker，避免并行下把别的用例的条目算进来
    let lines: Vec<&str> = lines_with(&text, "run.attempt_failed")
        .into_iter()
        .filter(|l| l.contains(marker))
        .collect();
    assert_eq!(lines.len(), 1, "质量失败只落一条：{text:?}");
    let line = lines[0];
    assert!(line.contains("\"level\":\"WARN\""), "质量失败落 WARN：{line}");
    assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
    assert!(line.contains("\"component\":\"run-retry\""), "{line}");
    assert!(line.contains("\"attempt\":1"), "{line}");
    assert!(line.contains("\"retryable\":false"), "质量失败不重启：{line}");
    assert!(line.contains(marker), "缺原始错误全文：{line}");
    assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
    assert!(
        !line.contains("\"level\":\"ERROR\""),
        "质量失败不得占 ERROR 级别：{line}"
    );
}

#[test]
pub(super) fn four_infra_failures_log_four_attempt_entries_and_one_exhausted_entry() {
    // 错实现形态：①只落一条（把四次尝试合成一条）②缺 run.attempt_exhausted
    // ③终态条目不带总尝试次数 → 本用例必须变红
    let cap = Capture::new();
    let marker = "f6-infra-marker-4";
    {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
            Err(DevforgeError::System(format!("第 {n} 次挂了（{marker}）")))
        });
        assert_eq!(attempts, 4, "首次 + 3 次重试");
        assert!(result.is_err());
    }
    let text = cap.lines();
    // 同上：只认本用例 marker
    let attempts: Vec<&str> = lines_with(&text, "run.attempt_failed")
        .into_iter()
        .filter(|l| l.contains(marker))
        .collect();
    assert_eq!(attempts.len(), 4, "4 次尝试各一条：{text:?}");
    for (i, line) in attempts.iter().enumerate() {
        assert!(line.contains(&format!("\"attempt\":{}", i + 1)), "{line}");
        assert!(line.contains("\"level\":\"ERROR\""), "基础设施故障落 ERROR：{line}");
    }
    let exhausted: Vec<&str> = lines_with(&text, "run.attempt_exhausted")
        .into_iter()
        .filter(|l| l.contains(marker))
        .collect();
    assert_eq!(exhausted.len(), 1, "应有且仅有一条终态条目：{text:?}");
    assert!(
        exhausted[0].contains("\"attempts\":4"),
        "终态条目要带总尝试次数：{}",
        exhausted[0]
    );
    assert!(
        exhausted[0].contains("\"level\":\"ERROR\""),
        "终态条目与尝试级同级：{}",
        exhausted[0]
    );
}

#[tokio::test]
pub(super) async fn the_error_entry_is_logged_before_the_restart_warning() {
    // 错实现形态：先公告「自动重启」再记错误（或错误只在终态落）→ 现场只剩「重启了」，
    // 看不出「为什么重启」→ 本用例必须变红
    let cap = Capture::new();
    let f = server_fixture(0, 1);
    make_task(&f, "p", "t", "planning").await;
    let engine: Box<dyn RunEngine + Send> = Box::new(MockEngine {
        calls: Arc::new(Mutex::new(Vec::new())),
        hang_ms: 0,
        fail_system_times: Arc::new(Mutex::new(1)),
    });
    let run = {
        let mut store = f.state.store.lock().expect("store 锁");
        store.begin_run("p", "t").expect("准入")
    };
    // 自审 minor（223）后：干预面由调用方先登记，run_in_background 只收已可见的 control。
    let control = f.state.register_control("p", "t");
    devforge::server::run_in_background(
        f.state.clone(),
        "p".into(),
        "t".into(),
        engine,
        run,
        control,
    );
    let text = cap.lines();
    let err_at = text
        .iter()
        .position(|l| l.contains("run.attempt_failed") && l.contains("\"attempt\":1"))
        .unwrap_or_else(|| panic!("第 1 次失败应落一条尝试级条目：{text:?}"));
    let restart_at = text
        .iter()
        .position(|l| l.contains("自动重启 run"))
        .unwrap_or_else(|| panic!("基础设施故障应触发自动重启：{text:?}"));
    assert!(
        err_at < restart_at,
        "错误条目必须先于「自动重启 run」落盘（先记错误、再决定重启）：{text:?}"
    );
    assert!(
        text[err_at].contains("\"level\":\"ERROR\""),
        "非质量类失败落 ERROR：{}",
        text[err_at]
    );
    assert!(
        text[restart_at].contains("\"attempt\":2"),
        "重启的是第 2 次尝试：{}",
        text[restart_at]
    );
}

// ---- R-1：attempt 的写入时机（每次尝试**开始时**写进 run 上下文）----

#[test]
pub(super) fn the_run_context_carries_the_current_attempt_during_a_later_attempt() {
    // 错实现形态：attempt 只在**失败分支**里写（第 1 次失败之后才写 1）→ 第 2 次尝试期间
    //   上下文仍是上一次的值 → 落点②（`map_intervention` 的质量 WARN）读到陈旧的 attempt
    //   → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let run_id = "run-f6-attempt";
    obs::begin_run_in(dir.path().to_path_buf(), run_id, "coding");
    let mut seen: Vec<u32> = Vec::new();
    let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
        // 每次尝试**开始时**，上下文里就应当是本次尝试号
        // （重试层在 `execute(attempt)` 之前写；`map_intervention` 从同一处读）
        seen.push(obs::attempt());
        if n == 1 {
            // 基础设施故障 → 触发自动重启（第 2 次尝试）
            Err(DevforgeError::System("模拟上游 500（测试注入）".into()))
        } else {
            // 第 2 次尝试期间的质量失败（生产链路上被 `map_intervention` 收成 `Ok(Failed)`）
            Err(DevforgeError::Quality("终审未通过".into()))
        }
    });
    obs::end_run(run_id);
    assert_eq!(
        attempts, 2,
        "第 1 次基础设施故障 → 自动重启一次；第 2 次的质量失败不再重启"
    );
    assert!(result.is_err(), "质量失败以错误收场");
    assert_eq!(
        seen,
        vec![1, 2],
        "每次尝试**开始时** run 上下文就应是本次尝试号（R-1：不是上一次的值）：{seen:?}"
    );
}

// ============================================================ 根因修复：渲染闸门

#[tokio::test]
pub(super) async fn start_waits_at_the_render_gate_until_the_required_profile_is_ready() {
    // 错实现形态：闸门不存在（start 立即开跑 → 门卫判「profile 未渲染」→ 盲重启）
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let waiting = tokio::time::timeout(
        Duration::from_millis(200),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await;
    assert!(
        waiting.is_err(),
        "渲染在途且所需 profile 未就绪时，start 不得立即开跑"
    );
    // 所需 profile 就绪 → 闸门放行 → start 202
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
        // 自审 minor（115）：闸门就绪判据 = 渲染完成标记
        std::fs::write(
            d.join(devforge::config::render::RENDER_READY_MARKER),
            "fp",
        )
        .expect("写渲染完成标记");
    }
    f.state.render_gate.end_render();
    let resp = tokio::time::timeout(
        Duration::from_secs(2),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("闸门放行后 start 应当返回");
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
pub(super) async fn start_returns_immediately_when_the_required_profiles_are_ready_even_if_a_render_is_in_flight() {
    // 错实现形态：闸门等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
        // 自审 minor（115）：闸门就绪判据 = 渲染完成标记
        std::fs::write(
            d.join(devforge::config::render::RENDER_READY_MARKER),
            "fp",
        )
        .expect("写渲染完成标记");
    }
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let resp = tokio::time::timeout(
        Duration::from_millis(500),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("所需 profile 已就绪时 start 不得等整轮渲染");
    assert_eq!(resp.status(), 202);
    // 渲染仍在途（闸门没有等整轮渲染）
    assert!(f.state.render_gate.is_in_flight());
    f.state.render_gate.end_render();
}

#[tokio::test]
pub(super) async fn a_ready_run_passes_on_the_first_attempt_without_a_restart_warning() {
    // 错实现形态：profile 就绪也走成「失败 → 自动重启」（attempt=2 的重启 WARN 出现）
    // → 本用例必须变红
    let cap = Capture::new();
    let f = server_fixture(0, 0);
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
        // 自审 minor（115）：闸门就绪判据 = 渲染完成标记
        std::fs::write(
            d.join(devforge::config::render::RENDER_READY_MARKER),
            "fp",
        )
        .expect("写渲染完成标记");
    }
    make_task(&f, "p", "t1", "coding").await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t1").await;
    let log = cap.lines();
    assert!(
        lines_with(&log, "自动重启 run").is_empty(),
        "profile 就绪时第一次尝试即通过，不得出现自动重启 WARN：{log:?}"
    );
    let finished = events(&f)
        .into_iter()
        .find(|e| e.contains("\"type\":\"run.finished\""))
        .expect("应有 run.finished 事件");
    assert!(
        finished.contains("\"attempts\":1"),
        "尝试次数应为 1：{finished}"
    );
}

#[test]
pub(super) fn a_missing_profile_fails_the_run_explicitly_without_a_restart() {
    // 错实现形态：profile 缺失时错误文本不点名缺失的 profile / 路径，或被当成基础设施故障盲重启
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let mut engine = devforge::engine::builtin::BuiltinRpcEngine {
        profile_root: dir.path().join("profiles"),
        session_root: dir.path().join("sessions"),
        plans_root: dir.path().join("plans"),
        ui: Arc::new(devforge::server::UiRelay::new(Arc::new(
            devforge::observability::events::EventBus::new(),
        ))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let (result, attempts) =
        run_with_retry(&RetryPolicy::fast(), &control, |_| engine.execute(&task, &run, &control));
    assert_eq!(attempts, 1, "profile 未就绪不得触发自动重启（尝试次数恒为 1）");
    let err = result.expect_err("profile 未渲染时 run 必须失败");
    let msg = err.to_string();
    assert!(msg.contains("coding-agent"), "错误要点名缺失的 profile：{msg}");
    assert!(msg.contains("settings.json"), "错误要点名缺失的路径：{msg}");
    assert!(msg.contains("未渲染"), "错误要说明未渲染：{msg}");
}

#[test]
pub(super) fn a_missing_profile_is_a_config_error_and_is_not_an_infrastructure_failure() {
    // 错实现形态：错误种类仍是 System（会被 is_infra_failure 判成基础设施故障 → 盲重启 3 次）
    // → 本用例必须变红
    let dir = tempfile::tempdir().expect("临时目录");
    let mut engine = devforge::engine::builtin::BuiltinRpcEngine {
        profile_root: dir.path().join("profiles"),
        session_root: dir.path().join("sessions"),
        plans_root: dir.path().join("plans"),
        ui: Arc::new(devforge::server::UiRelay::new(Arc::new(
            devforge::observability::events::EventBus::new(),
        ))),
        codegraph_bin: PathBuf::from("codegraph"),
        idle_ms: None,
    };
    let task = Task::new("t", "p", "coding", 1);
    let run = Run::new("p", "t");
    let control = RunControl::new();
    let err = engine
        .execute(&task, &run, &control)
        .expect_err("profile 未渲染时必须失败");
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "profile 未就绪应是配置错误：{err:?}"
    );
    assert!(
        !devforge::engine::policy::is_infra_failure(&Err(err)),
        "配置错误不得被判为基础设施故障（否则会盲重启）"
    );
}

#[tokio::test]
pub(super) async fn other_endpoints_are_served_while_a_start_waits_at_the_render_gate() {
    // 错实现形态：闸门等待用阻塞式等待占住 tokio worker → 等待期内其它端点的请求被拖住
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let r = f.router.clone();
    let waiter = tokio::spawn(async move {
        send(&r, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let resp = tokio::time::timeout(
        Duration::from_millis(500),
        send(&f.router, "GET", "/api/v1/projects", None),
    )
    .await
    .expect("闸门等待期间其它端点必须能被及时处理");
    assert_eq!(resp.status(), 200);
    f.state.render_gate.end_render();
    let resp = tokio::time::timeout(Duration::from_secs(3), waiter)
        .await
        .expect("闸门放行后 start 应当返回")
        .unwrap();
    assert_eq!(resp.status(), 202);
}

#[tokio::test]
pub(super) async fn two_concurrent_starts_while_the_gate_waits_run_at_most_one() {
    // 错实现形态：两个并发 start 都开跑（双跑）；或输的那一路静默消失（既没开跑也没 run.error）
    // → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(1500, 0);
    make_task(&f, "p", "t1", "coding").await;
    f.state.render_gate.begin_render();
    let r1 = f.router.clone();
    let r2 = f.router.clone();
    let a = tokio::spawn(async move {
        send(&r1, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    let b = tokio::spawn(async move {
        send(&r2, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
        // 自审 minor（115）：闸门就绪判据 = 渲染完成标记
        std::fs::write(
            d.join(devforge::config::render::RENDER_READY_MARKER),
            "fp",
        )
        .expect("写渲染完成标记");
    }
    f.state.render_gate.end_render();
    // server #1 修复后：准入在响应之前——一路 202、一路 409，不存在「后台才失败」的静默路径
    let s1 = a.await.expect("第一个 start").status().as_u16();
    let s2 = b.await.expect("第二个 start").status().as_u16();
    let mut got = vec![s1, s2];
    got.sort_unstable();
    assert_eq!(
        got,
        vec![202, 409],
        "并发 start 只能有一路拿到 202（另一路当场 409），实际：{s1}/{s2}"
    );
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t1/runs", None).await,
    )
    .await;
    assert_eq!(
        body["runs"].as_array().map(|r| r.len()),
        Some(1),
        "只能有一个 run 真正开跑：{body}"
    );
    // 引擎调用发生在**后台 run 线程**里，而输的那一路的 run.error 事件先到
    // → 断言「引擎只被调用一次」前要有界等待它出现，
    // 否则机器一忙就会把「还没调到」误判成「零跑/双跑」（假红）
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let calls = loop {
        let n = f.calls.lock().expect("calls 锁").len();
        if n > 0 || std::time::Instant::now() > deadline {
            break n;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(calls, 1, "引擎只能被调用一次（至多一个 run 真正开跑）");
}

// ---- R-2：放行条件回环重判 / R-3：在途计数的归零 ----

#[tokio::test]
pub(super) async fn the_gate_does_not_release_while_another_render_is_still_in_flight() {
    // 错实现形态：放行条件是「任意一次渲染结束就放行」（`rx.changed()` 分支直接 return）
    //   → 并发两轮渲染时，先结束的那一轮就把 run 放了出去（所需 profile 还没落盘）
    //   → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    make_task(&f, "p", "t1", "coding").await;
    // 两轮渲染在途
    f.state.render_gate.begin_render();
    f.state.render_gate.begin_render();
    let r = f.router.clone();
    let waiter = tokio::spawn(async move {
        send(&r, "POST", "/api/v1/projects/p/tasks/t1/start", None).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "两轮渲染都在途时不得开跑");
    // 其中一轮结束 → **仍有渲染在途** → 仍不得放行（R-2：放行条件要回环重判）
    f.state.render_gate.end_render();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !waiter.is_finished(),
        "仍有渲染在途时不得放行（不能由「任意一次渲染结束」放行）"
    );
    // 所需 profile 全部就绪 → 放行（放行条件的另一半）
    for p in ["coding-agent", "coding-review"] {
        let d = f.state.profiles_root.join(p);
        std::fs::create_dir_all(&d).expect("建 profile 目录");
        std::fs::write(d.join("settings.json"), "{}").expect("写 settings.json");
        // 自审 minor（115）：闸门就绪判据 = 渲染完成标记
        std::fs::write(
            d.join(devforge::config::render::RENDER_READY_MARKER),
            "fp",
        )
        .expect("写渲染完成标记");
    }
    let resp = tokio::time::timeout(Duration::from_secs(3), waiter)
        .await
        .expect("所需 profile 就绪后闸门应放行")
        .unwrap();
    assert_eq!(resp.status(), 202);
    f.state.render_gate.end_render();
}

#[tokio::test]
pub(super) async fn the_render_gate_count_goes_back_to_zero_after_the_render_thread_ends() {
    // 错实现形态：渲染结束后在途计数不归零（归零那一行被删 / drop 守卫没接上）
    //   → 之后的 start 永远在闸门上等 → 本用例必须变红
    let _cap = Capture::new();
    let f = server_fixture(0, 0);
    // 走公开面：PUT 清单 → 渲染在后台线程跑（立即 202）
    let manifest = body_json(send(&f.router, "GET", "/api/v1/config/manifest", None).await).await;
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/config/manifest",
        Some(manifest),
    )
    .await;
    assert_eq!(resp.status(), 202, "PUT 清单立即 202（渲染在后台）");
    // 渲染线程结束后，在途计数必须归零
    // （R-3：归零由 drop 守卫保证，不依赖线程体末尾的显式调用）
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while f.state.render_gate.is_in_flight() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !f.state.render_gate.is_in_flight(),
        "渲染结束后闸门的在途计数必须归零（R-3）"
    );
    // 归零的后果：之后的 start 不会无限等待
    make_task(&f, "p", "t1", "coding").await;
    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        send(&f.router, "POST", "/api/v1/projects/p/tasks/t1/start", None),
    )
    .await
    .expect("渲染结束后 start 不得无限等待（计数已归零）");
    assert_eq!(resp.status(), 202);
}

// ============================================================ 需求 2：里程碑

/// change 编排层的脚本化 session（plan 段打回一次 → 过；testing 段回注 coding）。
pub(super) struct ScriptedChangeSession {
    pub(super) plan_calls: usize,
}

impl ChangeSession for ScriptedChangeSession {
    fn run_plan(&mut self, _ui: &mut dyn UserLink, _roundtrip: usize) -> Result<PlanTurn> {
        self.plan_calls += 1;
        if self.plan_calls == 1 {
            Ok(PlanTurn::Fail {
                issues: vec!["影响面漏了调用方".into()],
            })
        } else {
            Ok(PlanTurn::Pass)
        }
    }
    fn amend_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _gaps: &[devforge::change::boundary::ImpactGap],
        _roundtrip: usize,
        _reason: &str,
    ) -> Result<PlanTurn> {
        Ok(PlanTurn::Pass)
    }
    fn fix_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _issues: &[String],
        _round: usize,
    ) -> Result<PlanTurn> {
        Ok(PlanTurn::Pass)
    }
    fn run_coding(
        &mut self,
        _ui: &mut dyn UserLink,
        _roundtrip: usize,
        _last_failure: Option<&devforge::change::state::TestFailureContext>,
    ) -> Result<CodingTurn> {
        Ok(CodingTurn {
            reply: "改完了\n<stage-complete>1</stage-complete>".into(),
            changed: vec![],
            reviews: 1,
            review_issues: Vec::new(),
        })
    }
    fn run_testing(&mut self, _ui: &mut dyn UserLink) -> Result<TestingTurn> {
        Ok(TestingTurn::RouteCoding {
            failures: vec!["tests/a_test.rs:10 断言失败".into()],
        })
    }
}


