//! 待答清单端点 + 久等告警（change-20261029-ui-transcript-agent-finding-f4-sr1-run-4）。
//!
//! 主题独立成文件（沿用 ARC-R6 的单文件拆分口径）：`observe.rs` 是 SSE 主题、
//! `http.rs` 是 cancel 解阻主题、`panic_safety.rs` 是 watchdog 解阻主题——本文件是
//! 「待答清单 + 久等告警」主题，混进既有文件会破坏拆分口径。
//!
//! **并发面三件事**（写用例前必须知道，否则必然写出假红用例）：
//! 1. `DEVFORGE_UI_PENDING_WARN_MS` 是**进程级 env**：并行用例会互相改阈值 → 读/写该键
//!    的用例必须串行（`warn_env_lock`）。
//! 2. env 是进程级 ⇒ **不得断言「全进程只有一条 WARN」**：并行用例的待答也会被告警。
//!    断言必须按本用例的唯一 question 过滤（WARN 行带 `question_prefix`）。
//! 3. 进程级日志缓冲是**共享**的：`capture_jsonl` 每次开跑都会清空它（test_support 的
//!    口径）→ 一次性大窗口捕获会随机丢行。故告警断言走**小窗口高频轮询**
//!    （`poll_warn_lines`），且丢行时**换一条新待答**重试（一条待答只告警一次，
//!    重试必须换条目）。

#[allow(unused_imports)]
use super::*;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// 久等告警阈值的环境变量名（与生产代码同键）。
const WARN_ENV: &str = "DEVFORGE_UI_PENDING_WARN_MS";

/// 轮询进程级缓冲的窗口大小（毫秒）。
///
/// 小窗口 = 告警行从「写入」到「被取走」的暴露窗口小 → 被并行用例顺手清掉的机会小。
const POLL_WINDOW_MS: u64 = 2;

/// 告警观察的重试上限（丢行时换一条新待答重来；一条待答只告警一次）。
const MAX_ATTEMPTS: usize = 12;

// ---------- 小工具 ----------

/// 久等告警旋钮是进程级 env：并行用例会互相串味（一个用例把阈值调小，另一个用例的
/// 待答也会被告警；两个用例同时改值 → 断言假红）。凡读/写该键的用例串行走这把锁。
fn warn_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 设阈值（返回原值，供恢复）。
///
/// SAFETY: 调用方必须持 [`warn_env_lock`]——该 env 是进程级的，并行改值会让断言假红。
fn set_warn_knob(ms: u64) -> Option<String> {
    let prev = std::env::var(WARN_ENV).ok();
    // SAFETY: 持锁期间本进程只有本用例写这个键（读者是扫描线程，只读不写）。
    unsafe { std::env::set_var(WARN_ENV, ms.to_string()) };
    prev
}

/// 恢复阈值（原值不存在则删除该键）。
///
/// SAFETY: 同 [`set_warn_knob`]（调用方持锁）。
fn restore_warn_knob(prev: Option<String>) {
    // SAFETY: 持锁期间独占该键。
    unsafe {
        match prev {
            Some(v) => std::env::set_var(WARN_ENV, v),
            None => std::env::remove_var(WARN_ENV),
        }
    }
}

/// 唯一问题串：同一进程内不重复（并行用例的待答不得互相串味）。
///
/// 结构：唯一标记**在前 80 字内**（WARN 行只带 `question_prefix` = 前 80 字 → 按标记
/// 过滤），后跟中文长文本（总长 > 80 字 → 前缀截断真的发生：按字节截断会切在多字节
/// 字符中间，前缀失真、断言变红）。
fn unique_question(tag: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!(
        "待答清单-{tag}-{}-{n}-{}",
        std::process::id(),
        "中文问题内容需要足够长才能触发前缀截断".repeat(3)
    )
}

/// 告警行里的过滤键：question 的前 80 字（= `question_prefix` 的口径）。
fn warn_filter_key(question: &str) -> String {
    question.chars().take(80).collect()
}

