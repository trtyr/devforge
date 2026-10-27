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
    ///
    /// 自审 minor（29）：空路径必须先拒——`create_dir_all("")` 返回 `Ok(())`，
    /// 旧口径会「静默成功但什么也没建」，后续跑 SOP 才在别的层报奇怪的错。
    pub fn ensure_local_dir(&self) -> crate::error::Result<()> {
        if self.local_path.as_os_str().is_empty() {
            return Err(crate::error::DevforgeError::Config(format!(
                "项目 `{}` 的本地路径为空（拒绝 create_dir_all(\"\") 的假成功）",
                self.name
            )));
        }
        std::fs::create_dir_all(&self.local_path)
            .map_err(crate::error::DevforgeError::from)
            .ctx(format!(
                "创建/确认项目 `{}` 本地目录 {}",
                self.name,
                self.local_path.display()
            ))
    }

    /// 对外视图（自审 minor 14）：**不回显主机绝对路径**——项目路径是主机的目录布局，
    /// 与仓库「不回显主机目录布局」的口径一致。只给「是否已登记」与名称/时间。
    pub fn public_view(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "created_at_ms": self.created_at_ms,
            "has_local_path": !self.local_path.as_os_str().is_empty(),
        })
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
