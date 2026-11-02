//! （ARC-R6 自 inventory.rs 平移，纯移动。）

use super::*;
use super::imports::norm;
use super::scanner::is_skipped_dir;

/// 约定面抽取（T002）：源代码里读的环境变量键 → 配置项条目。
pub fn config_keys(lang: Lang, text: &str) -> Vec<(usize, String)> {
    let markers: &[&str] = match lang {
        Lang::Python => &["os.environ.get(", "os.environ[", "os.getenv("],
        Lang::TsJs => &["process.env.", "process.env["],
        Lang::Go => &["os.Getenv(", "os.LookupEnv("],
        Lang::Rust => &["env::var(", "env::var_os("],
        Lang::Other => &[],
    };
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        for name in keys_after(line, markers) {
            out.push((idx + 1, name));
        }
    }
    out
}

/// 环境变量样板文件（.env.example / .env.sample）里的键。
pub fn env_file_keys(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let s = s.strip_prefix("export ").unwrap_or(s);
        if let Some((k, _)) = s.split_once('=') {
            let k = k.trim();
            if !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                out.push((idx + 1, k.to_string()));
            }
        }
    }
    out
}

/// 数据层（T002）：SQL DDL 里的表/视图名。
pub fn sql_objects(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    // R11-P98：跨行 DDL 状态——「create table/view」后表名在下一行
    let mut pending: Option<usize> = None;
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        // R11-P98：上一行是 create table/view 但表名为空 → 本行首 token 即表名
        if let Some(pl) = pending.take() {
            let lower = line.to_ascii_lowercase();
            let name = lower
                .split(|c: char| c.is_whitespace() || c == '(')
                .next()
                .unwrap_or("")
                .trim_matches(|c| c == '"' || c == '`' || c == ';' || c == '\'');
            if !name.is_empty() {
                out.push((pl, name.to_string()));
                continue;
            }
            // 本行仍取不出名（罕见）→ 放弃该条，不无限等
        }
        let lower = line.to_ascii_lowercase();
        let rest = match lower
            .strip_prefix("create table ")
            .or_else(|| lower.strip_prefix("create view "))
            // R11-P98：CREATE OR REPLACE VIEW/TABLE（旧口径全漏）
            .or_else(|| lower.strip_prefix("create or replace view "))
            .or_else(|| lower.strip_prefix("create or replace table "))
            // R11-P98：行尾恰为关键词（无尾随空格，如「create table」单独一行）→ 跨行形态
            .or_else(|| {
                lower == "create table"
                    || lower == "create view"
                    || lower == "create or replace table"
                    || lower == "create or replace view"
            }
            .then_some(""))
        {
            Some(r) => r,
            None => continue,
        };
        let rest = rest.trim_start_matches("if not exists ").trim();
        let name = rest
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or("")
            .trim_matches(|c| c == '"' || c == '`' || c == ';' || c == '\'');
        if !name.is_empty() {
            out.push((idx + 1, name.to_string()));
        } else {
            // R11-P98：行尾无表名（跨行 DDL 形态）→ 名字在下一行
            pending = Some(idx + 1);
        }
    }
    out
}

/// 构建/启动命令（T002）：Makefile 目标。
pub fn make_targets(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.starts_with('\t') || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((head, _)) = line.split_once(':') else {
            continue;
        };
        let name = head.trim();
        if !name.is_empty()
            && !name.contains('=')
            && !name.contains(' ')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            out.push((idx + 1, name.to_string()));
        }
    }
    out
}

/// 构建/启动命令（T002）：package.json 的 scripts 名。
pub fn package_scripts(text: &str) -> Vec<(usize, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        // 自审 major：解析失败不再静默——构建/启动命令条目整体消失且无痕。
        tracing::warn!(
            component = "doc-engine",
            "package.json 解析失败，构建/启动命令条目缺失（清单将无 T002 条目）"
        );
        return Vec::new();
    };
    let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    scripts
        .keys()
        .map(|k| {
            let line = text
                .lines()
                .position(|l| l.contains(&format!("\"{k}\"")))
                .map(|i| i + 1)
                .unwrap_or(1);
            (line, k.clone())
        })
        .collect()
}

/// 从一行里抽标记后面的键（跳过空白与引号/括号，取到下一个引号/边界）。
fn keys_after(line: &str, markers: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for m in markers {
        let mut from = 0;
        while let Some(pos) = line[from..].find(m) {
            let start = from + pos + m.len();
            let mut s = line[start..].trim_start();
            for p in ["[\"", "['", "(\"", "('", "[", "(", "\"", "'"] {
                if let Some(r) = s.strip_prefix(p) {
                    s = r;
                    break;
                }
            }
            let end = s.find(['"', '\'', ')', ']', ',', ';', ' ', '}']).unwrap_or(s.len());
            let name = &s[..end];
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
                && !out.iter().any(|e| e == name)
            {
                out.push(name.to_string());
            }
            from = start;
        }
    }
    out
}

