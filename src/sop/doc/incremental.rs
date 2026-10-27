//! 增量模式的接线层（008 · T024 / D021）。
//!
//! 两件事：
//! 1. 决定本轮跑「全量」还是「增量」（显式开关 + 状态里的 `base_sha`）；
//! 2. 把影响集合翻译成「本轮只派发哪些单元」，并在开工前把受影响单元从 done 里摘掉
//!    ——这样中途崩了也不会留下假绿。
//!
//! 影响面怎么算归 [`crate::docgen::impact`]；这里只做状态与计划的搬运。

use std::path::Path;

use crate::docgen::impact::{ASSEMBLY_PAGES, Impact};
use crate::runstate::RunState;
use crate::runstate::resume::ResumePlan;

/// 状态扩展段里的基线字段。
pub const BASE_SHA: &str = "base_sha";
/// 本轮增量记录（受影响集合，供报告/排障）。
pub const INCREMENTAL: &str = "incremental";

/// 本轮跑法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// 全量（附原因，进日志）。
    Full { reason: String },
    /// 只跑受影响单元。
    ///
    /// `head` = 规划时刻的 HEAD。自审 major：基线推进（advance_base）必须以它
    /// 为界——旧口径把它丢弃，推进写到运行结束时的 HEAD，会越过本轮实际未
    /// 覆盖的提交（下轮 diff 把没覆盖的当成已覆盖）。
    Incremental { base: String, head: String },
}

