//! 项目层（P004）：最高层级对象，与本地的关系 = 用户手动指定路径（decisions/001）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Context as _;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// 项目唯一标识（kebab-case，如 `devforge`）。
    pub name: String,
    /// 本地路径——用户手动指定（从 0 创建 = 指定创建位置；已有项目 = 指定路径登记）。
    pub local_path: PathBuf,
    pub created_at_ms: u64,
}

impl Project {
    pub fn new(name: impl Into<String>, local_path: PathBuf, created_at_ms: u64) -> Self {
        Self {
            name: name.into(),
            local_path,
            created_at_ms,
        }
    }

    /// 本地关系落地（P004 T004）：确保项目目录存在。
    /// 从 0 创建场景 = 在用户指定的位置建目录；已有项目目录已存在，幂等。
    pub fn ensure_local_dir(&self) -> crate::error::Result<()> {
        std::fs::create_dir_all(&self.local_path)
            .map_err(crate::error::DevforgeError::from)
            .ctx(format!(
                "创建/确认项目 `{}` 本地目录 {}",
                self.name,
                self.local_path.display()
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_holds_manually_designated_path() {
        let p = Project::new("devforge", PathBuf::from("/home/me/code/devforge"), 1);
        assert_eq!(p.name, "devforge");
        assert_eq!(p.local_path, PathBuf::from("/home/me/code/devforge"));
    }

    #[test]
    fn ensure_local_dir_creates_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("from-zero-project");
        let p = Project::new("fresh", target.clone(), 1);
        p.ensure_local_dir().unwrap();
        assert!(target.is_dir());
        p.ensure_local_dir().unwrap(); // 幂等
        assert!(target.is_dir());
    }
}
