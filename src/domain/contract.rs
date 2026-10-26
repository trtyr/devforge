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

    /// forbidden_paths 命中判定（P012 挂账补全）：绝对禁令，独立于 owned 覆盖。
    pub fn hits_forbidden(&self, path: &str) -> bool {
        self.forbidden_paths.iter().any(|p| path_covered_by(path, p))
    }
}

/// 单条目匹配（`WorkItemContract::covers` 的实现细节）。
fn path_covered_by(path: &str, pattern: &str) -> bool {
    if pattern == "**" {
        // 裸 `**` = 一切路径（gitignore 语义；与插件 match.ts 同步，样例集驱动）
        return true;
    }
    if let Some(sub) = pattern.strip_prefix("**/") {
        // 中间通配（gitignore 语义）：`**/secrets/**` = 任意层级的 secrets 目录下全部
        if sub == "**" {
            return true;
        }
        if let Some(dir) = sub.strip_suffix("/**") {
            return path == dir
                || path.starts_with(&format!("{dir}/"))
                || path.contains(&format!("/{dir}/"));
        }
        if sub.ends_with('/') {
            return path.starts_with(sub) || path.contains(&format!("/{sub}"));
        }
        return path == sub || path.ends_with(&format!("/{sub}"));
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
#[path = "contract_tests.rs"]
mod tests;
