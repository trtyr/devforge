//! F10/F13 真修（2026-11-01）：跨场守卫常量物化到运行时状态文件。
//!
//! 病根（plantree ideas/selfhost-findings F10/F13）：守卫的基线/白名单烙死在测试
//! 源码里，每场 step 1 刷新即提交 → 守卫文件随场增殖，两场之间任何非 tests 提交
//! 把它们全体染红，只能靠 `#[ignore]` 压住（每场发车前缴税）。
//!
//! 真修：常量 → `tests/.guard-state.json`（**gitignore**，场内 step 1 生成，
//! 两场之间自然消失）——守卫读到文件就对账，读不到就跳过（不红）。
//! 源码里不再有跨场常量，随场增殖与染红同时消失。
//!
//! 状态文件形状（场内 step 1 写）：
//!
//! ```json
//! {
//!   "change_id": "change-2026xxxx-...",
//!   "baseline": "<本场基线 sha>",
//!   "whitelist": ["src/...", "tests/..."]
//! }
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// 守卫状态文件的仓内路径（gitignore，不进提交）。
pub const GUARD_STATE_PATH: &str = "tests/.guard-state.json";

/// 一场变更的守卫对账输入（物化后的「常量」）。
#[derive(Debug, Clone, Deserialize)]
pub struct GuardState {
    /// 本场 change-id（步提交按 subject 含它过滤——F10 收敛口径）。
    pub change_id: String,
    /// 本场基线 sha（扫描区间的起点）。
    pub baseline: String,
    /// 本场 coding 步白名单。
    pub whitelist: Vec<String>,
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 从指定仓根读守卫状态：文件缺失 → `None`（两场之间，守卫跳过）；
/// 文件损坏 → panic（配置错误必须可观测，静默跳过 = 守卫假死）。
pub fn load_from(repo_root: &Path) -> Option<GuardState> {
    let path = repo_root.join(GUARD_STATE_PATH);
    let text = std::fs::read_to_string(&path).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "守卫状态文件损坏（{}）：{e}——重写或删除它（删除 = 本场守卫跳过）",
            path.display()
        )
    }))
}

/// 从本仓根读守卫状态（守卫测试的入口）。
pub fn load() -> Option<GuardState> {
    load_from(&repo_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_none_broken_file_panics_good_file_parses() {
        let tmp = tempfile::tempdir().unwrap();

        // 缺失 → None（两场之间，守卫跳过不红）
        assert!(load_from(tmp.path()).is_none());

        // 损坏 → panic（可观测，不许静默假死）
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join(GUARD_STATE_PATH), "{ not json").unwrap();
        let panicked = std::panic::catch_unwind(|| load_from(tmp.path())).is_err();
        assert!(panicked, "损坏的状态文件必须炸出来");

        // 好文件 → 解析齐全
        std::fs::write(
            tmp.path().join(GUARD_STATE_PATH),
            r#"{"change_id":"change-x","baseline":"abc123","whitelist":["src/a.rs"]}"#,
        )
        .unwrap();
        let state = load_from(tmp.path()).expect("好文件应解析");
        assert_eq!(state.change_id, "change-x");
        assert_eq!(state.baseline, "abc123");
        assert_eq!(state.whitelist, vec!["src/a.rs".to_string()]);
    }
}
