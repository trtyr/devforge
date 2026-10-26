//! （2026-10-26 测试分离：自 src/observability/mod.rs 内联平移，纯移动零改动。）
use std::cell::RefCell;
use std::sync::Once;

thread_local! {
    /// 本线程的日志缓冲（JSONL 文本）。
    static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 写进本线程缓冲的 writer（全局订阅器的落盘口）。
struct ThreadBufferWriter;

impl std::io::Write for ThreadBufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        BUFFER.with(|b| b.borrow_mut().push_str(&String::from_utf8_lossy(buf)));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadBufferWriter {
    type Writer = ThreadBufferWriter;
    fn make_writer(&'a self) -> Self::Writer {
        ThreadBufferWriter
    }
}

/// 装全局订阅器（进程内一次；callsite 恒为 interested → 不会因别的用例先跑而变死）。
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(ThreadBufferWriter)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// 跑一段闭包并返回**本线程**的 JSONL 日志全文。
pub fn capture_jsonl(f: impl FnOnce()) -> String {
    install();
    BUFFER.with(|b| b.borrow_mut().clear());
    f();
    BUFFER.with(|b| b.borrow().clone())
}

/// 断言一条**里程碑**条目的形状：级别 INFO + 带 `component` 字段。
///
/// 计划 §5：「给定全部里程碑条目 → 级别为 **INFO**、字段含 `component` 与 `event`」。
/// 错实现形态（任一命中即红）：①里程碑落成 WARN/ERROR（级别不是 INFO）
/// ②`component` 字段被删（事后按组件过滤就废了）。
///
/// 只钉**形状**，不钉 `component` 的取值（组件名由各模块自选，契约是「有这个字段」）。
pub fn assert_milestone_shape(line: &str) {
    assert!(
        line.contains("\"level\":\"INFO\""),
        "里程碑的级别必须是 INFO：{line}"
    );
    assert!(
        line.contains("\"component\":\""),
        "里程碑要带 component 字段：{line}"
    );
}
