//! `change-id` 与黑板目录（P009 T003 / D008）。
//!
//! 口径：
//! - 形状 `change-<YYYYMMDD>-<slug>`，slug 由需求短名归一化（小写、非字母数字折成 `-`）。
//! - **幂等**：同一 `project/task` 重复触发必须复用已有 id——黑板里 `<root>/<project>/<task>/change/current`
//!   记着当前 id，存在即复用；不复用会让「同一件事」在黑板里长出第二份计划。
//! - 目录形状：`<plans_root>/<project>/<task>/change/<change-id>/`（计划与历史落这里；
//!   检查点按 010 D001 另住 `<plans_root>/<project>/<task>/change/state.json` 同层不混）。

use std::path::{Path, PathBuf};

use crate::error::{Context as _, DevforgeError, Result};

/// 黑板里的 change 根（`plans/<p>/<t>/change`）。
pub fn change_root(plans_root: &Path, project: &str, task: &str) -> PathBuf {
    plans_root.join(project).join(task).join("change")
}

/// 当前 change-id 的记录文件。
pub fn current_marker(plans_root: &Path, project: &str, task: &str) -> PathBuf {
    change_root(plans_root, project, task).join("current")
}

/// 把需求短名归一化成 slug：小写、只留 `[a-z0-9]`，其余折成单个 `-`，首尾去 `-`。
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("change");
    }
    // 太长会把路径撑爆，也难看；取前 40 个字符
    out.chars().take(40).collect()
}

/// 组装 id（不落盘；`date` 由调用方给，便于测试与固定口径）。
pub fn make_change_id(date: &str, slug: &str) -> String {
    format!("change-{date}-{}", slugify(slug))
}

/// 取（或首次分配）change-id，并确保目录存在。**幂等**。
///
/// `date` 形如 `20261020`（与 id 同口径）；仅在首次分配时使用。
pub fn assign_change_id(
    plans_root: &Path,
    project: &str,
    task: &str,
    slug: &str,
    date: &str,
) -> Result<(String, PathBuf)> {
    let marker = current_marker(plans_root, project, task);
    if let Ok(existing) = std::fs::read_to_string(&marker) {
        let id = existing.trim().to_string();
        if !id.is_empty() {
            // 自审 CH-08：marker 是黑板里的普通文本文件（agent/半状态可写），读回的 id
            // 直接 join 进 change_root 前必须过白名单——`../..` 注入会变成任意目录
            // create_dir_all。不合法则告警并按无 marker 重新分配（不拒效，避免卡死流水线）。
            if let Err(e) = validate_change_id(&id) {
                tracing::warn!(
                    component = "change-id",
                    marker = %marker.display(),
                    raw = %id,
                    error = %e,
                    "current marker 里的 change-id 不合法，重新分配"
                );
            } else {
                let dir = change_root(plans_root, project, task).join(&id);
                if dir.is_dir() {
                    return Ok((id, dir));
                }
                // marker 存在但目录没了（被清过）：按已有 id 重建目录，不换号
                std::fs::create_dir_all(&dir).ctx(format!("重建黑板目录 {}", dir.display()))?;
                return Ok((id, dir));
            }
        }
    }

    let id = make_change_id(date, slug);
    let dir = change_root(plans_root, project, task).join(&id);
    std::fs::create_dir_all(&dir).ctx(format!("建黑板目录 {}", dir.display()))?;
    let marker_parent = marker.parent().expect("marker 必有父目录");
    std::fs::create_dir_all(marker_parent).ctx(format!("建 {}", marker_parent.display()))?;
    std::fs::write(&marker, format!("{id}\n"))
        .ctx(format!("写当前 change-id {}", marker.display()))?;
    Ok((id, dir))
}

/// `2026-10-20` 风格的日期 → id 段（`20261020`）。
pub fn date_segment(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}{month:02}{day:02}")
}

/// 今天的日期段（本地时钟；不引第三方时间库）。
///
/// 环境变量 `DEVFORGE_TODAY=YYYY-MM-DD` 可覆盖（CI / 固定口径用）。
pub fn date_segment_now() -> String {
    if let Ok(raw) = std::env::var("DEVFORGE_TODAY") {
        let parts: Vec<&str> = raw.trim().split('-').collect();
        if parts.len() == 3
            && let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            )
        {
            return date_segment(y, m, d);
        }
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    date_segment(y, m, d)
}

/// 今天的 `(年, 月, 日)`（本地时钟，`DEVFORGE_TODAY` 同样可覆盖）。
///
/// 给需要**数值**而非 id 段的调用方用（自审 44：调用方不该自己字节切片日期段）。
pub fn today_ymd() -> (i32, u32, u32) {
    if let Ok(raw) = std::env::var("DEVFORGE_TODAY") {
        let parts: Vec<&str> = raw.trim().split('-').collect();
        if parts.len() == 3
            && let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            )
        {
            return (y, m, d);
        }
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs.div_euclid(86_400))
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)。Howard Hinnant 的 civil_from_days。
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// id 合法性（防止有人手工塞进奇怪名字）。
pub fn validate_change_id(id: &str) -> Result<()> {
    let ok = id.starts_with("change-")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(DevforgeError::Config(format!("非法 change-id：{id}")))
    }
}

#[cfg(test)]
#[path = "change_id_tests.rs"]
mod tests;
