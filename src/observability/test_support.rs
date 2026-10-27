//! （2026-10-26 测试分离：自 src/observability/mod.rs 内联平移，纯移动零改动。）
use std::cell::RefCell;
use std::sync::Once;

thread_local! {
    /// 本线程的日志缓冲（JSONL 文本）。
    static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 进程级兜底缓冲（自审 minor 170）：里程碑常常从 **worker 线程**发出，旧口径只按线程
/// 分流 → 仓内捕获器看不见（集成测试侧却是进程级缓冲）。现在两处都写：线程缓冲给
/// 「本线程日志面」断言，进程缓冲给「跨线程里程碑」断言。
static PROC_BUFFER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn proc_push(text: &str) {
    let mut guard = PROC_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(String::new).push_str(text);
}

fn proc_take_and_clear() -> String {
    let mut guard = PROC_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    guard.take().unwrap_or_default()
}

/// 写进本线程缓冲的 writer（全局订阅器的落盘口）。
struct ThreadBufferWriter;

impl std::io::Write for ThreadBufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        BUFFER.with(|b| b.borrow_mut().push_str(&text));
        proc_push(&text);
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
        // 自审 minor（169）：吞掉 set_global_default 的失败 = 捕获器静默退化成空缓冲，
        // 失败表现为「莫名其妙的红测」。这里必须显式报出来（已装过则允许——那是幂等）。
        if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
            panic!("测试日志捕获器装不上（已有别的全局订阅器在位？）：{e}");
        }
    });
}

/// 跑一段闭包并返回**本线程**的 JSONL 日志全文。
pub fn capture_jsonl(f: impl FnOnce()) -> String {
    install();
    BUFFER.with(|b| b.borrow_mut().clear());
    let _ = proc_take_and_clear();
    f();
    BUFFER.with(|b| b.borrow().clone())
}

/// 跑一段闭包并返回**进程级**（含 worker 线程）的 JSONL 日志全文。
///
/// 自审 minor（170）：里程碑从 worker 线程发出时本线程缓冲为空——
/// 「日志面」断言必须能用这个口径，而不是把跨线程事件当成「没记」。
pub fn capture_jsonl_process_wide(f: impl FnOnce()) -> String {
    install();
    let _ = proc_take_and_clear();
    f();
    proc_take_and_clear()
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
