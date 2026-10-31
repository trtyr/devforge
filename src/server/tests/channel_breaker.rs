//! 渠道熔断（gap-B · 派发前渠道熔断）单测。
//!
//! 本文件按步长出：step 1 = 存储层（熔断表落盘 / 读回 / 到期清理），
//! step 2 = 签名与归因纯函数，step 3 = tick 扫描步，step 4 = 派发闸门与状态端点。
//!
//! 口径提醒（计划 §5）：
//! - 熔断条目存**绝对到期时刻**（unix ms）——读回后逐毫秒相等，不许漂；
//! - 旧版本 store.json（无 `channel_breakers` 键）必须能读回（数据迁移方向）。

use crate::domain::store::{FinishOutcome, Store};
use crate::server::scheduler::{
    channel_incident_ttl_ms, extract_agent_role, tick, ChannelIndex, SchedulerConfig,
    CHANNEL_BREAKER_MAX_TTL_MS, CHANNEL_BREAKER_TTL_CREDENTIAL_MS,
    CHANNEL_BREAKER_TTL_RATE_LIMIT_MS,
};

/// 记下的熔断条目在落盘 → 读回后渠道、到期时刻、原因摘要逐项相等。
#[test]
fn a_recorded_breaker_survives_a_store_roundtrip_with_the_same_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    let expires = 1_700_000_000_000u64;
    {
        let mut store = Store::new().with_persist(path.clone());
        store.record_channel_breaker("deepseek-flash", expires, "402 Payment Required");
    }
    let loaded = Store::load(&path).unwrap();
    assert_eq!(loaded.channel_breakers.len(), 1, "熔断条目应随 store 落盘");
    let b = &loaded.channel_breakers[0];
    assert_eq!(b.channel, "deepseek-flash");
    assert_eq!(b.expires_at_ms, expires, "到期时刻是绝对时间戳，读回不漂");
    assert_eq!(b.last_seen_reason, "402 Payment Required");
}

/// 旧版本 store.json（没有 channel_breakers 键）必须能读回——否则升级即炸。
#[test]
fn a_store_json_without_the_breaker_field_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.json");
    std::fs::write(&path, r#"{"projects":[],"tasks":[],"runs":[]}"#).unwrap();
    let loaded = Store::load(&path).unwrap();
    assert!(
        loaded.channel_breakers.is_empty(),
        "旧文件读回后熔断表为空（不是报错，也不是凭空造条目）"
    );
}

/// 同一条渠道重复记熔断：条目仍是**一条**，到期取较晚者，原因摘要刷新为最新。
#[test]
fn recording_the_same_channel_twice_keeps_one_entry_with_the_later_expiry() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 2_000, "第一次");
    store.record_channel_breaker("deepseek-flash", 5_000, "第二次");
    store.record_channel_breaker("deepseek-flash", 3_000, "第三次");
    assert_eq!(store.channel_breakers.len(), 1, "同渠道只留一条");
    assert_eq!(
        store.channel_breakers[0].expires_at_ms, 5_000,
        "到期取较晚者（后一次来得早不缩短已定的冷却窗口）"
    );
    assert_eq!(store.channel_breakers[0].last_seen_reason, "第三次");
}

/// 剩余冷却毫秒按「到期时刻 − 现在」算；到期即不再算冷却。
#[test]
fn remaining_ms_counts_down_to_the_expiry_and_stops_at_zero() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 5_000, "402");
    assert_eq!(store.channel_breaker_remaining_ms("deepseek-flash", 4_000), 1_000);
    assert_eq!(store.channel_breaker_remaining_ms("deepseek-flash", 5_000), 0);
    assert_eq!(store.channel_breaker_remaining_ms("grok-4.7", 4_000), 0);
}

