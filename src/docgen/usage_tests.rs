//! （2026-10-26 测试分离：自 src/docgen/usage.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn sums_usage_across_files_and_ignores_broken_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let day = tmp.path().join("run-1");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("a.jsonl"),
        concat!(
            r#"{"type":"session"}"#,
            "\n",
            r#"{"type":"message","message":{"role":"assistant","usage":{"input":100,"output":20,"totalTokens":120,"cost":{"total":0.5}}}}"#,
            "\n",
            "not json\n",
        ),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("b.jsonl"),
        // 缺 totalTokens：退回 input+output
        r#"{"message":{"usage":{"input":10,"output":5}}}"#,
    )
    .unwrap();
    let u = from_session_dirs(&[tmp.path()]);
    assert_eq!(u.input, 110);
    assert_eq!(u.output, 25);
    assert_eq!(u.total, 135);
    assert!((u.cost_usd - 0.5).abs() < 1e-9);
    assert!(u.render().contains("135 tokens"));
}

#[test]
fn missing_dir_yields_zero() {
    let u = from_session_dirs(&[Path::new("/definitely/not/here")]);
    assert_eq!(u, Usage::default());
}
