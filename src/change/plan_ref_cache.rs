//! F8 方向 A：plan 引用核验缓存（引擎侧确定性缓存，selfhost 台账 F8）。
//!
//! 病根（c5 场实测）：plan 复审把全文 `path:line` 从头重核——上一轮已核且计划未变
//! 的引用占 ~95%，审查官的工具轮纯属重复劳动。而引擎每轮本就机械核全量引用
//! （`audit_inner_roots` 的 `path_refs` + `resolves`，零成本）——缓存的**价值不是省
//! 引擎的核验成本，是把「已核事实」显式交给审查官**：两轮结论一致的引用标注
//! 「引擎已核、两轮一致，直接采信」，只有新增/变化的引用才需要审查官用工具重走。
//!
//! 失效口径：ref 级 verdict 比较（同 ref_text 两轮 verdict 一致 = 命中），计划整体
//! hash 只作诊断展示；因此计划局部修改时，未变引用自动命中、改动处自动 fresh。
//! 缓存是优化不是正确性依赖：黑板缺文件/坏 JSON 一律 `None` 宁降级不炸（全量重核
//! 就是改动前的原行为）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::change::plan_audit::path_refs;

/// 黑板上的缓存文件名（随 run 生命周期，与 impl-steps 等同层）。
pub const REF_CACHE_FILE: &str = "plan-ref-cache.json";

/// 逐条引用核验记录。verdict 三态：`exists` / `missing` / `line-out-of-range`。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct RefEntry {
    pub ref_text: String,
    pub verdict: String,
}

/// 缓存主体（黑板 JSON 形态）。`plan_hash` 仅诊断展示，命中判定不走它。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct RefCache {
    pub plan_hash: String,
    pub refs: Vec<RefEntry>,
}

/// 单条引用的四态判定：文件存在性（仓内，与机械审计同口径）+ 行号范围。
/// R8-P73：读失败（非 UTF-8/IO 错）加第四态 unverifiable——不折成
/// line-out-of-range（旧口径 total=0 恒越界，IO 抖动/二进制文件误判）。
fn verdict_for(repo: &Path, path: &str, line: u32) -> &'static str {
    let full = repo.join(path);
    if !full.exists() {
        return "missing";
    }
    let total = match std::fs::read_to_string(&full) {
        Ok(c) => c.lines().count() as u32,
        Err(_) => return "unverifiable",
    };
    if line == 0 || line > total {
        return "line-out-of-range";
    }
    "exists"
}

/// 从计划文本抽全部 `path:line` 并逐条机械核验（顺序与 `path_refs` 一致）。
pub fn extract_entries(plan: &str, repo: &Path) -> Vec<RefEntry> {
    path_refs(plan)
        .into_iter()
        .map(|(path, line)| RefEntry {
            verdict: verdict_for(repo, &path, line).to_string(),
            ref_text: format!("{path}:{line}"),
        })
        .collect()
}

/// 计划整体 hash（诊断展示用；命中判定不依赖它）。FNV 风格 64 位 hex。
pub fn plan_hash_of(plan: &str) -> String {
    let mut h = DefaultHasher::new();
    plan.hash(&mut h);
    format!("{:016x}", h.finish())
}

