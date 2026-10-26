//! 运行账单取数：从 pi 会话 jsonl 里累加 token 与花费（P008 T021 验收要求「token 落账单」）。
//!
//! 为什么从会话文件读而不问 Agent：Agent 自述的用量不可复算；会话文件是 pi 自己写的原始账。
//! 只认 `message.usage`（每次模型调用的原始计数），字段缺失就当 0，绝不估算。

use std::path::Path;

/// 一个会话目录（含多层子目录）里所有 jsonl 的用量合计。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
    pub cost_usd: f64,
}

impl Usage {
    pub fn render(&self) -> String {
        format!(
            "{} tokens（in {} / out {}），成本 ${:.4}",
            self.total, self.input, self.output, self.cost_usd
        )
    }
}

/// 汇总一个或多个会话根目录（递归扫 `*.jsonl`）。
pub fn from_session_dirs(dirs: &[impl AsRef<Path>]) -> Usage {
    let mut acc = Usage::default();
    for dir in dirs {
        for file in jsonl_files(dir.as_ref()) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(row) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                let Some(usage) = row
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .filter(|u| u.is_object())
                else {
                    continue;
                };
                let num = |k: &str| usage.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                acc.input += num("input");
                acc.output += num("output");
                let total = num("totalTokens");
                acc.total += if total > 0 {
                    total
                } else {
                    num("input") + num("output")
                };
                acc.cost_usd += usage
                    .get("cost")
                    .and_then(|c| c.get("total"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
            }
        }
    }
    acc
}

fn jsonl_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&cur) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