/// 过期条目被清掉（TTL 过后自然解除），未到期的留下。
#[test]
fn expired_breakers_are_purged_and_the_live_one_survives() {
    let mut store = Store::new();
    store.record_channel_breaker("deepseek-flash", 1_000, "402");
    store.record_channel_breaker("grok-4.7", 5_000, "429");
    assert_eq!(store.purge_expired_channel_breakers(999), 0, "未到期不动手");
    assert_eq!(store.channel_breakers.len(), 2);
    assert_eq!(store.purge_expired_channel_breakers(1_000), 1, "到期那条被清");
    assert_eq!(store.channel_breakers.len(), 1);
    assert_eq!(store.channel_breakers[0].channel, "grok-4.7");
    assert_eq!(
        store.active_channel_breakers(1_000).len(),
        1,
        "活跃列表只含未到期条目"
    );
    assert_eq!(store.channel_breakers[0].expires_at_ms, 5_000);
}

// ---------- step 2：签名分档与归因 ----------

/// 账户/凭证类签名（402 / 401 / 403 / Insufficient Balance / No API key found）→ 10 分钟档。
#[test]
fn credential_signatures_map_to_the_ten_minute_tier() {
    for reason in [
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required（配置问题：缺 API key）",
        "agent[change-plan-agent] 命令 prompt 失败：HTTP 401 Unauthorized",
        "agent[change-plan-agent] 命令 prompt 失败：HTTP 403 Forbidden",
        "agent[change-plan-agent] 命令 prompt 失败：Insufficient Balance",
        "agent[change-plan-agent] 命令 prompt 失败：No API key found",
    ] {
        assert_eq!(
            channel_incident_ttl_ms(reason),
            Some(CHANNEL_BREAKER_TTL_CREDENTIAL_MS),
            "凭证类签名应落 10 分钟档：{reason}"
        );
    }
}

/// 限流类（429）→ 60 秒短档（瞬时故障，短冷却自动恢复）。
#[test]
fn a_rate_limit_signature_maps_to_the_one_minute_tier() {
    assert_eq!(
        channel_incident_ttl_ms("agent[change-coding-agent] 命令 prompt 失败：HTTP 429 too many requests"),
        Some(CHANNEL_BREAKER_TTL_RATE_LIMIT_MS)
    );
}

/// 连接错误类 → 10 分钟硬档。
#[test]
fn connection_errors_map_to_the_ten_minute_tier() {
    for reason in [
        "agent[change-test-agent] 命令 prompt 失败：fetch failed: ECONNRESET",
        "agent[change-test-agent] 命令 prompt 失败：connect ECONNREFUSED 127.0.0.1:443",
        "agent[change-test-agent] 命令 prompt 失败：ETIMEDOUT",
        "agent[change-test-agent] 命令 prompt 失败：connection reset by peer",
        "agent[change-test-agent] 命令 prompt 失败：broken pipe",
    ] {
        assert_eq!(
            channel_incident_ttl_ms(reason),
            Some(CHANNEL_BREAKER_TTL_CREDENTIAL_MS),
            "连接错误类属 10 分钟硬档：{reason}"
        );
    }
}

/// 同时命中限流与凭证类 → 取**较长**档（保守方向：宁冷却久一点）。
#[test]
fn a_reason_hitting_two_tiers_takes_the_longer_one() {
    assert_eq!(
        channel_incident_ttl_ms("agent[change-plan-agent] 命令 prompt 失败：HTTP 429 … No API key found"),
        Some(CHANNEL_BREAKER_TTL_CREDENTIAL_MS)
    );
}

/// 「路径 + 行号」「版本号」形态不是状态码——不熔断（误伤面比漏判大一个量级）。
#[test]
fn a_path_and_line_number_form_is_not_a_status_code() {
    for reason in [
        "越界改动：3 个文件超出契约 owned_paths：src/domain/store.rs:402",
        "越界改动：3 个文件超出契约 owned_paths：src/domain/store.rs 402",
        "配置错误：模型版本 v1.402 不受支持",
        "配置错误：模型版本 v1-402 不受支持",
    ] {
        assert_eq!(
            channel_incident_ttl_ms(reason),
            None,
            "路径/版本号里的数字不是状态码：{reason}"
        );
    }
}