/// 约定面文件（不走源文件扫描的）：Makefile / package.json / env 样板 / SQL。
pub fn scan_support_files(root: &Path) -> Result<Vec<String>> {
    const NAMES: &[&str] = &["Makefile", "makefile", "package.json"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if is_skipped_dir(&name) {
                    continue;
                }
                stack.push(p);
                continue;
            }
            let name_is = NAMES.contains(&name.as_str())
                || name.ends_with(".sql")
                || name.ends_with(".env.example")
                || name.ends_with(".env.sample");
            if name_is
                && let Ok(stripped) = p.strip_prefix(root)
            {
                out.push(norm(stripped));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 测试文件判定：_test 后缀 / tests 目录 / .test./.spec. 中缀。
pub fn is_test_file(rel: &str) -> bool {
    let p = Path::new(rel);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = p
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    rel.split('/').any(|c| c == "tests" || c == "test" || c == "__tests__")
        || stem.ends_with("_test")
        || stem.starts_with("test_")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || name.ends_with("_test.go")
}

/// 测试即可执行规格（T003）：从测试反抽「行为条目」——只取名字与位置，不照拄断言。
///
/// 为什么只取名字：断言是实现细节的镜像，文档要的是「这条行为存在且可验证」，
/// 而它可验证这件事由 `path:line` 自己担保。
pub fn test_specs(lang: Lang, text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut prev_was_rust_attr = false;
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        match lang {
            Lang::Python => {
                if let Some(rest) = line.strip_prefix("async def ").or_else(|| line.strip_prefix("def "))
                    && let Some(name) = rest.split('(').next()
                    && name.starts_with("test")
                {
                    out.push((idx + 1, name.to_string()));
                }
            }
            Lang::TsJs => {
                for kw in ["it(", "test("] {
                    if let Some(rest) = line.strip_prefix(kw)
                        && let Some(q) = rest.trim_start().strip_prefix(['"', '\'', '`'])
                        && let Some(name) = q.split(['"', '\'', '`']).next()
                        && !name.is_empty()
                    {
                        out.push((idx + 1, name.to_string()));
                    }
                }
            }
            Lang::Go => {
                if let Some(rest) = line.strip_prefix("func Test")
                    && let Some(name) = rest.split('(').next()
                {
                    out.push((idx + 1, format!("Test{name}")));
                }
            }
            Lang::Rust => {
                let t = line.trim();
                if t == "#[test]" || t.starts_with("#[test ") {
                    prev_was_rust_attr = true;
                    continue;
                }
                // R11-P96：#[tokio::test]（本仓 165 处）与 #[test] 同权进分母——
                // 旧口径只认紧邻 #[test]，异步测试全部不在分母（分母变小，覆盖
                // 百分比虚高且不报警）。
                if t == "#[tokio::test]" || t.starts_with("#[tokio::test(") {
                    prev_was_rust_attr = true;
                    continue;
                }
                if t.starts_with("#[") {
                    // 其它属性（#[ignore]/#[should_panic]/#[cfg]…）不重置——属性块
                    // 连续（#[ignore] + #[tokio::test] + fn 三行形态），直到 fn 处理完才清
                    continue;
                }
                if prev_was_rust_attr
                    && let Some(rest) = t.strip_prefix("fn ").or_else(|| t.strip_prefix("async fn "))
                    && let Some(name) = rest.split('(').next()
                {
                    out.push((idx + 1, name.to_string()));
                }
                prev_was_rust_attr = false;
            }
            Lang::Other => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{sql_objects, test_specs};

    /// R11-P96：#[tokio::test] 与 #[test] 同权进分母；属性块连续（#[ignore]+
    /// #[tokio::test]+fn 三行形态）不丢抽取；普通属性行不误开 fn 候选。
    #[test]
    fn rust_tests_recognize_tokio_test_and_attribute_blocks() {
        let src = "\
#[test]\n\
fn sync_a() {}\n\
#[tokio::test]\n\
async fn async_b() {}\n\
#[ignore]\n\
#[tokio::test]\n\
async fn ignored_c() {}\n\
#[should_panic]\n\
fn not_a_test_marker() {}\n";
        let got = test_specs(crate::docgen::inventory::Lang::Rust, src);
        let names: Vec<&str> = got.iter().map(|(_, n)| n.as_str()).collect();
        assert!(names.contains(&"sync_a"), "#[test] 进分母：{names:?}");
        assert!(names.contains(&"async_b"), "#[tokio::test] 进分母：{names:?}");
        assert!(
            names.contains(&"ignored_c"),
            "#[ignore]+#[tokio::test] 属性块不丢抽取：{names:?}"
        );
        assert!(
            !names.contains(&"not_a_test_marker"),
            "非测试属性行不得误开 fn 候选：{names:?}"
        );
    }

    /// R11-P98：CREATE OR REPLACE VIEW（旧口径全漏）与跨行 DDL（表名在下一行）。
    #[test]
    fn sql_objects_covers_or_replace_and_multiline_ddl() {
        let src = "\
create table users (id int);\n\
CREATE OR REPLACE VIEW active_users AS SELECT 1;\n\
create table\n\
  sessions (id int);\n";
        let got = sql_objects(src);
        let names: Vec<&str> = got.iter().map(|(_, n)| n.as_str()).collect();
        assert!(names.contains(&"users"), "基线单行不回潮：{names:?}");
        assert!(
            names.contains(&"active_users"),
            "CREATE OR REPLACE VIEW 应被认出：{names:?}"
        );
        assert!(names.contains(&"sessions"), "跨行 DDL 表名应被认出：{names:?}");
    }
}
