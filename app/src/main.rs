mod state;

use anyhow::{Result, bail};
use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Window, WindowOptions, div};
use time::OffsetDateTime;
use tuclaw_core::paths::database_path;
use tuclaw_core::store::Store;

use state::AppState;

struct Root {
    _state: Entity<AppState>,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

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
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Root { _state: state })
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext, TestAppContext};
    use time::macros::datetime;
    use tuclaw_core::store::Store;

    use super::{AppState, Root};

    #[gpui::test]
    fn the_root_holds_the_loaded_workspace(cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        let state = AppState::new(store).expect("the workspace loads");
        let state = cx.new(|_| state);
        let root = cx.new(|_| Root { _state: state });
        root.read_with(cx, |_root, _cx| {});
    }
}