/// 读环境变量：`DEVFORGE_DOC_INCREMENTAL=1` 开启，`DEVFORGE_DOC_BASE=<sha>` 覆盖基线。
pub fn enabled_from_env() -> (bool, Option<String>) {
    let on = std::env::var("DEVFORGE_DOC_INCREMENTAL")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
    let base = std::env::var("DEVFORGE_DOC_BASE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (on, base)
}

/// 决议本轮的跑法。**默认关**：没开开关就老样子全量。
pub fn decide_mode(
    state: Option<&RunState>,
    head: Option<&str>,
    req_base: Option<&str>,
    enabled: bool,
) -> Mode {
    if !enabled {
        return Mode::Full {
            reason: "未开启增量（DEVFORGE_DOC_INCREMENTAL）".into(),
        };
    }
    let base = req_base
        .map(str::to_string)
        .or_else(|| state.and_then(state_base));
    let Some(base) = base else {
        return Mode::Full {
            reason: "状态无 base_sha（首次运行或旧状态）".into(),
        };
    };
    let Some(head) = head else {
        return Mode::Full {
            reason: "取不到 git HEAD".into(),
        };
    };
    Mode::Incremental {
        base,
        head: head.to_string(),
    }
}

/// 状态里的基线。
pub fn state_base(state: &RunState) -> Option<String> {
    state
        .data
        .get(BASE_SHA)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// 写基线（首次运行 = 当时的 HEAD）。
pub fn set_base(state: &mut RunState, sha: &str) {
    if let Some(obj) = state.data.as_object_mut() {
        obj.insert(BASE_SHA.into(), serde_json::Value::String(sha.into()));
    }
}

/// 把基线推进到 `head`——**只在终审 PASS 时调用**（D021：失败保留旧值）。
///
/// 返回是否真的写入了。
pub fn advance_base(state: &mut RunState, head: Option<&str>, passed: bool) -> bool {
    if !passed {
        return false;
    }
    let Some(head) = head.filter(|h| !h.is_empty()) else {
        return false;
    };
    set_base(state, head);
    true
}

/// 把影响集合翻译成本轮派发计划，并从 done 里摘掉受影响单元。
///
/// - `all_units`：清单里的单元顺序（派发顺序必须稳定）；
/// - 返回本轮要跑的单元（受影响集合，按 `all_units` 顺序）。
pub fn merge_plan(
    plan: &mut ResumePlan,
    state: &mut RunState,
    impact: &Impact,
    all_units: &[String],
) -> Vec<String> {
    let affected: std::collections::BTreeSet<&str> =
        impact.units.iter().map(String::as_str).collect();
    let to_run: Vec<String> = all_units
        .iter()
        .filter(|u| affected.contains(u.as_str()))
        .cloned()
        .collect();
    let skip: Vec<String> = all_units
        .iter()
        .filter(|u| !affected.contains(u.as_str()))
        .cloned()
        .collect();

    let mut done: Vec<String> = state
        .data
        .get("done")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let old_done = done.clone();
    done.retain(|u| !affected.contains(u.as_str()));
    let base = state_base(state);
    if let Some(obj) = state.data.as_object_mut() {
        obj.insert(
            "done".into(),
            serde_json::Value::Array(done.into_iter().map(serde_json::Value::String).collect()),
        );
        obj.insert(
            INCREMENTAL.into(),
            serde_json::json!({
                "base": base,
                "units": impact.units,
                "assembly_review": impact.assembly_review,
            }),
        );
    }

    plan.resumed = true;
    // 自审 minor（100）：旧口径直接覆盖 plan.redo——增量模式下引擎基于证据算出的 redo
    // （产物已消失的单元）被整体丢弃，它们不会被重跑。现在取并集：已验证需重做 ∪ 受影响
    // 且曾在 done 里的单元。
    let mut redo: Vec<String> = std::mem::take(&mut plan.redo);
    for u in old_done.into_iter().filter(|u| affected.contains(u.as_str())) {
        if !redo.iter().any(|r| r == &u) {
            redo.push(u);
        }
    }
    plan.redo = redo;
    plan.skip = skip;
    plan.to_run = to_run.clone();
    to_run
}

/// git HEAD（拿不到就 None，调用方降级全量）。
pub fn head_sha(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// 一致性校验（机械部分）：汇编页里的 `path:line` 是否还解析得动、词表还在不在。
///
/// 只报事实，不修；结果进终审输入，由审查官判是否 FAIL。
pub fn consistency_notes(docs_root: &Path, repo: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let glossary = docs_root.join("_meta/glossary.md");
    if !glossary.exists() {
        out.push("词表缺失：`_meta/glossary.md`（应由门 0 自动导出）".into());
    }
    let inventory = docs_root.join("_meta/inventory.json");
    if !inventory.exists() {
        out.push("清单缺失：`_meta/inventory.json`（增量对照与覆盖审计都依赖它）".into());
    }
    for page in ASSEMBLY_PAGES {
        let path = docs_root.join(page);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for r in path_refs(&text) {
            match resolve_ref(repo, &r) {
                Ok(()) => {}
                Err(why) => out.push(format!("汇编页 `{page}` 的引用 `{r}` 已陈旧：{why}")),
            }
        }
    }
    out
}

/// 从文本里抽 `path:line` 引用（反引号或裸写都收）。
fn path_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace() || matches!(c, '`' | '(' | ')' | '[' | ']' | '，' | '。' | '；' | '、' | '"' | '\'')
    }) {
        let token = raw.trim_matches(|c: char| matches!(c, ',' | '.' | ':' | ';'));
        let Some((path, line)) = token.rsplit_once(':') else {
            continue;
        };
        if path.is_empty() || !line.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !(path.contains('/') || path.contains('.')) || path.starts_with("http") {
            continue;
        }
        out.push(format!("{path}:{line}"));
    }
    out
}

fn resolve_ref(repo: &Path, r: &str) -> Result<(), String> {
    let (path, line) = r.rsplit_once(':').ok_or("不是 path:line")?;
    let line: usize = line.parse().map_err(|_| "行号不是数字")?;
    // 自审 major：路径逃逸拒绝——绝对路径/.. 段可读仓外任意文件（整文件进内存，
    // 设备/超大文件可致 OOM），且引用解析只应面向仓内相对路径。
    if std::path::Path::new(path).is_absolute()
        || path.split('/').any(|seg| seg == "..")
    {
        return Err(format!("引用路径越界（绝对路径或含 .. 段）：{path}"));
    }
    let abs = repo.join(path);
    let text = std::fs::read_to_string(&abs).map_err(|_| "文件不存在/不可读")?;
    let total = text.lines().count();
    if line == 0 || line > total {
        return Err(format!("行号越界（文件共 {total} 行）"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "incremental_tests.rs"]
mod tests;
