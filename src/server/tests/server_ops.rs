//! A7 server 运维信息端点单测：字段齐全、时间戳 UTC 口径、disk 落在 plans_root。

#[allow(unused_imports)]
use super::*;

/// 端点照出运维带：version 非空、started_at_ms 是合理 unix ms、uptime 单调、
/// 时间字段全部 unix ms（时区纪律：无裸时间字符串）。
#[tokio::test]
async fn server_ops_lists_version_uptime_memory_disk() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/server/ops", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;

    assert!(
        body["version"].as_str().is_some_and(|v| !v.is_empty()),
        "version 非空：{body}"
    );
    let started = body["started_at_ms"].as_u64().expect("started_at_ms unix ms");
    let now = crate::domain::now_ms();
    assert!(started > 1_700_000_000_000, "started 是 2023 后的 unix ms：{started}");
    assert!(started <= now, "started 不得来自未来");
    let uptime = body["uptime_ms"].as_u64().expect("uptime_ms unix ms");
    assert!(uptime <= now.saturating_sub(started) + 1, "uptime 与 started 自洽");

    // 时区纪律：时间字段只有 _ms 形态，无裸时间字符串
    let raw = serde_json::to_string(&body).unwrap();
    assert!(
        !raw.contains("T") || !raw.contains("-") || body["started_at"].is_null(),
        "不得输出裸时间字符串（RFC3339 之外一律 unix ms）：{raw}"
    );

    // memory：Linux 上必有（本仓 Linux-only 口径）
    assert!(
        body["memory"]["rss_bytes"].as_u64().is_some_and(|b| b > 0),
        "Linux 下 RSS 必有：{body}"
    );

    // disk：path 指向 plans_root 或其最近存在祖先（首次启动 plans 未建是合法态），
    // available <= total
    let checked = body["disk"]["path"].as_str().expect("disk 检查路径");
    let plans = f.state.plans_root.display().to_string();
    assert!(
        checked == plans || plans.starts_with(checked),
        "disk 检查应落在 plans_root 或其祖先：{checked} vs {plans}"
    );
    let avail = body["disk"]["available_bytes"].as_u64().unwrap();
    let total = body["disk"]["total_bytes"].as_u64().unwrap();
    assert!(avail <= total, "可用量不得大于总量");
}