/// 质量失败文案不是渠道事故——不熔断（否则质量劣化被当成渠道死亡，错杀健康渠道）。
#[test]
fn a_quality_failure_reason_is_not_a_channel_incident() {
    assert_eq!(
        channel_incident_ttl_ms(
            "change-plan-agent 连续 4 次未输出 <stage-complete>N（已追补 3 次），判定为质量失败（不自动重启）；最近一次回复尾巴：ok"
        ),
        None
    );
}

/// 裸 timeout 不是连接死亡（仓内 timeout 也表示「等 run 超时」）——不熔断。
#[test]
fn a_bare_timeout_is_not_a_connection_incident() {
    assert_eq!(
        channel_incident_ttl_ms("agent[change-test-agent] 命令 prompt 失败：timeout"),
        None
    );
    assert_eq!(
        channel_incident_ttl_ms("agent[change-test-agent] 命令 prompt 失败：timed out"),
        None
    );
}

/// 归因锚点两种文案形态都能抽到 agent 名。
#[test]
fn the_agent_role_is_extracted_from_both_attribution_anchors() {
    assert_eq!(
        extract_agent_role("agent[change-plan-agent] 命令 prompt 失败：402 Payment Required").as_deref(),
        Some("change-plan-agent")
    );
    assert_eq!(
        extract_agent_role(
            "change-plan-agent 连续 4 次未输出 <stage-complete>N（已追补 3 次），判定为质量失败"
        )
        .as_deref(),
        Some("change-plan-agent")
    );
}

/// 抽不出 agent 名 → None（调用方据此只记 WARN、不熔断）。
#[test]
fn an_unattributable_reason_yields_no_agent_role() {
    assert_eq!(extract_agent_role("402 Payment Required"), None);
    assert_eq!(extract_agent_role(""), None);
}

/// sop → 渠道列表按清单派生（精确归因的底座）。
#[test]
fn the_sop_channel_map_follows_the_manifest() {
    let manifest = crate::config::ConfigManifest::builtin();
    let index = ChannelIndex::build(&manifest, &crate::domain::sop::SopRegistry::builtin().ids());
    assert_eq!(
        index.channel_of("change-plan-agent"),
        Some("deepseek-flash"),
        "change-plan-agent 绑定的池条目即渠道"
    );
    assert_eq!(index.channel_of("change-plan-review"), Some("grok-4.7"));
    assert_eq!(index.channel_of("not-an-agent"), None);
    assert_eq!(
        index.channels_for_sop("doc-from-zero"),
        ["minimax-m3.1-flash".to_string(), "grok-4.7".to_string()],
        "doc-from-zero 的角色只绑 minimax 与 grok"
    );
    assert!(
        index
            .channels_for_sop("change")
            .contains(&"deepseek-flash".to_string()),
        "change 引用 deepseek-flash"
    );
}

/// 清单自洽断言（全仓今天没有这条）：`profiles_for_sop` 的每个角色都在清单里，
/// 且它的 `model` 在模型池里——归因链「profile 名 → 清单 agent → 池条目」的前提。
#[test]
fn every_sop_profile_exists_in_the_manifest_and_points_at_a_pool_entry() {
    let manifest = crate::config::ConfigManifest::builtin();
    for sop in crate::domain::sop::SopRegistry::builtin().ids() {
        for role in crate::engine::builtin::BuiltinRpcEngine::profiles_for_sop(sop) {
            let agent = manifest
                .agents
                .iter()
                .find(|a| a.name == *role)
                .unwrap_or_else(|| panic!("sop `{sop}` 的角色 `{role}` 不在清单 agents 里"));
            assert!(
                manifest.model_pool.iter().any(|m| m.id == agent.model),
                "sop `{sop}` 的角色 `{role}` 指向的池条目 `{}` 不在 model_pool 里",
                agent.model
            );
        }
    }
}

