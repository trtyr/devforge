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

/// 把单元从 done 里摘除（重做前用：旧记录已不可信）。
pub fn unmark_done(state: &mut RunState, unit: &str) {
    let kept: Vec<String> = done_units(state).into_iter().filter(|u| u != unit).collect();
    data_obj(state).insert("done".into(), serde_json::json!(kept));
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
