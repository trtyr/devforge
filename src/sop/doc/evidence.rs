//! doc run 的产出证据（plan 010 D003）：指纹快照 + 单元产出清单。

use std::path::Path;

/// 产出根下所有文件的相对路径与指纹（长度 + mtime 纳秒）——续跑证据的基线快照。
///
/// 用指纹而不是「路径集合」：重做单元时 agent 往往**改写**已有文件，
/// 只看「新出现的文件」会把重写判成没产出，下次续跑又会被踢回重做。
pub fn snapshot_files(root: &Path) -> std::collections::BTreeMap<String, (u64, u128)> {
    let mut out = std::collections::BTreeMap::new();
    fn walk(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, (u64, u128)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(rel) = p.strip_prefix(base) {
                let stamp = std::fs::metadata(&p)
                    .map(|m| {
                        let nanos = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_nanos())
                            .unwrap_or(0);
                        (m.len(), nanos)
                    })
                    .unwrap_or((0, 0));
                out.insert(rel.to_string_lossy().to_string(), stamp);
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// 单元跑完之后新增或改写的文件（相对产出根，排除 `_meta/` 下的元数据）。
pub fn produced_files(
    root: &Path,
    before: &std::collections::BTreeMap<String, (u64, u128)>,
) -> Vec<String> {
    snapshot_files(root)
        .into_iter()
        .filter(|(p, _)| !p.starts_with("_meta/"))
        .filter(|(p, stamp)| before.get(p) != Some(stamp))
        .map(|(p, _)| p)
        .collect()
}