/// 扫描窗口基准 = 三档里的最大 TTL（窗口取最小档会把长档失败整批漏掉）。
#[test]
fn the_scan_window_is_the_longest_ttl_tier() {
    assert_eq!(
        CHANNEL_BREAKER_MAX_TTL_MS,
        CHANNEL_BREAKER_TTL_CREDENTIAL_MS.max(CHANNEL_BREAKER_TTL_RATE_LIMIT_MS)
    );
}

// ---------- step 2：tick 扫描步 ----------

/// 建任务（绕过 API，直接操作 store——tick 单测只关心状态面）。
fn make_task(f: &super::Fixture, project: &str, id: &str, sop: &str) {
    let mut store = f.state.store.lock().unwrap();
    if store.get_project(project).is_none() {
        store
            .create_project(crate::domain::project::Project::new(
                project,
                std::path::PathBuf::from("/tmp/channel-breaker-fixture"),
                1,
            ))
            .unwrap();
    }
    store
        .create_task(
            &crate::domain::sop::SopRegistry::builtin(),
            project,
            id,
            sop,
            None,
            crate::domain::task::TaskDeps::default(),
        )
        .unwrap();
}

/// 造一条失败 run（带指定原因），返回 run id。
fn fail_run(f: &super::Fixture, project: &str, task: &str, reason: &str) -> String {
    let mut store = f.state.store.lock().unwrap();
    let run = store.begin_run(project, task).unwrap();
    store
        .finish_run(&run.id, FinishOutcome::Failed { reason: reason.into() })
        .unwrap();
    run.id.0.clone()
}

/// 事件环里的全部事件（按类型过滤用）。
fn events(f: &super::Fixture) -> Vec<serde_json::Value> {
    f.state
        .bus
        .replay_after(0)
        .into_iter()
        .map(|(_, raw)| serde_json::from_str(&raw).expect("事件应为 JSON"))
        .collect()
}

/// 事件环里某类型的条数。
fn event_count(f: &super::Fixture, kind: &str) -> usize {
    events(f)
        .iter()
        .filter(|e| e["type"] == serde_json::Value::String(kind.to_string()))
        .count()
}

/// store 里的熔断条目（渠道 id + 到期时刻）。
fn breakers(f: &super::Fixture) -> Vec<(String, u64)> {
    let store = f.state.store.lock().unwrap();
    store
        .channel_breakers
        .iter()
        .map(|b| (b.channel.clone(), b.expires_at_ms))
        .collect()
}

/// 402 失败 → 只熔断**该 agent 实际绑定**的那个池条目（精确归因，不按任务宽泛熔断）。
#[tokio::test]
async fn a_402_failure_trips_only_the_channel_of_the_failed_agent() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-a", "change");
    fail_run(
        &f,
        "devforge",
        "chan-a",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    let before = crate::domain::now_ms();
    tick(&f.state, &SchedulerConfig::default());
    let after = crate::domain::now_ms();

    let entries = breakers(&f);
    assert_eq!(entries.len(), 1, "只应有一条熔断条目：{entries:?}");
    assert_eq!(entries[0].0, "deepseek-flash", "change-plan-agent 绑定的池条目");
    let (channel, expires_at_ms) = entries[0].clone();
    assert_eq!(channel, "deepseek-flash");
    // 到期时刻 = tick 内的 now + TTL；now 落在 [before, after] 之间 → 上下界都用这两个采样点。
    assert!(
        expires_at_ms > before && expires_at_ms <= after + CHANNEL_BREAKER_TTL_CREDENTIAL_MS,
        "凭证类到期落在 (0, 600000]：{expires_at_ms} vs {before}..{after}"
    );
    assert_eq!(
        event_count(&f, "scheduler.channel_breaker_tripped"),
        1,
        "熔断事件发一条"
    );
    let evt = events(&f)
        .into_iter()
        .find(|e| e["type"] == "scheduler.channel_breaker_tripped")
        .unwrap();
    assert_eq!(evt["channel"], "deepseek-flash");
    assert_eq!(evt["agent"], "change-plan-agent");
    assert!(
        !evt["reason"].as_str().unwrap().is_empty(),
        "事件带原因摘要"
    );
}

