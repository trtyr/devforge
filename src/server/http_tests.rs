//! （2026-10-26 测试分离：自 src/server/http.rs 内联平移，纯移动零改动。）
use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn token_flag_space_form() {
    assert_eq!(parse_token_flag(&args(&["--token", "abc"])), Some("abc".into()));
}

#[test]
fn token_flag_eq_form() {
    assert_eq!(parse_token_flag(&args(&["--token=xyz"])), Some("xyz".into()));
}

#[test]
fn token_flag_absent() {
    assert_eq!(parse_token_flag(&args(&["--port", "1"])), None);
    assert_eq!(parse_token_flag(&[]), None);
}

#[test]
fn token_flag_dangling_value_is_none() {
    // `--token` 无后继值 → None（不 panic）
    assert_eq!(parse_token_flag(&args(&["--token"])), None);
}

/// 自审 major 回归：`--token` 紧跟另一个 flag → 不猜值（否则 --port 之类
/// 公开常量被当密钥吞掉）。
#[test]
fn token_flag_followed_by_another_flag_is_none() {
    assert_eq!(
        parse_token_flag(&args(&["--token", "--port", "7888"])),
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
    assert_eq!(resolve_startup_token(Some("   ".into()), Some("env".into())), Some("env".into()));
}

#[test]
fn startup_token_cli_wins_over_env() {
    assert_eq!(
        resolve_startup_token(Some("cli".into()), Some("env".into())),
        Some("cli".into())
    );
}

#[test]
fn startup_token_env_used_when_no_cli() {
    assert_eq!(
        resolve_startup_token(None, Some("env".into())),
        Some("env".into())
    );
}

#[test]
fn startup_token_empty_is_unset() {
    // 空串/纯空白视为未设置——防空 token 把服务锁死
    assert_eq!(resolve_startup_token(Some("  ".into()), None), None);
    assert_eq!(resolve_startup_token(None, Some("".into())), None);
    assert_eq!(resolve_startup_token(None, None), None);
}
