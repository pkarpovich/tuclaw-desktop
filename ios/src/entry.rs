use gpui::{App, AppContext, WindowOptions};
use gpui_kit::base::Root;
use gpui_mobile::ios::ffi;

use crate::smoke::Smoke;

struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static LOGGER: Stderr = Stderr;

#[unsafe(no_mangle)]
pub extern "C" fn tuclaw_ios_start() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    ffi::set_app_callback(Box::new(|cx: &mut App| {
        gpui_kit::init(cx);
        let opened = cx.open_window(WindowOptions::default(), |window, cx| {
            let smoke = cx.new(|cx| Smoke::new(window, cx));
            cx.new(|cx| Root::new(smoke, window, cx))
        });
        opened.expect("failed to open window");
        cx.activate(true);
    }));
    ffi::run_app();
}
