//! （2026-10-26 测试分离：自 src/server/http.rs 内联平移，纯移动零改动。）
use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn cli_flag_space_form_for_token() {
    assert_eq!(parse_cli_flag(&args(&["--token", "abc"]), "--token"), Some("abc".into()));
}

#[test]
fn cli_flag_eq_form_for_token() {
    assert_eq!(parse_cli_flag(&args(&["--token=xyz"]), "--token"), Some("xyz".into()));
}

#[test]
fn cli_flag_absent_for_token() {
    assert_eq!(parse_cli_flag(&args(&["--port", "1"]), "--token"), None);
    assert_eq!(parse_cli_flag(&[], "--token"), None);
}

#[test]
fn cli_flag_dangling_value_is_none() {
    // `--token` 无后继值 → None（不 panic）
    assert_eq!(parse_cli_flag(&args(&["--token"]), "--token"), None);
}

/// 自审 major 回归：`--token` 紧跟另一个 flag → 不猜值（否则 --port 之类
/// 公开常量被当密钥吞掉）。
#[test]
fn cli_flag_followed_by_another_flag_is_none() {
    assert_eq!(
        parse_cli_flag(&args(&["--token", "--port", "7888"]), "--token"),
        None
    );
}

/// 自审 major 回归：带空白的 token 归一化 trim——否则服务永久 401 无声锁死。
#[test]
fn startup_token_is_trimmed() {
    assert_eq!(
        resolve_startup_token(Some("  abc  ".into()), None),
        Some("abc".into())
    );
    assert_eq!(resolve_startup_token(Some("   ".into()), Some("cfg".into())), Some("cfg".into()));
}

#[test]
fn startup_token_cli_wins_over_config() {
    assert_eq!(
        resolve_startup_token(Some("cli".into()), Some("env".into())),
        Some("cli".into())
    );
}

#[test]
fn startup_token_config_used_when_no_cli() {
    assert_eq!(
        resolve_startup_token(None, Some("cfg".into())),
        Some("cfg".into())
    );
}

#[test]
fn startup_token_empty_is_unset() {
    // 空串/纯空白视为未设置——防空 token 把服务锁死
    assert_eq!(resolve_startup_token(Some("  ".into()), None), None);
    assert_eq!(resolve_startup_token(None, Some("".into())), None);
    assert_eq!(resolve_startup_token(None, None), None);
}

// ── 2026-10-27 自审 http 批回归：token 边界用例（219）──

/// 形似 flag 的下一个参数不得被当成密钥（旧口径 `--token --port` 会把 `--port` 当 token）。
#[test]
fn cli_flag_never_swallows_another_flag() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(parse_cli_flag(&args(&["--token", "--port"]), "--token"), None);
    assert_eq!(parse_cli_flag(&args(&["--port", "8080", "--token"]), "--token"), None);
    assert_eq!(
        parse_cli_flag(&args(&["--token", "s3cret"]), "--token"),
        Some("s3cret".to_string())
    );
    assert_eq!(
        parse_cli_flag(&args(&["--token=s3cret"]), "--token"),
        Some("s3cret".to_string())
    );
    // 首个命中生效
    assert_eq!(
        parse_cli_flag(&args(&["--token", "a", "--token", "b"]), "--token"),
        Some("a".to_string())
    );
}

/// 空白必须归一化（旧口径只判空不归一：`" abc "` 当密钥收下但客户端只发 `abc` → 永久 401）。
#[test]
fn token_is_trimmed_and_blank_falls_back() {
    assert_eq!(
        resolve_startup_token(Some("  abc  ".into()), None),
        Some("abc".to_string())
    );
    // CLI 空串/纯空白 → 回退 config 层
    assert_eq!(
        resolve_startup_token(Some("   ".into()), Some("from-env".into())),
        Some("from-env".to_string())
    );
    assert_eq!(resolve_startup_token(Some(String::new()), None), None);
    // CLI 优先于 config 层
    assert_eq!(
        resolve_startup_token(Some("cli".into()), Some("from-config".into())),
        Some("cli".to_string())
    );
    assert_eq!(resolve_startup_token(None, Some("  ".into())), None);
    // 含不可见字符的 token 会被 trim 掉两端，但中间不可见字符仍原样保留（不猜、不改写）
    assert_eq!(
        resolve_startup_token(Some("a\u{7}b".into()), None),
        Some("a\u{7}b".to_string())
    );
}

/// F14：旗标语法契约的单一来源在 parse_cli_flag——三件套共用一份语法。
#[test]
fn cli_flag_contract_is_generic_across_names() {
    // 同一实现服务 --home / --addr / --token：等号形、空格形、缺值形语义一致
    assert_eq!(parse_cli_flag(&args(&["--home", "/x/d"]), "--home"), Some("/x/d".into()));
    assert_eq!(parse_cli_flag(&args(&["--addr=0.0.0.0:7888"]), "--addr"), Some("0.0.0.0:7888".into()));
    assert_eq!(parse_cli_flag(&args(&["--home", "--addr"]), "--home"), None);
}
