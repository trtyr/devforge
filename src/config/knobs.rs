//! F14 · T205 · 旋钮单点声明（零环境变量配置层）。
//!
//! 目的地（2026-10-30 用户定稿）：**零环境变量**——全部旋钮走
//! `<root>/config.json` + 启动 CLI 参数（CLI 不是环境变量，是配置文件缺失时的
//! 引擎自举锚）；env 读取点全部删除（唯一豁免 `DEVFORGE_MANAGED`：跨进程内部
//! 协议标记，非用户配置）。
//!
//! 优先级一条链：**CLI > config file > 缺省**（CLI 在启动期覆写后即定死；本模块
//! 不读环境变量）。来源标注（default/file/cli）随生效值端点照出。
//!
//! 本模块是全部旋钮的**唯一声明处**：名字 / 类型 / 缺省 / 语义 / 读取点。
//! 读点迁移（ftask-2/3）后，仓内不得再有裸 `env::var("DEVFORGE_`（豁免清单外）。

use std::sync::RwLock;

/// 旋钮值的来源（生效值端点照出，前端据此渲染「这值哪来的」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnobSource {
    /// 内建缺省（声明表里的值）。
    Default,
    /// `<root>/config.json` 覆盖。
    File,
    /// 启动 CLI 参数覆盖（仅启动三件套可走这层）。
    Cli,
}

/// 启动三件套（自举语义：HOME/ADDR/TOKEN——进程早期可读，不走全局旋钮锁）。
#[derive(Debug, Clone, Default)]
pub struct BootstrapOverrides {
    pub home: Option<std::path::PathBuf>,
    pub addr: Option<String>,
    pub token: Option<String>,
}

/// 运行期旋钮集（typed 字段，编译期防拼错；声明表 [`KNOB_TABLE`] 供端点与文档）。
///
/// 口径统一（T205 收编）：
/// - `*_timeout_ms: Option<u64>` —— `None` = 不限（源值 0 映射为 None，「零硬限制哲学」）；
/// - 时长单位一律毫秒；并发一律 usize 且自带 clamp；
/// - `doc_level_change` / `doc_level_docgen` 是**两个旋钮**（T205 收编裁决）：
///   旧 `DEVFORGE_DOC_LEVEL` 一名喂两处、缺省还不同（none vs complete）——那不是
///   同一旋钮的漂移，是两个语义各拿半个名字。拆开后各自缺省归位，漂移消除。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeKnobs {
    /// ask 墙钟（None = 不限）。
    pub ui_ask_timeout_ms: Option<u64>,
    /// ui 待答久等告警阈值。
    pub ui_pending_warn_ms: u64,
    /// review 墙钟（None = 不限）。
    pub review_timeout_ms: Option<u64>,
    /// agent 失活告警阈值。
    pub stall_warn_ms: u64,
    /// run 失活收割阈值（None = 永不因无进展自动杀）。
    pub run_idle_ms: Option<u64>,
    /// run 墙钟（None = 不限）。
    pub run_timeout_ms: Option<u64>,
    /// 审计批次墙钟（None = 不限）。
    pub audit_batch_timeout_ms: Option<u64>,
    /// 审计并发（1..=8）。
    pub audit_concurrency: usize,
    /// 包安装超时（0 = 不限 → None）。
    pub install_timeout_ms: Option<u64>,
    /// change SOP 的文档档位（缺省 none：变更不顺手写文档）。
    pub doc_level_change: String,
    /// doc-from-zero 管线的文档档位（缺省 complete：出文档就是要全的）。
    pub doc_level_docgen: String,
    /// 文档增量开关。
    pub doc_incremental: bool,
    /// 文档增量基线覆盖。
    pub doc_base: Option<String>,
    /// 并发 run 上限（1..=64；0 视为 1）。
    pub max_concurrent: usize,
    /// 今天日期覆盖（YYYY-MM-DD；change_id 用——测试/固定口径注入，零 env 后的承接位）。
    pub today_override: Option<String>,
    /// 回环豁免（A1）：true = 来自 127.0.0.1 的请求免鉴权（本机零摩擦的既有体验）；
    /// false = 本机也必须 session/token（严谨模式）。远程永远不豁免。
    pub auth_loopback_exempt: bool,
}