/// 起一条待答（独立线程 + 长超时；应答由用例负责——不答就挂到超时）。
fn spawn_ask(
    ui: Arc<UiRelay>,
    question: &str,
) -> std::thread::JoinHandle<crate::error::Result<String>> {
    let q = question.to_string();
    std::thread::spawn(move || {
        ui.ask_blocking_with_timeout(&q, &["选项A"], Some(std::time::Duration::from_secs(30)))
    })
}

/// 等待待答表就位（登记发生在 ask 线程里，与用例线程不同步）。
fn wait_pending(ui: &UiRelay, n: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while ui.pending_count() < n {
        assert!(
            std::time::Instant::now() < deadline,
            "待答条目未就位（期望 {n}，实得 {}）",
            ui.pending_count()
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// 读待答清单（裸数组）。
async fn get_pending(router: &axum::Router) -> Value {
    let resp = send(router, "GET", "/api/v1/ui/pending", None).await;
    assert_eq!(resp.status(), 200, "清单端点必须 200（空表也是 200 + []）");
    body_json(resp).await
}

/// 应答一条待答（HTTP 面）→ 响应体。
async fn answer_pending(router: &axum::Router, id: &str, value: &str) -> Value {
    let resp = send(
        router,
        "POST",
        &format!("/api/v1/ui/{id}/answer"),
        Some(json!({"value": value})),
    )
    .await;
    assert_eq!(resp.status(), 200, "应答必须 200（只有未知 id 才 404）");
    body_json(resp).await
}

/// 取一条 JSONL 日志里的字段值（tracing 的 JSON 格式把事件字段放在 `fields` 下，
/// level/timestamp/target 在顶层——两处都找一遍）。
fn log_field(line: &str, key: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("fields")
        .and_then(|f| f.get(key))
        .or_else(|| v.get(key))
        .cloned()
}

/// 轮询进程级缓冲，收齐含 `key` 的久等告警行（最多 `want` 条；收不齐就按窗口截止）。
///
/// 为什么轮询而不是一次性大窗口捕获：进程级缓冲是共享的，并行用例的 `capture_jsonl`
/// 会清空它 → 大窗口捕获会随机丢行（假红）。小窗口高频轮询把暴露窗口压到最小。
fn poll_warn_lines(key: &str, want: usize, window: std::time::Duration) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let deadline = std::time::Instant::now() + window;
    while std::time::Instant::now() < deadline {
        let chunk = crate::observability::test_support::capture_jsonl_process_wide(|| {
            std::thread::sleep(std::time::Duration::from_millis(POLL_WINDOW_MS));
        });
        for l in chunk
            .lines()
            .filter(|l| l.contains("\"event\":\"ui.pending_warn\"") && l.contains(key))
        {
            if !lines.iter().any(|x| x == l) {
                lines.push(l.to_string());
            }
        }
        if lines.len() > want {
            break; // 超额 = 真缺陷（重复告警），别再等
        }
    }
    lines
}

/// 收尾：应答掉当前唯一那条待答，放行 ask 线程。
fn settle_one(ui: &UiRelay) {
    if let Some(v) = ui.pending_snapshot().first() {
        let _ = ui.answer(&v.id, "选项A");
    }
}

/// 观察一轮「造待答 → 等告警」：返回 (观察到的告警行, 该轮的 question, 该轮的待答 id)。
///
/// `scenario` 负责造出这一轮的待答（返回待答 id 与 ask 线程句柄）。
///
/// 丢行（并行用例清了共享的进程级缓冲）不是产品缺陷 → 换一条**新待答**重试（有界）：
/// 一条待答只告警一次，重试必须换条目。`want` = 期望行数（0 = 一次都不该有）。
fn observe_warns(
    ui: &Arc<UiRelay>,
    want: usize,
    window: std::time::Duration,
    scenario: impl Fn(Arc<UiRelay>, &str) -> (String, std::thread::JoinHandle<crate::error::Result<String>>),
) -> (Vec<String>, String, String) {
    let mut last = (Vec::new(), String::new(), String::new());
    for _ in 0..MAX_ATTEMPTS {
        let question = unique_question("warn");
        let (id, handle) = scenario(ui.clone(), &question);
        let lines = poll_warn_lines(&warn_filter_key(&question), want, window);
        settle_one(ui);
        let _ = handle.join();
        last = (lines, question, id);
        if last.0.len() >= want {
            return last;
        }
    }
    last
}

// ---------- 清单面（验收 1/2/3/4/5/6） ----------

#[tokio::test]
async fn pending_list_shows_the_ask_with_its_sse_id() {
    // 验收 1：清单里的 id 与 SSE `ui.request` 的 id 同源（监工「从事件看到 id → 查清单
    // → 应答」这条链路靠它接上），question 是原文，age_ms 是数字，run_id 键存在。
    //
    // 错实现形态（任一命中即红）：①清单 id 另起编号（与 SSE 的 id 对不上，监工查不到）
    // ②question 截断/变形（监工看不懂在问什么）③age_ms 缺失或不是数字（排序/老化无从判断）
    // ④run_id 键整个不存在（消费方无法区分「无归属」与「字段没实现」）。
    let f = fixture();
    // 先订阅 SSE（broadcast 只投递订阅后的事件），再发起 ask——顺序不可倒置
    let resp = send(&f.router, "GET", "/api/v1/events", None).await;
    assert_eq!(resp.status(), 200);
    let mut stream = resp.into_body().into_data_stream();

    let question = unique_question("sse");
    let q2 = question.clone();
    let handle = spawn_ask(f.state.ui.clone(), &q2);

    // 从 SSE 流里取 ui.request 的 id（监工在流上看到的就是这个 id）
    let mut got = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !got.contains("ui.request") {
        assert!(tokio::time::Instant::now() < deadline, "SSE 未送出 ui.request");
        let chunk = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
            .await
            .expect("chunk 超时");
        match chunk {
            Some(Ok(bytes)) => got.push_str(&String::from_utf8_lossy(&bytes)),
            _ => break,
        }
    }
    let v: Value = serde_json::from_str(
        got.split("data: ")
            .find(|l| l.contains("ui.request"))
            .and_then(|l| l.split('\n').next())
            .expect("data 行"),
    )
    .unwrap();
    let sse_id = v["id"].as_str().unwrap().to_string();

    wait_pending(&f.state.ui, 1);
    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 1, "恰好一条待答：{body}");
    let item = &arr[0];
    assert_eq!(
        item["id"].as_str(),
        Some(sse_id.as_str()),
        "清单 id 必须与 SSE ui.request 的 id 同源：{body}"
    );
    assert_eq!(
        item["question"].as_str(),
        Some(question.as_str()),
        "question 必须是原文：{body}"
    );
    assert!(
        item["age_ms"].as_u64().is_some(),
        "age_ms 必须是数字（且非负）：{body}"
    );
    assert!(
        item.get("run_id").is_some(),
        "run_id 键必须存在（无归属时是 null，不是缺键）：{body}"
    );

    // 收尾：应答放行 ask 线程
    answer_pending(&f.router, &sse_id, "选项A").await;
    assert_eq!(handle.join().unwrap().unwrap(), "选项A");
}

#[tokio::test]
async fn pending_list_run_id_is_null_without_run_context() {
    // 验收 2：无 run 上下文的 ask → 清单里 run_id 是 JSON null。
    //
    // 错实现形态（任一命中即红）：①run_id 键缺失（消费方分不清「无归属」与「没实现」）
    // ②run_id 落空串（与「知道归属但 id 为空」混同）③run_id 落 "null" 字符串。
    let f = fixture();
    let question = unique_question("no-run");
    let handle = spawn_ask(f.state.ui.clone(), &question);
    wait_pending(&f.state.ui, 1);

    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 1, "恰好一条待答：{body}");
    assert!(
        arr[0]["run_id"].is_null(),
        "无 run 上下文 → run_id 必须是 JSON null：{body}"
    );

    // 收尾
    let id = arr[0]["id"].as_str().unwrap().to_string();
    answer_pending(&f.router, &id, "选项A").await;
    handle.join().unwrap().unwrap();
}

