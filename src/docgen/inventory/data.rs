//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;

/// 参与清单的源文件扩展名（D004：多语言；未覆盖语言登记为缺口）。
pub const SOURCE_EXTENSIONS: &[(&str, Lang)] = &[
    ("py", Lang::Python),
    ("ts", Lang::TsJs),
    ("tsx", Lang::TsJs),
    ("js", Lang::TsJs),
    ("jsx", Lang::TsJs),
    ("mjs", Lang::TsJs),
    ("go", Lang::Go),
    ("rs", Lang::Rust),
    ("java", Lang::Other),
    ("cs", Lang::Other),
    ("php", Lang::Other),
    ("rb", Lang::Other),
    ("c", Lang::Other),
    ("h", Lang::Other),
    ("cc", Lang::Other),
    ("cpp", Lang::Other),
    ("hpp", Lang::Other),
    ("kt", Lang::Other),
    ("swift", Lang::Other),
    ("scala", Lang::Other),
    ("dart", Lang::Other),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Python,
    TsJs,
    Go,
    Rust,
    /// 已知扩展名但不做符号抽取（D004：登记为缺口）
    Other,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        SOURCE_EXTENSIONS
            .iter()
            .find(|(e, _)| *e == ext)
            .map(|(_, l)| *l)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Entry,
    Module,
    Symbol,
    Config,
    Data,
    Behavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Coverage {
    Uncovered,
    Covered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub id: String,
    pub kind: EntryKind,
    /// 相对仓库根的路径（统一 `/` 分隔，便于跨机比对）
    pub path: String,
    /// 1-based 行号；module 条目为 1
    #[serde(default = "one")]
    pub line: usize,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub status: Coverage,
    /// 门 0 预分配的单元号（D020），空 = 未分组
    #[serde(default)]
    pub unit_id: String,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryMeta {
    pub root: String,
    pub codegraph_indexed: bool,
    /// 未支持符号抽取的语言（D004 缺口登记）
    pub unsupported_languages: Vec<String>,
    /// 单元编号序列水位（预分配不漂移）
    pub unit_seq: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub meta: InventoryMeta,
    pub entries: Vec<InventoryEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closure {
    EntryReachability,
    FileCoverage,
    SymbolCoverage,
}

impl Closure {
    pub fn as_str(self) -> &'static str {
        match self {
            Closure::EntryReachability => "entry-reachability",
            Closure::FileCoverage => "file-coverage",
            Closure::SymbolCoverage => "symbol-coverage",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClosureFinding {
    pub closure: Closure,
    pub path: String,
    pub line: usize,
    pub message: String,
}

impl Inventory {
    pub fn entry_paths(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Module)
            .map(|e| e.path.clone())
            .collect()
    }

    pub fn symbol_names(&self) -> BTreeSet<(String, String)> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symbol)
            .map(|e| (e.path.clone(), e.canonical_name.clone()))
            .collect()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx(format!("建目录 {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).ctx(format!("写清单 {}", path.display()))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).ctx(format!("读清单 {}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    }
}
