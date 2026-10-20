//! 往返历史（P009 T013）：每次 coding→plan 回退都落一行到
//! `docs/plantree/changes/<change-id>/history.md`，供事后审计「边界是怎么被撑大的」。

use std::path::{Path, PathBuf};

use crate::change::boundary::ImpactGap;
use crate::error::{DevforgeError, Result};

/// 历史文件路径。
pub fn history_path(repo: &Path, change_id: &str) -> PathBuf {
    repo.join("docs/plantree/changes")
        .join(change_id)
        .join("history.md")
}

/// 追加一次往返记录（文件不存在则带表头新建）。
pub fn append_roundtrip(
    repo: &Path,
    change_id: &str,
    roundtrip: usize,
    gaps: &[ImpactGap],
) -> Result<PathBuf> {
    let path = history_path(repo, change_id);
    let dir = path.parent().expect("history 必有父目录");
    std::fs::create_dir_all(dir)
        .map_err(|e| DevforgeError::System(format!("建目录 {} 失败：{e}", dir.display())))?;

    let mut text = if path.is_file() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        format!("# 变更往返历史 · {change_id}\n\n| 往返 | 越界文件 | 理由 | 证据 |\n| --- | --- | --- | --- |\n")
    };
    for g in gaps {
        text.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            roundtrip, g.file, g.reason, g.evidence
        ));
    }
    std::fs::write(&path, text)
        .map_err(|e| DevforgeError::System(format!("写 {} 失败：{e}", path.display())))?;
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
