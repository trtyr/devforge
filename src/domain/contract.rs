//! 工作项契约（P012 M0 · T0.1）：mutation 工作项自带的机器可校验边界声明。
//!
//! 设计文档：`docs/plantree/plans/012-parallel-dev-base/topics/01-work-item-contract.md`。
//! M0 范围：契约字段落库（Task 元数据）+ 引擎终校验（越界即 FAIL）；冲突预检与调度器
//! 拒绝（owned_paths 交集体）属 M4。

use serde::{Deserialize, Serialize};

/// 隔离模式（T0.4）：`none`（分支直改，现状）/ `worktree`（M1：独立 git worktree 全生命周期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// 当前分支直改（现状固化，向后兼容的枚举起点）。
    None,
    /// 独立 worktree（M1）：create → spawn cwd = worktree → freeze 候选 → remove（分支保留）。
    Worktree,
}

/// 工作项契约：task 创建时提交，不可变（改契约 = 新工作项）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItemContract {
    /// 执行期实际改动必须 ⊆ 于此（引擎终校验，越界 = FAIL + boundary.violation 事件）。
    /// 条目形态：精确路径（`src/a.rs`）、目录前缀（`src/config/`）、递归 glob（`docs/**`）。
    pub owned_paths: Vec<String>,
    /// 读不受限（readable 默认全读）；写绝对禁止的路径（M0 占位，校验在 M1 随 worktree 落地）。
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
    /// worktree pin 点（M1 使用；M0 记录不校验）。
    #[serde(default)]
    pub base_rev: Option<String>,
    /// 隔离模式（T0.4）。
    pub isolation: Isolation,
}

impl WorkItemContract {
    /// 路径是否被条目覆盖：精确相等 / 目录前缀（条目以 `/` 结尾）/ `/**` 递归 glob。
    /// 最小实现：不做完整 glob 语义，`**` 仅支持「条目尾部 `/**` = 前缀下全部」形态。
    pub fn covers(&self, path: &str) -> bool {
        self.owned_paths.iter().any(|pat| path_covered_by(path, pat))
    }
}

/// 单条目匹配（`WorkItemContract::covers` 的实现细节）。
fn path_covered_by(path: &str, pattern: &str) -> bool {
    if pattern == "**" {
        // 裸 `**` = 一切路径（gitignore 语义；与插件 match.ts 同步，样例集驱动）
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        // `src/**`：src 自身或其子路径
        path == prefix || path.starts_with(&format!("{prefix}/"))
    } else if pattern.ends_with('/') {
        // `src/config/`：目录前缀
        path.starts_with(pattern)
    } else {
        path == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(paths: &[&str]) -> WorkItemContract {
        WorkItemContract {
            owned_paths: paths.iter().map(|s| s.to_string()).collect(),
            forbidden_paths: vec![],
            base_rev: None,
            isolation: Isolation::None,
        }
    }

    #[test]
    fn exact_path_matches_only_itself() {
        let c = contract(&["src/a.rs"]);
        assert!(c.covers("src/a.rs"));
        assert!(!c.covers("src/a.rs.bak"));
        assert!(!c.covers("src/b.rs"));
    }

    #[test]
    fn dir_prefix_matches_subtree() {
        let c = contract(&["src/config/"]);
        assert!(c.covers("src/config/render.rs"));
        assert!(!c.covers("src/configure/a.rs"));
        assert!(!c.covers("src/a.rs"));
    }

    #[test]
    fn globstar_matches_prefix_tree() {
        let c = contract(&["docs/**"]);
        assert!(c.covers("docs/a.md"));
        assert!(c.covers("docs/x/y.md"));
        assert!(!c.covers("docsx/a.md"));
        assert!(!c.covers("src/a.rs"));
    }

    #[test]
    fn any_entry_grants_coverage() {
        let c = contract(&["src/a.rs", "src/config/"]);
        assert!(c.covers("src/config/render.rs"));
        assert!(c.covers("src/a.rs"));
        assert!(!c.covers("src/b.rs"));
    }
    // ---------- 双端样例集同判（P012 topic 08：与 path-guard 插件 match.ts 共用） ----------

    /// 样例集 = assets/extensions/path-guard/test-cases.json（插件 match.ts 的 node 测试
    /// 用同一份）。修改 glob 语义必须让这份样例集在两端全绿。
    #[test]
    fn sample_cases_agree_with_plugin_match_semantics() {
        let raw = include_str!("../../assets/extensions/path-guard/test-cases.json");
        let cases: Vec<(String, String, bool)> =
            serde_json::from_str(raw).expect("样例集 JSON 合法");
        assert!(cases.len() >= 10, "样例集不应缩水：{}", cases.len());
        for (path, pattern, expected) in cases {
            // 单条目判定：path_covered_by 是私有 fn，经 covers 单条目化验证
            let single = contract(&[&pattern]);
            assert_eq!(
                single.covers(&path),
                expected,
                "样例 ({path:?}, {pattern:?}) 双端语义不一致"
            );
        }
    }

}