#[tokio::test]
async fn pending_list_run_id_is_the_run_id_within_run_context() {
    // 验收 3：run 上下文里的 ask → 清单里 run_id 等于该 run id。
    //
    // 错实现形态（任一命中即红）：①run_id 恒为 null（监工分不清是哪条 run 在等人）
    // ②run_id 落成 run 名的片段/大小写变形（对不上台账）。
    let f = fixture();
    let question = unique_question("with-run");
    let run_id = format!("run-pending-with-{}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let ui = f.state.ui.clone();
    let q = question.clone();
    let rid = run_id.clone();
    let handle = std::thread::spawn(move || {
        crate::observability::begin_run_in_deferred(root, &rid, "plan");
        let r = ui.ask_blocking_with_timeout(&q, &["选项A"], Some(std::time::Duration::from_secs(30)));
        crate::observability::end_run(&rid);
        r
    });
    wait_pending(&f.state.ui, 1);

    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 1, "恰好一条待答：{body}");
    assert_eq!(
        arr[0]["run_id"].as_str(),
        Some(run_id.as_str()),
        "有 run 上下文 → run_id 必须等于该 run id：{body}"
    );

    // 收尾
    let id = arr[0]["id"].as_str().unwrap().to_string();
    answer_pending(&f.router, &id, "选项A").await;
    handle.join().unwrap().unwrap();
}

