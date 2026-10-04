pub mod agents;
pub mod audio;
pub mod composer;
pub mod failure;
pub mod feed;
pub mod input;
pub mod link;
pub mod live;
pub mod menu;
pub mod message;
pub mod rich;
pub mod shell;
pub mod sidebar;
pub mod state;
#[cfg(test)]
mod testing;
pub mod theme;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use failure::Startup;
use link::Config;
use shell::Shell;

pub fn run() {
    let startup = failure::start(Config::from_env());
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        input::bind_keys(cx);
        menu::install(cx);
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
                cx.open_window(options, |_, cx| cx.new(|cx| Shell::new(state, cx)))
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
