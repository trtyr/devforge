//! （2026-10-26 测试分离：自 src/logging.rs 内联平移，纯移动零改动。）
use super::*;
use std::io::Write as _;

#[test]
fn log_file_path_convention() {
    let p = log_file_path(Path::new("/tmp/logs"), "engine");
    assert_eq!(p, Path::new("/tmp/logs/engine.log.jsonl"));
}

#[test]
fn open_log_file_creates_dir_and_appends() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b");
    let mut f1 = open_log_file(&nested, "test").expect("应自动建目录");
    writeln!(f1, "first").unwrap();
    let f2 = open_log_file(&nested, "test").expect("二次打开");
    // f2 追加模式：文件仍含 first
    let content = std::fs::read_to_string(nested.join("test.log.jsonl")).unwrap();
    assert_eq!(content, "first\n");
    drop(f2);
}

#[test]
fn jsonl_file_receives_structured_events() {
    let dir = tempfile::tempdir().unwrap();
    let file = open_log_file(dir.path(), "evt").unwrap();

    // 不碰全局 subscriber：局部安装，测「事件 → JSONL 文件」核心行为
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(std::sync::Mutex::new(file))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(component = "engine", run_id = "r-1", "hello devforge");
    });

    let content =
        std::fs::read_to_string(dir.path().join("evt.log.jsonl")).expect("日志文件应存在");
    assert!(content.contains("hello devforge"), "缺消息: {content}");
    assert!(content.contains("engine"), "缺结构化字段: {content}");
    assert!(content.contains("r-1"), "缺 run_id 字段: {content}");
}
