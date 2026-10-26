//! 运行状态（plan 010 · T001）：断点真源。
//!
//! 口径（010 decisions）：
//! - **落点**：黑板内 `<plans_root>/<project>/<task>/<sop>/state.json` —— 按 project / task / SOP
//!   三级分文件，同一 task 下 doc 与 change 各有一份，互不覆盖。
//! - **真源唯一**：产物目录（如 `docs/contexts/_meta/`）不再存放运行状态。
//! - **身份自证**：文件内 `schema_version` / `sop` / `project` / `task` 必须与请求一致，
//!   不一致按冷启动处理（绝不猜测——跑错比白跑贵）。
//! - **原子写**：先写同目录临时文件再 rename，中途断电不会留下半个 JSON。
//! - **容错**：文件缺失 / 解析失败 / 身份不符 → 视为无断点，由调用方走冷启动并记日志。
//!
//! 本模块只认「状态怎么存」；「什么算做完」「要重做哪些单元」归各 SOP（T002）。

use std::io::Write as _;
use std::path::{Path, PathBuf};

pub mod resume;

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, DevforgeError, Result};

/// 状态文件名（每个 SOP 目录一份）。
pub const STATE_FILE: &str = "state.json";
/// 当前 schema 版本；结构不兼容时递增，旧版本按冷启动处理。
pub const SCHEMA_VERSION: u32 = 1;
/// 临时文件后缀（rename 原子替换用）。
const TMP_SUFFIX: &str = ".tmp";

/// 状态的身份四元组：谁、哪个任务、哪个 SOP。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateKey {
    pub project: String,
    pub task: String,
    pub sop: String,
}

impl StateKey {
    pub fn new(project: impl Into<String>, task: impl Into<String>, sop: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            task: task.into(),
            sop: sop.into(),
        }
    }

    /// 该 SOP 的状态文件绝对路径：`<root>/<project>/<task>/<sop>/state.json`。
    pub fn path(&self, plans_root: &Path) -> PathBuf {
        plans_root
            .join(&self.project)
            .join(&self.task)
            .join(&self.sop)
            .join(STATE_FILE)
    }
}

/// 运行状态：公共壳 + SOP 扩展段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub schema_version: u32,
    pub sop: String,
    pub project: String,
    pub task: String,
    /// 最近一次落盘时间（毫秒 epoch）。
    #[serde(default)]
    pub updated_at_ms: i64,
    /// 第几次尝试（基础设施故障自动重启时递增，T005）。
    #[serde(default)]
    pub attempt: u32,
    /// SOP 自己的扩展段：引擎不解释其中任何字段。
    #[serde(default)]
    pub data: serde_json::Value,
}

impl RunState {
    pub fn new(key: &StateKey, data: serde_json::Value) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sop: key.sop.clone(),
            project: key.project.clone(),
            task: key.task.clone(),
            updated_at_ms: now_ms(),
            attempt: 0,
            data,
        }
    }

    /// 身份是否与请求一致（含 schema 版本）。
    pub fn matches(&self, key: &StateKey) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.sop == key.sop
            && self.project == key.project
            && self.task == key.task
    }
}

/// 读取结果——调用方据此决定续跑还是冷启动。
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    /// 有可续跑的断点。
    Found(RunState),
    /// 没有断点（文件不存在）。
    Missing,
    /// 有文件但不可用（损坏 / 身份不符 / schema 过期）：按冷启动处理，附带原因便于记日志。
    Unusable(String),
}

impl Loaded {
    pub fn state(&self) -> Option<&RunState> {
        match self {
            Loaded::Found(s) => Some(s),
            _ => None,
        }
    }
}

/// 读断点。**任何异常都不报错**——断点坏了不该让 run 起不来。
pub fn load(plans_root: &Path, key: &StateKey) -> Loaded {
    let path = key.path(plans_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(e) => return Loaded::Unusable(format!("读 {} 失败：{e}", path.display())),
    };
    match serde_json::from_str::<RunState>(&text) {
        Ok(state) if state.matches(key) => Loaded::Found(state),
        Ok(state) => Loaded::Unusable(format!(
            "断点身份不符（文件 {} / {} / {}，请求 {} / {} / {}）——按冷启动处理",
            state.project, state.task, state.sop, key.project, key.task, key.sop
        )),
        Err(e) => Loaded::Unusable(format!("断点解析失败（{}）：{e}", path.display())),
    }
}

/// 原子写断点：同目录临时文件 → rename。父目录不存在则建。
pub fn save(plans_root: &Path, key: &StateKey, state: &mut RunState) -> Result<PathBuf> {
    // 身份以 key 为准，防止调用方把 A 的状态写到 B 的路径上
    state.schema_version = SCHEMA_VERSION;
    state.project.clone_from(&key.project);
    state.task.clone_from(&key.task);
    state.sop.clone_from(&key.sop);
    state.updated_at_ms = now_ms();

    let path = key.path(plans_root);
    let dir = path.parent().ok_or_else(|| {
        DevforgeError::System(format!("状态路径无父目录：{}", path.display()))
    })?;
    std::fs::create_dir_all(dir).ctx(format!("建状态目录 {}", dir.display()))?;
    let tmp = dir.join(format!("{STATE_FILE}{TMP_SUFFIX}"));
    let json = serde_json::to_vec_pretty(state)?;
    {
        let mut f = std::fs::File::create(&tmp).ctx(format!("建临时状态 {}", tmp.display()))?;
        f.write_all(&json).ctx(format!("写临时状态 {}", tmp.display()))?;
        f.sync_all().ctx(format!("刷盘 {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path).ctx(format!("落盘状态 {}", path.display()))?;
    Ok(path)
}

/// 删除断点（`fresh: true` 与终态清理用）。不存在视为成功。
pub fn clear(plans_root: &Path, key: &StateKey) -> Result<()> {
    let path = key.path(plans_root);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(DevforgeError::System(format!(
            "删除断点 {} 失败：{e}",
            path.display()
        ))),
    }
}

/// 一个 SOP 的状态存取句柄：绑定 `<plans_root>/<project>/<task>/<sop>/state.json`。
///
/// 引擎与 SOP 都只通过它读写断点，不自己拼路径。
#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
    key: StateKey,
}

impl StateStore {
    pub fn new(plans_root: impl Into<PathBuf>, key: StateKey) -> Self {
        Self {
            root: plans_root.into(),
            key,
        }
    }

    pub fn key(&self) -> &StateKey {
        &self.key
    }

    pub fn path(&self) -> PathBuf {
        self.key.path(&self.root)
    }

    /// 读断点（异常不报错，由调用方按 `Loaded` 决定冷暖启动）。
    pub fn load(&self) -> Loaded {
        load(&self.root, &self.key)
    }

    /// 原子写入断点。
    pub fn save(&self, state: &mut RunState) -> Result<PathBuf> {
        save(&self.root, &self.key, state)
    }

    /// 删除断点（fresh / 终态清理）。
    pub fn clear(&self) -> Result<()> {
        clear(&self.root, &self.key)
    }

    /// 空状态（冷启动用；`data` 为对象，便于 SOP 直接写扩展段）。
    pub fn fresh_state(&self) -> RunState {
        RunState::new(&self.key, serde_json::json!({}))
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
