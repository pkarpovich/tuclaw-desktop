use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, AppContext, Application, ApplicationHandle, WindowOptions};
use gpui_kit::base::Root;
use gpui_mobile::ios::IosPlatform;
use gpui_mobile::ios::ffi;
use tuclaw_desktop::failure::{self, Startup};
use tuclaw_desktop::icon::Icons;
use tuclaw_desktop::link::Config;

use crate::phone::Phone;

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

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

#[unsafe(no_mangle)]
pub extern "C" fn tuclaw_ios_start() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    ffi::gpui_ios_initialize();
    let startup = failure::start(Config::from_env());
    let platform = Rc::new(IosPlatform::new());
    let application = Application::with_platform(platform)
        .with_assets(Icons)
        .run_embedded(move |cx: &mut App| open(startup, cx));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(application));
    ffi::gpui_ios_did_finish_launching(std::ptr::null_mut());
}

fn open(startup: Startup, cx: &mut App) {
    gpui_kit::init(cx);
    let opened = match startup {
        Startup::Ready(state) => {
            let state = cx.new(|_| *state);
            state.update(cx, |state, cx| state.start(cx));
            cx.open_window(WindowOptions::default(), |window, cx| {
                let phone = cx.new(|cx| Phone::new(state, window, cx));
                cx.new(|cx| Root::new(phone, window, cx))
            })
            .map(|_| ())
        }
        Startup::Failed(view) => cx
            .open_window(WindowOptions::default(), |_, cx| cx.new(|_| view))
            .map(|_| ()),
    };
    opened.expect("failed to open window");
    cx.activate(true);
}
