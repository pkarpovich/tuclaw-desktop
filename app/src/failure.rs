use std::path::Path;

use anyhow::{Result, bail};
use gpui::{
    BoxShadow, Context, FontWeight, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};
use time::OffsetDateTime;
use tuclaw_core::paths::database_path;
use tuclaw_core::store::Store;

use crate::state::AppState;
use crate::theme;

pub enum Startup {
    Ready(Box<AppState>),
    Failed(FailureView),
}

pub struct FailureView {
    path: SharedString,
    error: SharedString,
}

pub fn start(now: OffsetDateTime) -> Startup {
    let path = match database_path() {
        Ok(path) => path,
        Err(error) => {
            return Startup::Failed(FailureView {
                path: SharedString::new_static("~/Library/Application Support/tuclaw-desktop"),
                error: SharedString::from(format!("{error:#}")),
            });
        }
    };
    start_at(&path, now)
}

pub fn start_at(path: &Path, now: OffsetDateTime) -> Startup {
    match load(path, now) {
        Ok(state) => Startup::Ready(Box::new(state)),
        Err(error) => Startup::Failed(FailureView {
            path: SharedString::from(path.display().to_string()),
            error: SharedString::from(format!("{error:#}")),
        }),
    }
}

fn load(path: &Path, now: OffsetDateTime) -> Result<AppState> {
    let Some(directory) = path.parent() else {
        bail!("the database path {} names no directory", path.display());
    };
    std::fs::create_dir_all(directory)?;
    let store = Store::open(path)?;
    store.seed_if_needed(now)?;
    AppState::new(store)
}

impl Render for FailureView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(40.))
            .bg(theme::window())
            .text_color(theme::text_primary())
            .text_size(px(13.5))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .max_w(px(560.))
                    .p(px(24.))
                    .rounded(px(12.))
                    .bg(theme::card())
                    .border_1()
                    .border_color(theme::border())
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(8.), theme::shadow())
                            .blur_radius(px(24.))
                            .spread_radius(px(-10.)),
                    ])
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child("The workspace could not be opened"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme::text_label())
                                    .child("Database"),
                            )
                            .child(
                                div()
                                    .p(px(8.))
                                    .rounded(px(8.))
                                    .bg(theme::sunken())
                                    .text_color(theme::text_secondary())
                                    .child(self.path.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme::text_label())
                                    .child("Error"),
                            )
                            .child(div().text_color(theme::accent()).child(self.error.clone())),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::TestAppContext;
    use time::macros::datetime;

    use super::{FailureView, SharedString, Startup, start_at};

    #[test]
    fn an_unwritable_path_produces_the_failure_state() {
        let directory = std::env::temp_dir().join("tuclaw-desktop-failure-view");
        fs::create_dir_all(&directory).expect("the temporary directory is created");
        let blocker = directory.join("blocker");
        fs::write(&blocker, b"not a directory").expect("the blocking file is written");
        let path = blocker.join("tuclaw.sqlite");
        let startup = start_at(&path, datetime!(2026-08-26 21:00 UTC));
        fs::remove_dir_all(&directory).expect("the temporary directory is removed");
        match startup {
            Startup::Ready(_) => panic!("a database under a file must not open"),
            Startup::Failed(FailureView { path, error }) => {
                assert!(path.ends_with("blocker/tuclaw.sqlite"), "{path}");
                assert!(!error.is_empty());
            }
        }
    }

    #[gpui::test]
    fn drawing_the_failure_view_does_not_panic(cx: &mut TestAppContext) {
        let view = FailureView {
            path: SharedString::new_static("/tmp/tuclaw-desktop/tuclaw.sqlite"),
            error: SharedString::new_static("unable to open database file"),
        };
        let (view, cx) = cx.add_window_view(move |_window, _cx| view);
        cx.run_until_parked();
        view.read_with(cx, |view, _cx| {
            assert_eq!(
                view.error,
                SharedString::new_static("unable to open database file")
            )
        });
    }
}