#[tokio::test]
async fn pending_list_is_ordered_by_registration_time() {
    // 验收 4：先后登记的 A、B → 清单顺序 [A, B]（按登记时间升序，同毫秒按 id 兜底）。
    //
    // 错实现形态（任一命中即红）：①按 HashMap 迭代序返回（顺序随机，多跑几次就变）
    // ②按 id 排序（id 与登记时间不是一回事）③登记晚的排前面（倒序）。
    let f = fixture();
    let qa = unique_question("order-a");
    let qb = unique_question("order-b");
    let h_a = spawn_ask(f.state.ui.clone(), &qa);
    wait_pending(&f.state.ui, 1);
    let h_b = spawn_ask(f.state.ui.clone(), &qb);
    wait_pending(&f.state.ui, 2);

    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 2, "两条待答：{body}");
    assert_eq!(
        arr[0]["question"].as_str(),
        Some(qa.as_str()),
        "先登记的排在前：{body}"
    );
    assert_eq!(
        arr[1]["question"].as_str(),
        Some(qb.as_str()),
        "后登记的排在后：{body}"
    );
    // 「按登记时间升序」的可观察面：先登记的 age 更大（沿数组 age 非增）。
    // （同一 relay 内 id 与登记顺序同序，故这条是「按登记时间」而非「按 id」的唯一可观察差。）
    let age_a = arr[0]["age_ms"].as_u64().expect("age_ms 是数字");
    let age_b = arr[1]["age_ms"].as_u64().expect("age_ms 是数字");
    assert!(
        age_a >= age_b,
        "先登记的待答 age 不得更小（登记时间升序）：{body}"
    );

    // 收尾：两条都应答
    for v in f.state.ui.pending_snapshot() {
        answer_pending(&f.router, &v.id, "选项A").await;
    }
    h_a.join().unwrap().unwrap();
    h_b.join().unwrap().unwrap();
}