impl RefCache {
    /// 读缓存：缺文件 / 坏 JSON / 非对象 → `None`（宁降级：全量重核 = 改动前原行为）。
    pub fn load(blackboard: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(blackboard.join(REF_CACHE_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// 原子写（tmp+rename）。写失败只 WARN——缓存缺失只会让下一轮全量重核，
    /// 不值得为它中断 change 场。
    pub fn store(&self, blackboard: &Path) {
        let path = blackboard.join(REF_CACHE_FILE);
        let tmp = blackboard.join(format!("{REF_CACHE_FILE}.tmp"));
        let write = || -> std::io::Result<()> {
            std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default())?;
            std::fs::rename(&tmp, &path)
        };
        if let Err(e) = write() {
            tracing::warn!(
                component = "change-engine",
                event = "plan_ref_cache.store_failed",
                error = %e,
                path = %path.display(),
                "引用核验缓存写盘失败（下一轮将全量重核，不阻断）"
            );
        }
    }
}

/// 缓存比对结果：`hits` = 两轮 verdict 一致（审查官免重核）；`fresh` = 新增或
/// verdict 变化（审查官重点核）。
#[derive(Debug, Default, PartialEq)]
pub struct CacheDiff {
    pub hits: Vec<RefEntry>,
    pub fresh: Vec<RefEntry>,
}

/// ref 级比对：同 ref_text 且 verdict 相同 = 命中；否则 fresh。
pub fn diff(prev: &RefCache, current: &[RefEntry]) -> CacheDiff {
    let prev_map: std::collections::HashMap<&str, &str> = prev
        .refs
        .iter()
        .map(|e| (e.ref_text.as_str(), e.verdict.as_str()))
        .collect();
    let mut d = CacheDiff::default();
    for e in current {
        match prev_map.get(e.ref_text.as_str()) {
            Some(&v) if v == e.verdict => d.hits.push(e.clone()),
            _ => d.fresh.push(e.clone()),
        }
    }
    d
}

/// 渲染进审查指令的「引擎已核」段（放在引擎审计段之后）。
pub fn render_section(d: &CacheDiff) -> String {
    let hits: Vec<&str> = d.hits.iter().map(|e| e.ref_text.as_str()).collect();
    let fresh: Vec<String> = d
        .fresh
        .iter()
        .map(|e| format!("{}（引擎判定：{}）", e.ref_text, e.verdict))
        .collect();
    let mut s = String::from("## 引擎引用核验缓存（确定性，勿重核）\n\n");
    if hits.is_empty() {
        s.push_str("命中：无（首轮或缓存缺失——全部引用按判据 2 自行核实）。\n");
    } else {
        s.push_str(&format!(
            "以下 {} 条引用引擎本轮已机械核验、且与上一轮结论一致，**判据 2 对它们直接采信，\
             不要再用工具重核**（语义相关性仍归你判）：\n{}\n",
            hits.len(),
            hits.join("、")
        ));
    }
    if !fresh.is_empty() {
        s.push_str(&format!(
            "\n以下 {} 条是**新增或结论有变化的**引用，判据 2 必须逐条用工具核实：\n{}\n",
            fresh.len(),
            fresh.join("、")
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "l1\nl2\nl3\n").unwrap();
        dir
    }

    #[test]
    fn verdict_covers_three_states() {
        let r = repo();
        assert_eq!(verdict_for(r.path(), "src/a.rs", 2), "exists");
        assert_eq!(verdict_for(r.path(), "src/nope.rs", 1), "missing");
        assert_eq!(verdict_for(r.path(), "src/a.rs", 4), "line-out-of-range");
        assert_eq!(verdict_for(r.path(), "src/a.rs", 0), "line-out-of-range");
    }

    #[test]
    fn extract_preserves_order_and_text() {
        let r = repo();
        let plan = "影响面：`src/a.rs:2` 与 `src/a.rs:99`，另有 `src/ghost.rs:1`。";
        let es = extract_entries(plan, r.path());
        assert_eq!(es.len(), 3);
        assert_eq!(es[0].ref_text, "src/a.rs:2");
        assert_eq!(es[0].verdict, "exists");
        assert_eq!(es[1].verdict, "line-out-of-range");
        assert_eq!(es[2].verdict, "missing");
    }

    #[test]
    fn store_load_roundtrip_and_corrupt_falls_to_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(RefCache::load(dir.path()).is_none(), "缺文件 = None");
        let c = RefCache {
            plan_hash: plan_hash_of("x"),
            refs: vec![RefEntry {
                ref_text: "src/a.rs:2".into(),
                verdict: "exists".into(),
            }],
        };
        c.store(dir.path());
        assert_eq!(RefCache::load(dir.path()).unwrap(), c);
        std::fs::write(dir.path().join(REF_CACHE_FILE), "{not json").unwrap();
        assert!(RefCache::load(dir.path()).is_none(), "坏 JSON = None 降级");
    }

    #[test]
    fn diff_splits_hits_and_fresh() {
        let prev = RefCache {
            plan_hash: "p".into(),
            refs: vec![
                RefEntry {
                    ref_text: "src/a.rs:2".into(),
                    verdict: "exists".into(),
                },
                RefEntry {
                    ref_text: "src/a.rs:3".into(),
                    verdict: "exists".into(),
                },
                RefEntry {
                    ref_text: "src/old.rs:1".into(),
                    verdict: "missing".into(),
                },
            ],
        };
        let cur = vec![
            RefEntry {
                ref_text: "src/a.rs:2".into(),
                verdict: "exists".into(),
            },
            RefEntry {
                ref_text: "src/a.rs:3".into(),
                verdict: "line-out-of-range".into(),
            },
            RefEntry {
                ref_text: "src/new.rs:1".into(),
                verdict: "exists".into(),
            },
        ];
        let d = diff(&prev, &cur);
        assert_eq!(d.hits.len(), 1, "只有 verdict 一致的 src/a.rs:2 命中");
        assert_eq!(d.hits[0].ref_text, "src/a.rs:2");
        assert_eq!(d.fresh.len(), 2, "verdict 变化 + 新增都算 fresh");
    }

    #[test]
    fn render_section_names_hits_and_fresh() {
        let d = CacheDiff {
            hits: vec![RefEntry {
                ref_text: "src/a.rs:2".into(),
                verdict: "exists".into(),
            }],
            fresh: vec![RefEntry {
                ref_text: "src/new.rs:1".into(),
                verdict: "missing".into(),
            }],
        };
        let s = render_section(&d);
        assert!(s.contains("src/a.rs:2"));
        assert!(s.contains("直接采信"));
        assert!(s.contains("src/new.rs:1"));
        assert!(s.contains("missing"));
    }

    #[test]
    fn render_section_first_round_says_all_manual() {
        let s = render_section(&CacheDiff::default());
        assert!(s.contains("无"));
        assert!(!s.contains("直接采信"));
    }

    /// 验证契约的「两轮对比」证据（组件级）：第一轮全 fresh 落缓存，第二轮
    /// 同计划 diff 全命中——引用复核量从 N 降到 0。
    #[test]
    fn two_round_replay_second_round_all_hits() {
        let r = repo();
        let plan = "影响面：`src/a.rs:1`、`src/a.rs:3`。";
        let first = extract_entries(plan, r.path());
        assert_eq!(first.len(), 2);
        let c1 = RefCache {
            plan_hash: plan_hash_of(plan),
            refs: first.clone(),
        };
        c1.store(r.path()); // 黑板就用临时仓根，load/store 只认路径

        let second = extract_entries(plan, r.path());
        let d = diff(&RefCache::load(r.path()).unwrap(), &second);
        assert!(d.fresh.is_empty(), "计划未变，第二轮不应有 fresh");
        assert_eq!(d.hits.len(), 2, "第二轮全部命中——审查官免重核");
    }
}
