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
    /// 本次加载到的断点快照（自审 minor 186：旧口径 plan 内部加载却不交出来，调用方必须
    /// 二次加载体 resume_brief —— redo 与 state 可能来自两个不同快照）。
    pub state: Option<RunState>,
}

/// 计算续跑计划。
///
/// - `fresh = true`：忽略断点，全部重跑（调用方负责清空检查点）。
///
/// 2026-10-27 自审 minor 185：**生产路径不经此分支**——`start` 入口走
/// [`start_gate`]（它负责 clear 断点），这里的 `fresh` 是给直接调用方的兜底。
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
            state: None,
        };
    }

    let key = StateKey::new(project, task, sop.name());
    match load(plans_root, &key) {
        Loaded::Found(state) => {
            // 自审 minor（42）：done 名单只求值一次（旧口径在 filter 闭包里按单元重复分配）
            let done = done_units(&state);
            // 自审 minor（187）：redo 必须与本轮清单求交——旧口径会把不在本轮计划里的
            // 单元报给 agent（它无从下手）。
            let redo: Vec<String> = sop
                .verify_done(&state)
                .into_iter()
                .filter(|u| all_units.iter().any(|a| a == u))
                .collect();
            let skip: Vec<String> = all_units
                .iter()
                .filter(|u| !redo.iter().any(|r| r == *u))
                .filter(|u| done.iter().any(|d| d == *u))
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
                state: Some(state),
            }
        }
        Loaded::Missing => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: None,
            state: None,
        },
        Loaded::Unusable(reason) => ResumePlan {
            resumed: false,
            to_run: all_units.to_vec(),
            redo: Vec::new(),
            skip: Vec::new(),
            note: Some(reason.detail),
            state: None,
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
    // 自审 minor（188）：本函数是**破坏性**公开 API（删盘上文件），路径段必须自校验，
    // 不能把安全性完全押在调用方身上。
    key.validate()?;
    if fresh {
        crate::runstate::clear(plans_root, &key)?;
        // 自审 minor（189）：冷启动原因以前只回显到 HTTP 响应体，服务端不留痕
        tracing::info!(
            component = "runstate",
            event = "runstate.start_cold",
            project = %key.project,
            task = %key.task,
            sop = %key.sop,
            reason = "fresh",
            "start：fresh 清断点，冷启动"
        );
        return Ok(StartDecision {
            resumed: false,
            note: Some("fresh: true —— 已清空该 SOP 的断点，全量重跑".into()),
        });
    }
    let decision = match load(plans_root, &key) {
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
            note: Some(reason.detail.clone()),
        }),
    };
    // 自审 minor（189）：断点不可用导致的冷启动必须能从日志重建（不只活在响应体里）
    if let Ok(d) = &decision {
        if let Some(note) = &d.note {
            tracing::warn!(
                component = "runstate",
                event = "runstate.start_cold",
                project = %key.project,
                task = %key.task,
                sop = %key.sop,
                reason = %note,
                "start：断点不可用，冷启动"
            );
        }
    }
    decision
}

/// 从状态扩展段取 `done` 名单（各 SOP 统一放这里，便于引擎不碰业务结构）。
pub fn done_units(state: &RunState) -> Vec<String> {
    let Some(raw) = state.data.get("done") else {
        return Vec::new();
    };
    let Some(arr) = raw.as_array() else {
        // 自审 minor（190）：畸形扩展段旧口径静默降级成「一个单元都没做」，
        // 而 plan 仍报 resumed=true / note=None —— 排障时看不出断点已损坏。
        tracing::warn!(
            component = "runstate",
            field = "done",
            shape = ?raw,
            "断点扩展段 done 不是数组，按空名单处理（断点可能已损坏）"
        );
        return Vec::new();
    };
    let out: Vec<String> = arr
        .iter()
        .filter_map(|v| match v.as_str() {
            Some(s) => Some(s.to_string()),
            None => {
                tracing::warn!(
                    component = "runstate",
                    item = ?v,
                    "断点扩展段 done 内有非字符串条目，已跳过"
                );
                None
            }
        })
        .collect();
    out
}

/// 重做单元必须从 done 名单里摘掉（自审 minor 99：旧口径 redo 只用于本轮 skip 计算，
/// 断点里的旧 done 标记不动——重做失败后下一轮会把「审不过/写坏」的单元当成已完成跳过）。
///
/// 返回是否真的改动了名单（调用方据此决定是否落盘）。
pub fn invalidate_done(state: &mut RunState, redo: &[String]) -> bool {
    if redo.is_empty() {
        return false;
    }
    let mut done = done_units(state);
    let before = done.len();
    done.retain(|u| !redo.iter().any(|r| r == u));
    if done.len() == before {
        return false;
    }
    if let Some(obj) = state.data.as_object_mut() {
        obj.insert(
            "done".into(),
            serde_json::Value::Array(done.into_iter().map(serde_json::Value::String).collect()),
        );
    }
    true
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
