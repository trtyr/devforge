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
    Incremental { base: String },
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
    let _ = head;
    Mode::Incremental { base }
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
    plan.redo = old_done
        .into_iter()
        .filter(|u| affected.contains(u.as_str()))
        .collect();
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
    let abs = repo.join(path);
    let text = std::fs::read_to_string(&abs).map_err(|_| "文件不存在/不可读")?;
    let total = text.lines().count();
    if line == 0 || line > total {
        return Err(format!("行号越界（文件共 {total} 行）"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docgen::impact::Impact;
    use crate::runstate::StateKey;

    fn state(done: &[&str], base: Option<&str>) -> RunState {
        let key = StateKey::new("p", "t", "doc-from-zero");
        let mut data = serde_json::json!({ "done": done });
        if let Some(b) = base {
            data["base_sha"] = serde_json::Value::String(b.into());
        }
        RunState::new(&key, data)
    }

    fn plan(all: &[&str], done: &[&str]) -> ResumePlan {
        ResumePlan {
            resumed: true,
            to_run: Vec::new(),
            redo: Vec::new(),
            skip: all.iter().map(|s| s.to_string()).filter(|u| !done.contains(&u.as_str())).collect(),
            note: None,
        }
    }

    #[test]
    fn base_advances_only_on_pass() {
        let mut st = state(&[], Some("old"));
        assert!(!advance_base(&mut st, Some("new"), false), "失败不得推进");
        assert_eq!(state_base(&st).as_deref(), Some("old"));
        assert!(advance_base(&mut st, Some("new"), true));
        assert_eq!(state_base(&st).as_deref(), Some("new"));
        assert!(!advance_base(&mut st, None, true), "取不到 HEAD 不得瞎写");
    }

    #[test]
    fn disabled_mode_is_full() {
        let st = state(&[], Some("abc"));
        assert_eq!(
            decide_mode(Some(&st), Some("def"), None, false),
            Mode::Full {
                reason: "未开启增量（DEVFORGE_DOC_INCREMENTAL）".into()
            }
        );
    }

    #[test]
    fn missing_base_degrades_to_full_with_reason() {
        let st = state(&["unit-a"], None);
        match decide_mode(Some(&st), Some("head"), None, true) {
            Mode::Full { reason } => assert!(reason.contains("base_sha"), "{reason}"),
            other => panic!("应降级全量：{other:?}"),
        }
    }

    #[test]
    fn missing_head_degrades_to_full() {
        let st = state(&[], Some("abc"));
        match decide_mode(Some(&st), None, None, true) {
            Mode::Full { reason } => assert!(reason.contains("HEAD"), "{reason}"),
            other => panic!("应降级全量：{other:?}"),
        }
    }

    #[test]
    fn explicit_base_overrides_state_and_enables_incremental() {
        let st = state(&[], Some("old"));
        assert_eq!(
            decide_mode(Some(&st), Some("head"), Some("given"), true),
            Mode::Incremental {
                base: "given".into()
            }
        );
    }

    #[test]
    fn only_affected_units_are_dispatched() {
        let all = ["unit-a", "unit-b", "unit-c"];
        let mut st = state(&["unit-a", "unit-b", "unit-c"], Some("base1"));
        let mut p = plan(&all, &[]);
        let impact = Impact {
            units: vec!["unit-b".into()],
            ..Default::default()
        };
        let to_run = merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
        assert_eq!(to_run, vec!["unit-b".to_string()], "只派发受影响单元");
        assert_eq!(p.skip, vec!["unit-a".to_string(), "unit-c".to_string()]);
        assert_eq!(p.redo, vec!["unit-b".to_string()], "原本 done 的受影响单元记 redo");
    }

    #[test]
    fn affected_unit_is_removed_from_done_before_dispatch() {
        let all = ["unit-a", "unit-b"];
        let mut st = state(&["unit-a", "unit-b"], Some("base1"));
        let mut p = plan(&all, &[]);
        let impact = Impact {
            units: vec!["unit-a".into()],
            ..Default::default()
        };
        merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
        let done = crate::runstate::resume::done_units(&st);
        assert_eq!(done, vec!["unit-b".to_string()], "受影响单元必须先摘掉，崩溃也不假绿");
    }

    #[test]
    fn incremental_record_is_written_into_state() {
        let all = ["unit-a"];
        let mut st = state(&["unit-a"], Some("base1"));
        let mut p = plan(&all, &[]);
        let impact = Impact {
            units: vec!["unit-a".into()],
            assembly_review: vec!["README.md".into()],
            ..Default::default()
        };
        merge_plan(&mut p, &mut st, &impact, &all.map(str::to_string));
        assert_eq!(st.data["incremental"]["units"][0], "unit-a");
        assert_eq!(st.data["incremental"]["assembly_review"][0], "README.md");
    }

    #[test]
    fn stale_assembly_reference_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/a.rs"), "a\nb\n").unwrap();
        std::fs::write(
            docs.join("README.md"),
            "见 `src/a.rs:2`，旧引用 `src/a.rs:99`，失效 `src/gone.rs:1`\n",
        )
        .unwrap();
        let notes = consistency_notes(&docs, &repo);
        assert!(
            notes.iter().any(|n| n.contains("src/a.rs:99")),
            "越界行号要报：{notes:?}"
        );
        assert!(
            notes.iter().any(|n| n.contains("src/gone.rs:1")),
            "缺失文件要报：{notes:?}"
        );
        assert!(
            !notes.iter().any(|n| n.contains("src/a.rs:2`")),
            "有效引用不该报：{notes:?}"
        );
    }

    #[test]
    fn missing_glossary_and_inventory_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = consistency_notes(tmp.path(), tmp.path());
        assert!(notes.iter().any(|n| n.contains("glossary.md")), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("inventory.json")), "{notes:?}");
    }
}
