//! The warnings the code logs, for a test to assert a failure isn't dropped silently: a logger
//! installed once for the test process keeps each thread's warnings and errors, so tests running
//! side by side don't see each other's (a `#[tokio::test]` runs its tasks on its one thread).

use std::cell::RefCell;
use std::sync::Once;

thread_local! {
    static LOGGED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            LOGGED.with(|l| l.borrow_mut().push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

/// The warnings and errors logged on this thread since the last call, oldest first. The first call
/// installs the logger: a test calls it once before the code under test, then to read.
pub fn take_warnings() -> Vec<String> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&Capture).expect("no other logger in the unit tests");
        log::set_max_level(log::LevelFilter::Warn);
    });
    LOGGED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