/// 命中签名但归因不出 agent → 不熔断，只发 unattributed 事件（宁可漏熔，不可错杀）。
#[tokio::test]
async fn an_unattributable_incident_records_no_breaker_but_emits_a_warn_event() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-b", "change");
    let run_id = fail_run(&f, "devforge", "chan-b", "402 Payment Required");
    tick(&f.state, &SchedulerConfig::default());

    assert!(breakers(&f).is_empty(), "归因不出就不熔断：{:?}", breakers(&f));
    assert_eq!(event_count(&f, "scheduler.channel_breaker_unattributed"), 1);
    let evt = events(&f)
        .into_iter()
        .find(|e| e["type"] == "scheduler.channel_breaker_unattributed")
        .unwrap();
    assert_eq!(evt["run_id"], run_id.as_str());
}

/// 质量失败不是渠道事故 → 不熔断。
#[tokio::test]
async fn a_quality_failure_trips_no_breaker() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-c", "change");
    fail_run(
        &f,
        "devforge",
        "chan-c",
        "change-plan-agent 连续 4 次未输出 <stage-complete>N（已追补 3 次），判定为质量失败（不自动重启）",
    );
    tick(&f.state, &SchedulerConfig::default());
    assert!(breakers(&f).is_empty(), "质量失败不熔断：{:?}", breakers(&f));
    assert_eq!(event_count(&f, "scheduler.channel_breaker_tripped"), 0);
}

/// 扫描窗口基准是 `ended_at_ms`：跑了很久才 402 的 run 仍在窗口内。
#[tokio::test]
async fn the_scan_window_is_measured_from_the_ended_at_timestamp() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-d", "change");
    let run_id = fail_run(
        &f,
        "devforge",
        "chan-d",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    let now = crate::domain::now_ms();
    {
        let mut store = f.state.store.lock().unwrap();
        let run = store
            .runs
            .iter_mut()
            .find(|r| r.id.0 == run_id)
            .expect("run 应在 store 里");
        run.started_at_ms = now - 3_600_000; // 跑了一小时
        run.ended_at_ms = Some(now - 1_000); // 刚刚才结束
    }
    tick(&f.state, &SchedulerConfig::default());
    assert_eq!(
        breakers(&f).len(),
        1,
        "窗口基准是 ended_at_ms（不是 started_at_ms）"
    );
}

/// 超出窗口的陈旧失败不重放熔断（否则重启后会把历史事故再熔一遍）。
#[tokio::test]
async fn a_stale_failure_is_not_replayed_into_a_breaker() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-e", "change");
    let run_id = fail_run(
        &f,
        "devforge",
        "chan-e",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    let now = crate::domain::now_ms();
    {
        let mut store = f.state.store.lock().unwrap();
        let run = store
            .runs
            .iter_mut()
            .find(|r| r.id.0 == run_id)
            .expect("run 应在 store 里");
        run.ended_at_ms = Some(now - (CHANNEL_BREAKER_MAX_TTL_MS + 1_000));
    }
    tick(&f.state, &SchedulerConfig::default());
    assert!(breakers(&f).is_empty(), "陈旧失败不熔断：{:?}", breakers(&f));
}

/// 同一失败 run 连扫三次 → 到期时刻不推进、事件只发一次（否则熔断永不过期）。
#[tokio::test]
async fn the_same_failure_is_only_recorded_once() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-f", "change");
    fail_run(
        &f,
        "devforge",
        "chan-f",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    tick(&f.state, &SchedulerConfig::default());
    let first = breakers(&f);
    tick(&f.state, &SchedulerConfig::default());
    tick(&f.state, &SchedulerConfig::default());
    let third = breakers(&f);
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].1, third[0].1,
        "到期时刻不随 tick 推进（否则熔断永不过期）"
    );
    assert_eq!(
        event_count(&f, "scheduler.channel_breaker_tripped"),
        1,
        "事件不刷屏"
    );
}

