use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_kit::base::Root;

use crate::failure::{self, Startup};
use crate::link::Config;
use crate::shell::{self, Shell};
use crate::{composer, icon, menu, notify};

pub fn run() {
    let startup = failure::start(Config::from_env());
    gpui_platform::application()
        .with_assets(icon::Icons)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            menu::install(cx);
            composer::bind_keys(cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(shell::traffic_light_position()),
                }),
                ..Default::default()
            };
            let opened = match startup {
                Startup::Ready(state) => {
                    let state = cx.new(|_| *state);
                    state.update(cx, |state, cx| state.start(cx));
                    notify::attach(&state, notify::system(), cx);
                    cx.open_window(options, |window, cx| {
                        let shell = cx.new(|cx| Shell::new(state, window, cx));
                        cx.new(|cx| Root::new(shell, window, cx))
                    })
                    .map(|_| ())
                }
                Startup::Failed(view) => cx
                    .open_window(options, |_, cx| cx.new(|_| view))
                    .map(|_| ()),
            };
            opened.expect("failed to open window");
            cx.activate(true);
        });
}
