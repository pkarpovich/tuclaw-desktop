mod agents;
mod composer;
mod failure;
mod feed;
mod input;
mod message;
mod shell;
mod sidebar;
mod state;
mod theme;
mod thread;

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
use time::OffsetDateTime;

use failure::Startup;
use shell::Shell;

fn main() {
    let startup = failure::start(OffsetDateTime::now_utc());
    gpui_platform::application().run(move |cx: &mut App| {
        input::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(18.))),
            }),
            ..Default::default()
        };
        let opened = match startup {
            Startup::Ready(state) => {
                let state = cx.new(|_| *state);
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
