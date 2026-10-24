//! 清单（inventory）：反向写文档的「分母」——机械抽取 / 闭包校验 / 单元预分配（D002/D013）。
//! ARC-R6 自单文件垂直切分：data 模型 / support 支持文件扫描 / scanner 主扫+单元分配 /
//! closure 三道闭包 / imports 依赖解析 / tests。外部路径不变（pub use 层）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Context as _, Result};

pub use data::{Closure, ClosureFinding, Coverage, EntryKind, Inventory, InventoryEntry, InventoryMeta, Lang, SOURCE_EXTENSIONS};
pub use support::{config_keys, env_file_keys, is_test_file, make_targets, package_scripts, scan_support_files, sql_objects, test_specs};
pub use scanner::{assign_units, build_inventory, is_entry_file, public_symbols, scan_source_files};
pub use closure::{check_closures, export_glossary};
pub use imports::{resolve_imports};

mod closure;
mod data;
mod imports;
mod scanner;
mod support;

#[cfg(test)]
mod tests;
