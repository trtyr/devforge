//! 往返历史（P009 T013）：每次 coding→plan 回退都落一行到
//! `<台账根>/changes/<change-id>/history.md`，供事后审计「边界是怎么被撑大的」。
//! 2026-10-25 反转 D003：台账不进仓——旧实现拼的是目标仓 `docs/plantree/changes/…`。

use std::path::{Path, PathBuf};

use crate::change::boundary::ImpactGap;
use crate::error::{DevforgeError, Result};

/// 历史文件路径（`change_dir` = 台账根下的计划目录）。
pub fn history_path(change_dir: &Path) -> PathBuf {
    change_dir.join("history.md")
}

/// 追加一次往返记录（文件不存在则带表头新建）。
pub fn append_roundtrip(
    change_dir: &Path,
    change_id: &str,
    roundtrip: usize,
    gaps: &[ImpactGap],
) -> Result<PathBuf> {
    let path = history_path(change_dir);
    let dir = path.parent().expect("history 必有父目录");
    std::fs::create_dir_all(dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", dir.display())))?;

    let mut text = if path.is_file() {
        // 引擎审计修复 #8：读失败必须 Err——unwrap_or_default 会把不可读的旧历史
        // 当空串覆盖写，header 跳过、往返轨迹静默清零（审计链断裂）。
        std::fs::read_to_string(&path).map_err(|e| {
            DevforgeError::System(format!(
                "读变更往返历史 {} 失败：{e}（宁可不写也不静默清空既有轨迹）",
                path.display()
            ))
        })?
    } else {
        format!("# 变更往返历史 · {change_id}\n\n| 往返 | 越界文件 | 理由 | 证据 |\n| --- | --- | --- | --- |\n")
    };
    for g in gaps {
        text.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            roundtrip, g.file, g.reason, g.evidence
        ));
    }
    // 自审 major：truncate+write 非原子——崩溃留下半截文件，既有往返轨迹清零，
    // 与本文件「宁可不写也不静默清空」不变量自相矛盾。唯一临时名 + rename。
    let tmp = path.with_extension(format!("md.{}.tmp", std::process::id()));
    (|| -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| DevforgeError::System(format!("建临时文件失败：{e}")))?;
        f.write_all(text.as_bytes())
            .map_err(|e| DevforgeError::System(format!("写临时文件失败：{e}")))?;
        f.sync_all()
            .map_err(|e| DevforgeError::System(format!("刷临时文件失败：{e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| DevforgeError::System(format!("原子替换 {} 失败：{e}", path.display())))?;
        Ok(())
    })()
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(file: &str) -> ImpactGap {
        ImpactGap {
            file: file.into(),
            reason: "接口在这".into(),
            evidence: format!("{file}:9"),
        }
    }

    #[test]
    fn first_roundtrip_creates_history_with_header() {
        let tmp = tempfile::tempdir().unwrap();
        let path = append_roundtrip(tmp.path(), "change-20261020-x", 1, &[gap("src/extra.rs")])
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# 变更往返历史"), "{text}");
        assert!(text.contains("| 1 |"), "{text}");
        assert!(text.contains("src/extra.rs"), "{text}");
    }

    #[test]
    fn later_roundtrips_append_without_losing_history() {
        let tmp = tempfile::tempdir().unwrap();
        append_roundtrip(tmp.path(), "c1", 1, &[gap("a.rs")]).unwrap();
        let path = append_roundtrip(tmp.path(), "c1", 2, &[gap("b.rs")]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("a.rs") && text.contains("b.rs"), "{text}");
        assert_eq!(text.matches("# 变更往返历史").count(), 1, "表头只一次");
        assert!(text.contains("| 2 |"), "{text}");
    }

    #[test]
    fn multiple_gaps_in_one_roundtrip_all_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let path = append_roundtrip(tmp.path(), "c1", 1, &[gap("a.rs"), gap("b.rs")]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("| 1 |").count(), 2, "{text}");
    }
}