#[tokio::test]
async fn answering_removes_the_entry_from_the_pending_list() {
    // 验收 5：应答后该条同时从 pending 表与清单消失。
    //
    // 错实现形态（任一命中即红）：①应答只解阻不摘表（清单常驻幽灵条目 → 监工误判还在等）
    // ②应答返回非 200 ③应答体不是 {"ok":true}。
    let f = fixture();
    let question = unique_question("answer");
    let handle = spawn_ask(f.state.ui.clone(), &question);
    wait_pending(&f.state.ui, 1);

    let body = get_pending(&f.router).await;
    let id = body.as_array().unwrap()[0]["id"].as_str().unwrap().to_string();
    let ans = answer_pending(&f.router, &id, "选项A").await;
    assert_eq!(ans["ok"], json!(true), "应答体必须是 {{\"ok\":true}}：{ans}");
    assert_eq!(
        handle.join().unwrap().unwrap(),
        "选项A",
        "应答值必须回到 ask 线程"
    );

    let body = get_pending(&f.router).await;
    assert!(
        body.as_array().expect("裸数组").is_empty(),
        "应答后清单必须清空：{body}"
    );
    assert_eq!(f.state.ui.pending_count(), 0, "应答后待答表必须清空");
}

#[tokio::test]
async fn pending_list_is_empty_array_when_nothing_is_pending() {
    // 验收 6：无待答 → 200 + 裸空数组 []（不是 404、不是 500、不是 {"pending":[]}）。
    //
    // 错实现形态（任一命中即红）：①空表 404（监工把「当前没人等」读成「端点不存在」）
    // ②包成 {"pending":[]}（消费方按需求字面取 body[0] 会拿到 null）③空表 500。
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/ui/pending", None).await;
    assert_eq!(resp.status(), 200, "空表也是 200（不是 404）");
    let body = body_json(resp).await;
    assert!(body.is_array(), "空表必须是裸数组 []：{body}");
    assert!(
        body.as_array().unwrap().is_empty(),
        "空表必须是空数组：{body}"
    );
    assert!(
        body.get("pending").is_none(),
        "不得包成 {{\"pending\":[]}}：{body}"
    );
}

// ---------- 告警面（验收 7/8/9/10） ----------

#[test]
fn long_pending_ask_emits_exactly_one_warn_line() {
    // 验收 7 + 8：久等告警**恰好一条**、四字段齐；条目继续存活不得再告警（防刷屏）。
    //
    // 错实现形态（任一命中即红）：①压根不告警（盲区没消——这就是本变更要治的病）
    // ②每个周期都告警（刷屏，日志被噪音埋掉）③只带部分字段（事后定位不到是哪条待答）
    // ④打标按 question 文本（见下一个用例）。
    let _guard = warn_env_lock();
    let prev = set_warn_knob(50);
    crate::observability::test_support::install(); // 先装捕获器：晚装的会漏掉早到的告警
    let f = fixture();
    let ui = f.state.ui.clone();

    // 观察窗口：≥ 4 个扫描周期（阈值 50 → 周期 25ms）→ 首条告警必在其中，
    // 且窗口后半段仍在「继续存活」状态（若实现会重复告警，这里就能抓到第二条）。
    let window = std::time::Duration::from_millis(600);
    let (lines, used_question, used_id) = observe_warns(&ui, 1, window, |ui, question| {
        let handle = spawn_ask(ui.clone(), question);
        wait_pending(&ui, 1);
        let id = ui
            .pending_snapshot()
            .first()
            .map(|v| v.id.clone())
            .unwrap_or_default();
        (id, handle)
    });
    restore_warn_knob(prev);

    assert_eq!(
        lines.len(),
        1,
        "该待答恰好一条 WARN（继续存活不得再告警）：{lines:?}"
    );
    let line = &lines[0];
    assert!(line.contains("\"level\":\"WARN\""), "必须是 WARN 级：{line}");
    assert!(
        line.contains("\"event\":\"ui.pending_warn\""),
        "事件名必须是 ui.pending_warn：{line}"
    );
    assert!(
        line.contains("\"component\":\"ui\""),
        "必须带 component 字段：{line}"
    );
    assert!(
        line.contains(&format!("\"id\":\"{used_id}\"")),
        "必须带本条待答的 id：{line}"
    );
    assert!(
        line.contains("\"run_id\":\"null\""),
        "无 run 上下文 → run_id 落 null（不是缺字段、不是 Debug 串）：{line}"
    );
    assert!(line.contains("\"age_ms\":"), "必须带 age_ms 字段：{line}");
    assert!(
        line.contains("\"question_prefix\":\""),
        "必须带 question_prefix 字段：{line}"
    );
    let age = log_field(line, "age_ms").and_then(|v| v.as_u64());
    assert!(
        age.unwrap_or(0) >= 50,
        "age_ms 必须不小于阈值（超阈值才告警）：{line}"
    );
    let prefix = log_field(line, "question_prefix")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        !prefix.is_empty() && used_question.starts_with(&prefix),
        "question_prefix 必须是原 question 的前缀：{line}"
    );
}

