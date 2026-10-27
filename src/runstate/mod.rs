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

    /// 三段标识的路径安全校验（自审 minor 94）：`Path::join` 遇绝对路径或 `..` 段会**逃出**
    /// plans_root，而 save 还会 create_dir_all 到那个路径。不变式落在模块自身，不靠 HTTP 层。
    pub fn validate(&self) -> Result<()> {
        for (label, v) in [
            ("project", &self.project),
            ("task", &self.task),
            ("sop", &self.sop),
        ] {
            let ok = !v.is_empty()
                && v != "."
                && v != ".."
                && !v.contains('/')
                && !v.contains('\\')
                && !v.contains('\0')
                && !Path::new(v).is_absolute();
            if !ok {
                return Err(DevforgeError::Store(format!(
                    "状态键的 {label} 段 `{v}` 含非法路径成分（不可为空/./..，不可含分隔符，不可为绝对路径）"
                )));
            }
        }
        Ok(())
    }

    /// 该 SOP 的状态文件绝对路径：`<root>/<project>/<task>/<sop>/state.json`。
    ///
    /// 调用方应先用 [`StateKey::validate`] 校验（load/save/clear 已内置）。
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
    /// 有文件但不可用：按冷启动处理，附带原因便于记日志。
    ///
    /// 自审 minor（95）：三种性质不同的原因（损坏 / 身份不符+schema 过期 / 读失败）不再
    /// 折叠成一个裸字符串——消费方可以按 [`UnusableReason::kind`] 对齐策略，
    /// 而不是把「读不到」和「内容坏了」当成同一件事。
    Unusable(UnusableReason),
}

/// 断点不可用的原因（自审 minor 95/97/180）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusableReason {
    pub kind: UnusableKind,
    /// 人读详情（日志用；消息里必须能看出**真实**原因，不许三种原因同一个文案）。
    pub detail: String,
}

impl std::fmt::Display for UnusableReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

/// 不可用的性质分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnusableKind {
    /// 文件在但读不出来（EACCES/EIO/EMFILE…）——基础设施面，不是断点内容的问题。
    IoError,
    /// 内容不是合法 JSON / 结构不符。
    Corrupt,
    /// 身份不符（project/task/sop 不同，或 schema 版本过期）。
    IdentityMismatch,
}

impl Loaded {
    /// 可续跑的断点（`Missing` 与 `Unusable` 都返回 None）。
    ///
    /// 自审 minor（179）：本访问器会把「没有断点」和「断点坏了」压成同一个 None——
    /// 需要区分时读 [`Loaded::why`]（别在该分支写盘覆盖断点）。
    pub fn state(&self) -> Option<&RunState> {
        match self {
            Loaded::Found(s) => Some(s),
            _ => None,
        }
    }

    /// 没有可用断点时的原因（`Found` 返回 None）。
    pub fn why(&self) -> Option<&UnusableReason> {
        match self {
            Loaded::Unusable(r) => Some(r),
            _ => None,
        }
    }
}