/// 429 失败 → 60 秒短档（瞬时故障，短冷却自动恢复）。
#[tokio::test]
async fn a_rate_limit_failure_records_the_one_minute_tier() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-g", "change");
    fail_run(
        &f,
        "devforge",
        "chan-g",
        "agent[change-coding-agent] 命令 prompt 失败：HTTP 429 too many requests",
    );
    let before = crate::domain::now_ms();
    tick(&f.state, &SchedulerConfig::default());
    let after = crate::domain::now_ms();
    let entries = breakers(&f);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "deepseek-flash");
    assert!(
        entries[0].1 > before && entries[0].1 <= after + CHANNEL_BREAKER_TTL_RATE_LIMIT_MS,
        "限流类到期落在 (0, 60000]：{:?}",
        entries[0]
    );
}

/// 连接错误类 → 10 分钟硬档。
#[tokio::test]
async fn a_connection_failure_records_the_ten_minute_tier() {
    let f = super::fixture();
    make_task(&f, "devforge", "chan-h", "change");
    fail_run(
        &f,
        "devforge",
        "chan-h",
        "agent[change-test-agent] 命令 prompt 失败：fetch failed: ECONNRESET",
    );
    let before = crate::domain::now_ms();
    tick(&f.state, &SchedulerConfig::default());
    let after = crate::domain::now_ms();
    let entries = breakers(&f);
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].1 > before && entries[0].1 <= after + CHANNEL_BREAKER_TTL_CREDENTIAL_MS,
        "连接错误类到期落在 (0, 600000]：{:?}",
        entries[0]
    );
}

/// 正在跑的 run 撞上冷却渠道 → 事件里看得见（纯观测：不拦、不改状态、不重复发）。
#[tokio::test]
async fn a_running_run_on_a_cooling_channel_is_reported_once_without_interference() {
    let f = super::fixture();
    // 一个失败 run 把 deepseek-flash 熔断掉。
    make_task(&f, "devforge", "chan-i", "change");
    fail_run(
        &f,
        "devforge",
        "chan-i",
        "agent[change-plan-agent] 命令 prompt 失败：402 Payment Required",
    );
    // 另一个 run 正在跑，它的任务引用同一条渠道（change → change-plan-agent → deepseek-flash）。
    make_task(&f, "devforge", "chan-j", "change");
    let running_id = {
        let mut store = f.state.store.lock().unwrap();
        store.begin_run("devforge", "chan-j").unwrap().id.0
    };
    tick(&f.state, &SchedulerConfig::default());

    assert_eq!(
        event_count(&f, "channel.running_on_cooldown"),
        1,
        "撞熔断事件发一条"
    );
    let evt = events(&f)
        .into_iter()
        .find(|e| e["type"] == "channel.running_on_cooldown")
        .unwrap();
    assert_eq!(evt["run_id"], running_id.as_str());
    assert_eq!(evt["channel"], "deepseek-flash");
    assert!(
        evt["remaining_secs"].as_u64().unwrap() >= 1,
        "事件带剩余秒数"
    );
    // 纯观测：run 与任务都还在跑。
    {
        let store = f.state.store.lock().unwrap();
        let run = store
            .runs
            .iter()
            .find(|r| r.id.0 == running_id)
            .expect("run 应在 store 里");
        assert_eq!(run.status, crate::domain::run::RunStatus::Running);
        let task = store.get_task("devforge", "chan-j").unwrap();
        assert_eq!(task.status, crate::domain::task::TaskStatus::Running);
    }
    // 再 tick 一次不重复发。
    tick(&f.state, &SchedulerConfig::default());
    assert_eq!(
        event_count(&f, "channel.running_on_cooldown"),
        1,
        "同一 run 不重复发（不刷屏）"
    );
}