#[test]
fn warn_line_carries_the_run_id_when_the_ask_has_a_run_context() {
    // 钉 step 3 的字段口径（有归属方向）：run_id 就是那个 run id —— 可 grep 的 triage
    // 字段，不是 Debug 串 `Some("run-a")`，也不是缺字段。
    //
    // 错实现形态（任一命中即红）：①run_id 落 `Some("run-a")`（grep 不到 run id）
    // ②run_id 恒为 null（分不清是哪条 run 在等人）③run_id 字段缺失。
    let _guard = warn_env_lock();
    let prev = set_warn_knob(50);
    crate::observability::test_support::install();
    let f = fixture();
    let ui = f.state.ui.clone();
    let run_id = format!("run-pending-warn-{}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let rid = run_id.clone();

    let (lines, _question, _id) = observe_warns(
        &ui,
        1,
        std::time::Duration::from_millis(600),
        |ui, question| {
            let ui2 = ui.clone();
            let q = question.to_string();
            let rid = rid.clone();
            let root = root.clone();
            let handle = std::thread::spawn(move || {
                crate::observability::begin_run_in_deferred(root, &rid, "plan");
                let r = ui2.ask_blocking_with_timeout(
                    &q,
                    &["选项A"],
                    Some(std::time::Duration::from_secs(30)),
                );
                crate::observability::end_run(&rid);
                r
            });
            wait_pending(&ui, 1);
            let id = ui
                .pending_snapshot()
                .first()
                .map(|v| v.id.clone())
                .unwrap_or_default();
            (id, handle)
        },
    );
    restore_warn_knob(prev);

    assert_eq!(lines.len(), 1, "恰好一条 WARN：{lines:?}");
    assert!(
        lines[0].contains(&format!("\"run_id\":\"{run_id}\"")),
        "WARN 的 run_id 必须是那个 run id：{}",
        lines[0]
    );
}

#[test]
fn warn_knob_zero_disables_the_warning() {
    // 验收 9：`DEVFORGE_UI_PENDING_WARN_MS=0` → 零条 WARN（旋钮关闭）。
    //
    // 错实现形态（任一命中即红）：①`0` 被当成「阈值 0」→ 每条待答都告警（关不掉）
    // ②`0` 被当成「不限时」（与相邻旋钮 DEVFORGE_UI_ASK_TIMEOUT_MS 混同 → 语义分叉）
    // ③非数字/空串回退缺省时把 `0` 也算进去。
    let _guard = warn_env_lock();
    let prev = set_warn_knob(0);
    crate::observability::test_support::install();
    let f = fixture();
    let ui = f.state.ui.clone();

    let question = unique_question("warn-off");
    let handle = spawn_ask(ui.clone(), &question);
    wait_pending(&ui, 1);
    // 观察窗口 ≥ 4 个扫描周期：旋钮关着就该一条都不出
    let lines = poll_warn_lines(&warn_filter_key(&question), 0, std::time::Duration::from_millis(400));
    settle_one(&ui);
    let _ = handle.join();
    restore_warn_knob(prev);

    assert!(
        lines.is_empty(),
        "旋钮关闭 → 零条 WARN（实测 {} 条）：{lines:?}",
        lines.len()
    );
}

#[test]
fn a_new_ask_with_the_same_question_gets_its_own_warn_line() {
    // 验收 10：已告警的待答被 answer 后，同一 relay 再发一条**同 question** 的新 ask
    // → 新条目独立产生 1 条 WARN（打标按条目，不按文本）。
    //
    // 错实现形态（任一命中即红）：①打标按 question 文本（第二条被误判「已告警」→ 静默）
    // ②两条共用一个 id（清单与应答都指不准）。
    let _guard = warn_env_lock();
    let prev = set_warn_knob(50);
    crate::observability::test_support::install();
    let f = fixture();
    let ui = f.state.ui.clone();

    let mut lines: Vec<String> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for _ in 0..MAX_ATTEMPTS {
        lines.clear();
        ids.clear();
        let question = unique_question("warn-same");

        // 第一条：等它告警 → 应答掉
        let h1 = spawn_ask(ui.clone(), &question);
        wait_pending(&ui, 1);
        let first = poll_warn_lines(&warn_filter_key(&question), 1, std::time::Duration::from_millis(400));
        let id1 = ui.pending_snapshot().first().map(|v| v.id.clone()).unwrap_or_default();
        let _ = ui.answer(&id1, "选项A");
        let _ = h1.join();
        if first.is_empty() {
            continue; // 丢行（并行用例清了缓冲）→ 换一批重来
        }
        lines.extend(first);
        ids.push(id1);

        // 第二条：同 question 的新条目 → 必须独立再告警一次
        let h2 = spawn_ask(ui.clone(), &question);
        wait_pending(&ui, 1);
        let second = poll_warn_lines(&warn_filter_key(&question), 2, std::time::Duration::from_millis(400));
        let id2 = ui.pending_snapshot().first().map(|v| v.id.clone()).unwrap_or_default();
        let _ = ui.answer(&id2, "选项A");
        let _ = h2.join();
        lines.extend(second);
        ids.push(id2);
        if lines.len() >= 2 {
            break;
        }
    }
    restore_warn_knob(prev);

    assert_eq!(
        lines.len(),
        2,
        "同 question 的两条待答各告警一次（打标按条目不按文本）：{lines:?}"
    );
    assert_ne!(ids[0], ids[1], "两条待答必须是不同的 id：{ids:?}");
    assert!(
        lines[0].contains(&format!("\"id\":\"{}\"", ids[0])),
        "第一条告警必须指向第一条待答：{}",
        lines[0]
    );
    assert!(
        lines[1].contains(&format!("\"id\":\"{}\"", ids[1])),
        "第二条告警必须指向第二条待答：{}",
        lines[1]
    );
}

// ---------- 并发面（验收 11/12） ----------

#[test]
fn many_ask_answer_rounds_do_not_deadlock() {
    // 验收 11：阈值极小（每条都会告警）+ 20 轮 ask/answer 交替 → 20 次 answer 全 200、
    // 20 个 ask 线程全部返回，无死锁、无超时（扫描与应答共用同一张表但互不阻塞）。
    //
    // 错实现形态（任一命中即红）：①扫描线程持锁发日志（应答被日志 I/O 卡住 → 死锁）
    // ②应答路径与扫描路径抢同一把锁且不释放 → 死锁 ③超时清理与新登记不同步 → 幽灵条目。
    //
    // 写成同步用例 + 自建 runtime：旋钮锁必须覆盖整段并发窗口，而 `MutexGuard` 不得跨
    // await（clippy::await_holding_lock）——锁在同步帧里持有，异步段只跑 HTTP 往返。
    let _guard = warn_env_lock();
    let prev = set_warn_knob(1); // 极小阈值：每条待答都会告警，扫描与应答正面并发
    let f = fixture();
    let ui = f.state.ui.clone();
    let base = unique_question("rounds");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        for i in 0..20 {
            let question = format!("{base}-{i}");
            let handle = spawn_ask(ui.clone(), &question);
            wait_pending(&ui, 1); // 每轮只有本轮这一条在等（上一轮已应答摘表）
            let entry = ui
                .pending_snapshot()
                .into_iter()
                .find(|v| v.question == question)
                .expect("本轮待答必须在表里");
            let ans = answer_pending(&f.router, &entry.id, "选项A").await;
            assert_eq!(ans["ok"], json!(true), "第 {i} 轮应答体必须是 ok：{ans}");
            assert_eq!(
                handle.join().unwrap().unwrap(),
                "选项A",
                "第 {i} 轮的 ask 线程必须返回（不得死锁）"
            );
        }
    });
    restore_warn_knob(prev);
    assert_eq!(ui.pending_count(), 0, "20 轮跑完不得残留待答");
}