/// 读断点。**任何异常都不报错**——断点坏了不该让 run 起不来。
pub fn load(plans_root: &Path, key: &StateKey) -> Loaded {
    // 自审 minor（94）：键本身不安全时不许拼路径（join 会逃出 plans_root）
    if let Err(e) = key.validate() {
        tracing::error!(
            component = "runstate",
            event = "runstate.invalid_key",
            project = %key.project,
            task = %key.task,
            sop = %key.sop,
            error = %e,
            "断点键含非法路径成分，按无断点处理"
        );
        return Loaded::Unusable(UnusableReason {
            kind: UnusableKind::IdentityMismatch,
            detail: e.to_string(),
        });
    }
    let path = key.path(plans_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(component = "runstate", path = %path.display(), "无断点（冷启动）");
            return Loaded::Missing;
        }
        Err(e) => {
            // 自审 minor（180）：读失败 ≠ 内容损坏——分类留给消费方
            let detail = format!("读断点 {} 失败：{e}", path.display());
            tracing::warn!(component = "runstate", path = %path.display(), error = %e, "断点读取失败，按冷启动");
            return Loaded::Unusable(UnusableReason {
                kind: UnusableKind::IoError,
                detail,
            });
        }
    };
    match serde_json::from_str::<RunState>(&text) {
        Ok(state) if state.matches(key) => {
            tracing::debug!(
                component = "runstate",
                event = "runstate.resumable",
                project = %key.project,
                task = %key.task,
                sop = %key.sop,
                attempt = state.attempt,
                "命中可续跑断点"
            );
            Loaded::Found(state)
        }
        Ok(state) => {
            // 自审 minor（97）：schema 过期不得与「身份不符」共用同一段文案（两边 project/
            // task/sop 完全相同时，运维看到的是一句自相矛盾的诊断）。
            let detail = if state.schema_version != SCHEMA_VERSION {
                format!(
                    "断点 schema 版本过期（文件 v{}，期望 v{}）：{} / {} / {}——按冷启动处理",
                    state.schema_version, SCHEMA_VERSION, key.project, key.task, key.sop
                )
            } else {
                format!(
                    "断点身份不符（文件 {} / {} / {}，请求 {} / {} / {}）——按冷启动处理",
                    state.project, state.task, state.sop, key.project, key.task, key.sop
                )
            };
            tracing::warn!(
                component = "runstate",
                path = %path.display(),
                detail = %detail,
                "断点不可用，按冷启动"
            );
            Loaded::Unusable(UnusableReason {
                kind: UnusableKind::IdentityMismatch,
                detail,
            })
        }
        Err(e) => {
            let detail = format!("断点解析失败（{}）：{e}", path.display());
            tracing::warn!(component = "runstate", path = %path.display(), error = %e, "断点损坏，按冷启动");
            Loaded::Unusable(UnusableReason {
                kind: UnusableKind::Corrupt,
                detail,
            })
        }
    }
}

/// 原子写断点：同目录临时文件 → rename。父目录不存在则建。
pub fn save(plans_root: &Path, key: &StateKey, state: &mut RunState) -> Result<PathBuf> {
    // 自审 minor（94）：键不安全 → 拒绝写盘（否则 create_dir_all + rename 可写任意路径）
    key.validate()?;
    // 自审 minor（181）：旧口径无条件把 schema_version 盖成当前版本——外来版本的断点会被
    // 「洗」成当前版本而不是被拒，与 load/matches 的拒绝口径自相矛盾。
    if state.schema_version != SCHEMA_VERSION && state.schema_version != 0 {
        return Err(DevforgeError::Store(format!(
            "拒绝把 schema v{} 的断点写成 v{SCHEMA_VERSION}（外来版本应先按冷启动重建）",
            state.schema_version
        )));
    }
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
    // 自审 minor（98）：临时名固定 = 多写者并发互相截断/覆盖，rename 可把半截 JSON
    // 发布成正式断点（模块宣称的原子性只在单写者下成立）。按 pid+序号唯一。
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("{STATE_FILE}.{}.{seq}{TMP_SUFFIX}", std::process::id()));
    // 自审 minor（41）：序列化是唯一没挂错误上下文的失败点
    let json = serde_json::to_vec_pretty(state).ctx("序列化运行状态")?;
    let written = (|| -> Result<()> {
        let mut f = std::fs::File::create(&tmp).ctx(format!("建临时状态 {}", tmp.display()))?;
        f.write_all(&json).ctx(format!("写临时状态 {}", tmp.display()))?;
        f.sync_all().ctx(format!("刷盘 {}", tmp.display()))?;
        Ok(())
    })();
    if let Err(e) = written {
        // 自审 minor（182）：写失败要清掉半截临时文件（否则黑板里永久残留）
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &path).ctx(format!("落盘状态 {}", path.display())) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    tracing::debug!(
        component = "runstate",
        event = "runstate.saved",
        path = %path.display(),
        attempt = state.attempt,
        "断点已落盘"
    );
    Ok(path)
}

/// 删除断点（`fresh: true` 与终态清理用）。不存在视为成功。
pub fn clear(plans_root: &Path, key: &StateKey) -> Result<()> {
    key.validate()?;
    let path = key.path(plans_root);
    // 自审 minor（182）：清断点时连带清掉历史临时文件（旧固定名 state.json.tmp 与
    // 新的唯一名都清），否则黑板里永久残留半截文件。
    if let Some(dir) = path.parent() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&format!("{STATE_FILE}.")) && name.ends_with(TMP_SUFFIX) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
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

/// 落盘时间戳（自审 minor 183：不再自建时钟，统一走 `domain::now_ms` 的毫秒口径）。
fn now_ms() -> i64 {
    crate::domain::now_ms() as i64
}

#[cfg(test)]
mod tests;