impl Default for RuntimeKnobs {
    fn default() -> Self {
        Self {
            ui_ask_timeout_ms: None,
            ui_pending_warn_ms: 120_000,
            review_timeout_ms: Some(30 * 60 * 1000),
            stall_warn_ms: 900_000,
            run_idle_ms: None,
            run_timeout_ms: None,
            audit_batch_timeout_ms: Some(30 * 60 * 1000),
            audit_concurrency: 4,
            install_timeout_ms: Some(600_000),
            doc_level_change: "none".to_string(),
            doc_level_docgen: "complete".to_string(),
            doc_incremental: false,
            doc_base: None,
            max_concurrent: 2,
            today_override: None,
            auth_loopback_exempt: true,
        }
    }
}

/// 声明表：全部旋钮的 key / 缺省 / 单位 / 语义（生效值端点与文档的唯一来源）。
///
/// 与 `RuntimeKnobs` 字段一一对应；`key` 即 config.json 里的字段名。
pub const KNOB_TABLE: &[(&str, &str, &str, &str)] = &[
    ("ui_ask_timeout_ms", "不限", "ms", "ask 墙钟（0=不限）"),
    ("ui_pending_warn_ms", "120000", "ms", "ui 待答久等告警阈值"),
    ("review_timeout_ms", "1800000", "ms", "review 墙钟（0=不限）"),
    ("stall_warn_ms", "900000", "ms", "agent 失活告警阈值"),
    ("run_idle_ms", "不限", "ms", "run 失活收割（0=不限）"),
    ("run_timeout_ms", "不限", "ms", "run 墙钟（0=不限）"),
    ("audit_batch_timeout_ms", "1800000", "ms", "审计批次墙钟（0=不限）"),
    ("audit_concurrency", "4", "个", "审计并发（1..=8）"),
    ("install_timeout_ms", "600000", "ms", "包安装超时（0=不限）"),
    ("doc_level_change", "none", "档位", "change SOP 文档档位"),
    ("doc_level_docgen", "complete", "档位", "doc-from-zero 文档档位"),
    ("doc_incremental", "false", "bool", "文档增量开关"),
    ("doc_base", "无", "sha", "文档增量基线覆盖"),
    ("max_concurrent", "2", "个", "并发 run 上限"),
    ("today_override", "无", "YYYY-MM-DD", "今天日期覆盖（测试/固定口径注入）"),
    (
        "auth_loopback_exempt",
        "true",
        "bool",
        "A1 回环豁免（本机免鉴权；远程永不豁免）",
    ),
];

/// 生效值条目（端点形态：key + 值 + 来源）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct KnobEffective {
    pub key: &'static str,
    pub value: serde_json::Value,
    pub source: KnobSource,
    pub unit: &'static str,
    pub doc: &'static str,
}