#[tokio::test]
async fn abort_run_only_removes_its_own_run_from_the_pending_list() {
    // 验收 12：两条待答分属 run-a / run-b → `abort_run("run-a")` 返回 1，清单只剩 run-b
    // 那条（并发 run 的 dialog 不得被误伤——server #3 的隔离语义不许退化）。
    //
    // 错实现形态（任一命中即红）：①归属判定写错谓词 → 连 run-b 一起打掉
    // ②abort_run 只解阻不摘表（清单仍显示已被中止的待答）③返回值不是被摘条数。
    let f = fixture();
    let ui = f.state.ui.clone();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let (root_a, root_b) = (dir_a.path().to_path_buf(), dir_b.path().to_path_buf());
    let (id_a, id_b) = (
        format!("run-pending-abort-a-{}", std::process::id()),
        format!("run-pending-abort-b-{}", std::process::id()),
    );

    let ui_a = ui.clone();
    let rid_a = id_a.clone();
    let h_a = std::thread::spawn(move || {
        crate::observability::begin_run_in_deferred(root_a, &rid_a, "plan");
        let r = ui_a.ask_blocking_with_timeout(
            "run-a 的提问",
            &["选项A"],
            Some(std::time::Duration::from_secs(30)),
        );
        crate::observability::end_run(&rid_a);
        r
    });
    let ui_b = ui.clone();
    let rid_b = id_b.clone();
    let h_b = std::thread::spawn(move || {
        crate::observability::begin_run_in_deferred(root_b, &rid_b, "plan");
        let r = ui_b.ask_blocking_with_timeout(
            "run-b 的提问",
            &["选项A"],
            Some(std::time::Duration::from_secs(30)),
        );
        crate::observability::end_run(&rid_b);
        r
    });
    wait_pending(&ui, 2);

    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 2, "两条待答：{body}");
    let run_ids: Vec<&str> = arr.iter().filter_map(|v| v["run_id"].as_str()).collect();
    assert!(
        run_ids.contains(&id_a.as_str()) && run_ids.contains(&id_b.as_str()),
        "两条待答必须各带自己的 run id：{body}"
    );

    // 只解自己那个 run
    assert_eq!(ui.abort_run(&id_a), 1, "run-a 的待答必须被摘掉");
    assert!(h_a.join().unwrap().is_err(), "被中止的 ask 必须失败返回");

    let body = get_pending(&f.router).await;
    let arr = body.as_array().expect("清单是裸数组");
    assert_eq!(arr.len(), 1, "清单只剩 run-b 那条：{body}");
    assert_eq!(
        arr[0]["run_id"].as_str(),
        Some(id_b.as_str()),
        "剩下的必须是 run-b 那条：{body}"
    );

    // 收尾：run-b 那条也收掉
    let _ = ui.abort_all();
    let _ = h_b.join();
}
