//! doc run 的断点状态（plan 010）：进度字段住 `RunState.data`（黑板），引擎不解释语义。
//!
//! - `done`：已通过审查并落盘的单元号（预分配编号，重跑不漂移——P008 D020）
//! - `files`：每个单元当初产出的文件（相对产出根），续跑复核证据用（010 D003）
//! - `review_rounds` / `issues_total`：审查轮次与累计问题（账单用）

use crate::runstate::RunState;

/// 进度字段（存在 `RunState.data` 里，引擎不解释；plan 010 D005）。
pub(super) fn data_obj(state: &mut RunState) -> &mut serde_json::Map<String, serde_json::Value> {
    if !state.data.is_object() {
        state.data = serde_json::json!({});
    }
    state.data.as_object_mut().expect("data 已确保为对象")
}

/// 已完成的单元列表。
pub fn done_units(state: &RunState) -> Vec<String> {
    crate::runstate::resume::done_units(state)
}

/// 计划总单元数（DR-5：进度聚合端点用；未写入 = None）。
pub fn units_total(state: &RunState) -> Option<usize> {
    state
        .data
        .get("units_total")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

/// 是否已完成。
pub fn is_done(state: &RunState, unit: &str) -> bool {
    done_units(state).iter().any(|u| u == unit)
}

/// 标记单元完成，并登记它产出的文件（相对产出根）。
pub fn mark_done(state: &mut RunState, unit: &str, files: &[String]) {
    let mut done = done_units(state);
    if !done.iter().any(|u| u == unit) {
        done.push(unit.to_string());
    }
    let obj = data_obj(state);
    obj.insert("done".into(), serde_json::json!(done));
    let files_map = obj
        .entry("files")
        .or_insert_with(|| serde_json::json!({}));
    if !files_map.is_object() {
        *files_map = serde_json::json!({});
    }
    files_map
        .as_object_mut()
        .expect("files 已确保为对象")
        .insert(unit.to_string(), serde_json::json!(files));
}

/// 审查轮次 +1（包括重试与打回）。
pub fn bump_review(state: &mut RunState) -> usize {
    let round = review_rounds(state) + 1;
    data_obj(state).insert("review_rounds".into(), serde_json::json!(round));
    round
}

pub fn review_rounds(state: &RunState) -> usize {
    state
        .data
        .get("review_rounds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

/// R6-P64：fix 队列落盘——断路器文案承诺「resume 可续」，断点必须真实承载
/// （旧口径为栈变量，重启全丢，承诺落空）。
pub fn record_fix_queue(state: &mut RunState, queue: &[(String, String, Vec<String>, u32)]) {
    let rows: Vec<serde_json::Value> = queue
        .iter()
        .map(|(unit, route, issues, round)| {
            serde_json::json!({ "unit": unit, "route": route, "issues": issues, "round": round })
        })
        .collect();
    data_obj(state).insert("fix_queue".into(), serde_json::json!(rows));
}

/// R6-P64：断点回放——恢复 fix 队列（重启续跑继续修，旧口径一条都不恢复）。
pub fn restore_fix_queue(state: &RunState) -> Vec<(String, String, Vec<String>, u32)> {
    state
        .data
        .get("fix_queue")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|o| {
                    Some((
                        o.get("unit")?.as_str()?.to_string(),
                        o.get("route")?.as_str()?.to_string(),
                        o.get("issues")?
                            .as_array()?
                            .iter()
                            .filter_map(|i| i.as_str().map(str::to_string))
                            .collect(),
                        o.get("round")?.as_u64()? as u32,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// R6-P64：连续失败计数落盘（断路器跨重启延续——重启不能洗掉断路进度）。
pub fn set_consecutive_fails(state: &mut RunState, n: u32) {
    data_obj(state).insert("consecutive_fails".into(), serde_json::json!(n));
}

pub fn consecutive_fails(state: &RunState) -> u32 {
    state
        .data
        .get("consecutive_fails")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32
}

/// 累计问题数。
pub fn add_issues(state: &mut RunState, n: usize) {
    let total = issues_total(state) + n;
    data_obj(state).insert("issues_total".into(), serde_json::json!(total));
}

pub fn issues_total(state: &RunState) -> usize {
    state
        .data
        .get("issues_total")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}