impl RuntimeKnobs {
    /// 从 config.json 的旋钮段构建（文件缺失 = 全缺省）。
    ///
    /// 解析失败的**单个**旋钮：忽略该键并 WARN（不让一个手滑值炸掉整个启动；
    /// 生效值端点会照出它落在 default——错配可见，不静默）。
    pub fn from_config_json(json: &serde_json::Value) -> (Self, Vec<(&'static str, KnobSource)>) {
        let mut knobs = Self::default();
        let mut sources = Vec::new();
        let Some(obj) = json.as_object() else {
            return (knobs, sources);
        };
        macro_rules! set {
            ($field:ident, $key:literal, $parse:expr) => {
                if let Some(v) = obj.get($key) {
                    match $parse(v) {
                        Some(parsed) => {
                            knobs.$field = parsed;
                            sources.push(($key, KnobSource::File));
                        }
                        None => tracing::warn!(
                            component = "config",
                            event = "config.knob_ignored",
                            key = $key,
                            value = %v,
                            "旋钮值解析失败，回落缺省（生效值端点可见）"
                        ),
                    }
                }
            };
        }
        set!(ui_ask_timeout_ms, "ui_ask_timeout_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        // 0 = 关闭告警（本旋钮的合法语义，与旧 env 口径一致）——不过滤
        set!(ui_pending_warn_ms, "ui_pending_warn_ms", |v: &serde_json::Value| v
            .as_u64());
        set!(review_timeout_ms, "review_timeout_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        set!(stall_warn_ms, "stall_warn_ms", |v: &serde_json::Value| v
            .as_u64()
            .filter(|ms| *ms > 0));
        set!(run_idle_ms, "run_idle_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        set!(run_timeout_ms, "run_timeout_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        set!(audit_batch_timeout_ms, "audit_batch_timeout_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        set!(audit_concurrency, "audit_concurrency", |v: &serde_json::Value| v
            .as_u64()
            .map(|n| n.clamp(1, 8) as usize));
        set!(install_timeout_ms, "install_timeout_ms", |v: &serde_json::Value| v
            .as_u64()
            .map(|ms| if ms == 0 { None } else { Some(ms) }));
        set!(doc_level_change, "doc_level_change", |v: &serde_json::Value| v
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty()));
        set!(doc_level_docgen, "doc_level_docgen", |v: &serde_json::Value| v
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty()));
        set!(doc_incremental, "doc_incremental", |v: &serde_json::Value| v
            .as_bool());
        set!(doc_base, "doc_base", |v: &serde_json::Value| v
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(Some));
        set!(max_concurrent, "max_concurrent", |v: &serde_json::Value| v
            .as_u64()
            .map(|n| n.clamp(1, 64) as usize));
        set!(
            today_override,
            "today_override",
            |v: &serde_json::Value| v
                .as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(Some)
        );
        set!(auth_loopback_exempt, "auth_loopback_exempt", |v: &serde_json::Value| v
            .as_bool());
        (knobs, sources)
    }
}

/// 全局旋钮（进程单例）：启动期 `init` 一次，运行期只读；
/// 测试用 [`with_mut`] 独占改（配合既有串行锁纪律）。
static KNOBS: RwLock<Option<RuntimeKnobs>> = RwLock::new(None);
static KNOB_SOURCES: RwLock<Vec<(&'static str, KnobSource)>> = RwLock::new(Vec::new());

/// 启动期初始化：`config_json` = `<root>/config.json` 的解析结果（None = 文件缺失）。
/// 幂等：重复 init 以最后一次为准（测试可反复用）。
pub fn init(config_json: Option<&serde_json::Value>) {
    let (knobs, sources) = match config_json {
        Some(json) => RuntimeKnobs::from_config_json(json),
        None => (RuntimeKnobs::default(), Vec::new()),
    };
    *KNOBS.write().unwrap_or_else(|e| e.into_inner()) = Some(knobs);
    *KNOB_SOURCES.write().unwrap_or_else(|e| e.into_inner()) = sources;
}

/// 确保已初始化（未 init 时读 = 全缺省；不 panic——旋钮层绝不成为启动顺序的炸点）。
fn snapshot() -> RuntimeKnobs {
    KNOBS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// 测试专用：全局旋钮单例的串行锁——凡 init/with_mut/端点触全局的用例整段持锁
/// （全局是进程单例，两个测试并行 init 互相覆盖 = 假红；本锁让它们串行）。
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 测试/工具：独占改旋钮（写锁内闭包；配合测试串行锁纪律防并行串味）。
pub fn with_mut<R>(f: impl FnOnce(&mut RuntimeKnobs) -> R) -> R {
    let mut guard = KNOBS.write().unwrap_or_else(|e| e.into_inner());
    let knobs = guard.get_or_insert_with(RuntimeKnobs::default);
    f(knobs)
}

/// 生效值清单（全局快照 + 来源表）。
pub fn effective_values() -> Vec<KnobEffective> {
    let knobs = snapshot();
    let sources = KNOB_SOURCES.read().unwrap_or_else(|e| e.into_inner());
    let src_of = |key: &str| knob_source_in(&sources, key);
    vec![
        eff("ui_ask_timeout_ms", knobs.ui_ask_timeout_ms, src_of("ui_ask_timeout_ms")),
        eff("ui_pending_warn_ms", knobs.ui_pending_warn_ms, src_of("ui_pending_warn_ms")),
        eff("review_timeout_ms", knobs.review_timeout_ms, src_of("review_timeout_ms")),
        eff("stall_warn_ms", knobs.stall_warn_ms, src_of("stall_warn_ms")),
        eff("run_idle_ms", knobs.run_idle_ms, src_of("run_idle_ms")),
        eff("run_timeout_ms", knobs.run_timeout_ms, src_of("run_timeout_ms")),
        eff(
            "audit_batch_timeout_ms",
            knobs.audit_batch_timeout_ms,
            src_of("audit_batch_timeout_ms"),
        ),
        eff("audit_concurrency", knobs.audit_concurrency, src_of("audit_concurrency")),
        eff("install_timeout_ms", knobs.install_timeout_ms, src_of("install_timeout_ms")),
        eff("doc_level_change", knobs.doc_level_change, src_of("doc_level_change")),
        eff("doc_level_docgen", knobs.doc_level_docgen, src_of("doc_level_docgen")),
        eff("doc_incremental", knobs.doc_incremental, src_of("doc_incremental")),
        eff("doc_base", knobs.doc_base, src_of("doc_base")),
        eff("max_concurrent", knobs.max_concurrent, src_of("max_concurrent")),
        eff(
            "today_override",
            knobs.today_override,
            src_of("today_override"),
        ),
        eff(
            "auth_loopback_exempt",
            knobs.auth_loopback_exempt,
            src_of("auth_loopback_exempt"),
        ),
    ]
}

fn eff(key: &'static str, value: impl serde::Serialize, source: KnobSource) -> KnobEffective {
    let (_, _default, unit, doc) = KNOB_TABLE
        .iter()
        .find(|(k, _, _, _)| *k == key)
        .copied()
        .unwrap_or((key, "", "", ""));
    KnobEffective {
        key,
        value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        source,
        unit,
        doc,
    }
}

/// 来源表里的单键查询（表外 = Default）。
fn knob_source_in(sources: &[(&'static str, KnobSource)], key: &str) -> KnobSource {
    sources
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| *s)
        .unwrap_or(KnobSource::Default)
}

/// 单键来源查询（全局来源表）。
pub(crate) fn knob_source(key: &str) -> KnobSource {
    let sources = KNOB_SOURCES.read().unwrap_or_else(|e| e.into_inner());
    knob_source_in(&sources, key)
}

/// 并发上限的「显式设置值」：仅 config.json/CLI 覆盖时 Some；缺省返回 None，
/// 消费方（main 启动序 → apply_startup_concurrency）走自己的 store/缺省链——
/// 保持 F13 的「显式值 > store > 缺省」语义，只是显式值的载体从 env 换成了 knobs。
pub fn max_concurrent_set() -> Option<usize> {
    if knob_source("max_concurrent") == KnobSource::Default {
        None
    } else {
        Some(snapshot().max_concurrent)
    }
}

// ---------- typed getters（读点的唯一入口；ftask-2 逐个接线） ----------

/// ask 墙钟（None = 不限）。
pub fn ui_ask_timeout_ms() -> Option<u64> {
    snapshot().ui_ask_timeout_ms
}

/// ui 待答久等告警阈值。
pub fn ui_pending_warn_ms() -> u64 {
    snapshot().ui_pending_warn_ms
}

/// review 墙钟（None = 不限）。
pub fn review_timeout_ms() -> Option<u64> {
    snapshot().review_timeout_ms
}

/// agent 失活告警阈值。
pub fn stall_warn_ms() -> u64 {
    snapshot().stall_warn_ms
}

/// run 失活收割阈值（None = 永不）。
pub fn run_idle_ms() -> Option<u64> {
    snapshot().run_idle_ms
}

/// run 墙钟（None = 不限）。
pub fn run_timeout_ms() -> Option<u64> {
    snapshot().run_timeout_ms
}

/// 审计批次墙钟（None = 不限）。
pub fn audit_batch_timeout_ms() -> Option<u64> {
    snapshot().audit_batch_timeout_ms
}

/// 审计并发（1..=8）。
pub fn audit_concurrency() -> usize {
    snapshot().audit_concurrency
}

/// 包安装超时（None = 不限）。
pub fn install_timeout_ms() -> Option<u64> {
    snapshot().install_timeout_ms
}

/// change SOP 文档档位。
pub fn doc_level_change() -> String {
    snapshot().doc_level_change
}

/// doc-from-zero 文档档位。
pub fn doc_level_docgen() -> String {
    snapshot().doc_level_docgen
}

/// 文档增量开关。
pub fn doc_incremental() -> bool {
    snapshot().doc_incremental
}

/// 文档增量基线覆盖。
pub fn doc_base() -> Option<String> {
    snapshot().doc_base
}

/// 并发 run 上限（0 视为 1 的口径在消费方，与 Store::begin_run 同链）。
pub fn max_concurrent() -> usize {
    snapshot().max_concurrent
}

/// 今天日期覆盖（change_id 用；None = 用本地时钟）。
pub fn today_override() -> Option<String> {
    snapshot().today_override
}

/// 回环豁免开关（A1；缺省 true = 本机免鉴权，config 可收紧）。
pub fn auth_loopback_exempt() -> bool {
    snapshot().auth_loopback_exempt
}

#[cfg(test)]
#[path = "knobs_tests.rs"]
mod tests;
