mod feed;
mod shell;
mod sidebar;
mod state;
mod theme;

use anyhow::{Result, bail};
use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
use time::OffsetDateTime;
use tuclaw_core::paths::database_path;
use tuclaw_core::store::Store;

use shell::Shell;
use state::AppState;

fn load_state() -> Result<AppState> {
    let path = database_path()?;
    let Some(directory) = path.parent() else {
        bail!("the database path {} names no directory", path.display());
    };
    std::fs::create_dir_all(directory)?;
    let store = Store::open(&path)?;
    store.seed_if_needed(OffsetDateTime::now_utc())?;
    AppState::new(store)
}

fn main() {
    let state = match load_state() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("the workspace could not be opened: {error}");
            return;
        }
    };
    gpui_platform::application().run(move |cx: &mut App| {
        let state = cx.new(|_| state);
        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.), px(18.))),
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Shell::new(state, cx)),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
