//! SOP 接入面（plan 010 · T002）：引擎只管「续到哪」，SOP 管「凭什么算做完」。
//!
//! 分工（010 D005）：
//! - **引擎**（本模块 + [`super`]）：状态怎么存、`start` 续跑/fresh 语义、复核调度、故障重启。
//! - **SOP**（实现 [`Resumable`]）：扩展段结构、`done` 的证据定义与复核、恢复时给 agent 的开工语。
//!
//! 这个 trait 刻意只有三个方法，且**不含任何具体 SOP 的业务词**——一旦这里出现某个 SOP 的
//! 单元名/阶段名，就说明抽象漏了，引擎会被迫知道业务细节。

use std::path::Path;

use crate::error::Result;
use crate::runstate::{Loaded, RunState, StateKey, load};

/// 一个可断点续跑的 SOP。
pub trait Resumable {
    /// SOP 名（与注册名一致，用作状态目录名）。
    fn name(&self) -> &str;

    /// 复核 `done`：返回**证据已失效、必须重做**的单元。
    ///
    /// 只做机械可判定的事（文件在不在、门禁还绿不绿）；语义判断归审查，不归这里。
    fn verify_done(&self, state: &RunState) -> Vec<String>;

    /// 恢复时追加给 agent 的开工语（重做了哪些、为什么）。空串表示不需要额外说明。
    fn resume_brief(&self, state: &RunState, redo: &[String]) -> String;
}

/// 续跑计划：调用方据此决定跳过哪些、重做哪些。
#[derive(Debug, Clone, PartialEq)]
pub struct ResumePlan {
    /// 是否从断点继续（false = 冷启动）。
    pub resumed: bool,
    /// 本次真正要跑的单元（按传入顺序，已剔除可信的 done）。
    pub to_run: Vec<String>,
    /// 本次要重做的单元（原本 done，但证据失效）。
    pub redo: Vec<String>,
    /// 本次跳过的单元（done 且证据成立）。
    pub skip: Vec<String>,
    /// 加载断点失败的原因（冷启动时非空，供记日志）。
    pub note: Option<String>,
}

/// 计算续跑计划。
///
/// - `fresh = true`：忽略断点，全部重跑（调用方负责清空检查点）。
/// - 断点缺失 / 不可用：冷启动，全部跑。
/// - 断点可用：`verify_done` 复核，证据失效的单元并入重做集合。
pub fn plan<S: Resumable + ?Sized>(
    plans_root: &Path,
    project: &str,
    task: &str,
    sop: &S,
    all_units: &[String],
    fresh: bool,
) -> ResumePlan {
    if fresh {
        return ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: Some("fresh: true —— 忽略断点全量重跑".into()),
        };
    }

    let key = StateKey::new(project, task, sop.name());
    match load(plans_root, &key) {
        Loaded::Found(state) => {
            let redo = sop.verify_done(&state);
            let skip: Vec<String> = all_units
                .iter()
                .filter(|u| !redo.iter().any(|r| r == *u))
                .filter(|u| done_units(&state).iter().any(|d| d == *u))
                .cloned()
                .collect();
            let to_run: Vec<String> = all_units
                .iter()
                .filter(|u| !skip.iter().any(|s| s == *u))
                .cloned()
                .collect();
            ResumePlan {
                resumed: true,
                to_run,
                redo,
                skip,
                note: None,
            }
        }
        Loaded::Missing => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: None,
        },
        Loaded::Unusable(reason) => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: Some(reason),
        },
    }
}

/// `start` 的续跑判定结果（plan 010 · T003）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartDecision {
    /// 是否从断点继续（false = 冷启动）。
    pub resumed: bool,
    /// 冷启动原因（fresh / 断点不可用），供日志与响应回显。
    pub note: Option<String>,
}

/// `start` 入口的续跑门：`fresh = true` 时清空该 SOP 的断点并回冷启动。
///
/// **只动该 SOP 自己那份**——同一 task 下其他 SOP 的状态文件不受影响（D001）。
pub fn start_gate(
    plans_root: &Path,
    project: &str,
    task: &str,
    sop_name: &str,
    fresh: bool,
) -> Result<StartDecision> {
    let key = StateKey::new(project, task, sop_name);
    if fresh {
        crate::runstate::clear(plans_root, &key)?;
        return Ok(StartDecision {
            resumed: false,
            note: Some("fresh: true —— 已清空该 SOP 的断点，全量重跑".into()),
        });
    }
    match load(plans_root, &key) {
        Loaded::Found(_) => Ok(StartDecision {
            resumed: true,
            note: None,
        }),
        Loaded::Missing => Ok(StartDecision {
            resumed: false,
            note: None,
        }),
        Loaded::Unusable(reason) => Ok(StartDecision {
            resumed: false,
            note: Some(reason),
        }),
    }
}

/// 从状态扩展段取 `done` 名单（各 SOP 统一放这里，便于引擎不碰业务结构）。
pub fn done_units(state: &RunState) -> Vec<String> {
    state
        .data
        .get("done")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
