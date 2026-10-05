use gpui::{
    BoxShadow, Context, FontWeight, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};

use crate::audio::{PeakCache, RodioSpeaker};
use crate::link::{self, Config};
use crate::state::AppState;
use crate::theme;

pub enum Startup {
    Ready(Box<AppState>),
    Failed(FailureView),
}

pub struct FailureView {
    source: SharedString,
    error: SharedString,
}

pub fn start(config: Config) -> Startup {
    let source = SharedString::from(config.label());
    match config.client() {
        Ok((client, source)) => {
            let peaks = link::peak_directory(&source);
            let state = AppState::new(client, source, Box::new(RodioSpeaker::default()));
            #[cfg(target_os = "macos")]
            let state = state.with_recorder(Box::new(crate::recorder::AvRecorder::default()));
            let state = match peaks {
                Some(directory) => state.with_peak_cache(PeakCache::new(directory)),
                None => state,
            };
            Startup::Ready(Box::new(state))
        }
        Err(error) => Startup::Failed(FailureView {
            source,
            error: SharedString::from(error),
        }),
    }
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
                            .child("Tuclaw could not start"),
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
                                    .child("Daemon"),
                            )
                            .child(
                                div()
                                    .p(px(8.))
                                    .rounded(px(8.))
                                    .bg(theme::sunken())
                                    .text_color(theme::text_secondary())
                                    .child(self.source.clone()),
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
    use gpui::{SharedString, TestAppContext};

    use super::{FailureView, Startup, start};
    use crate::link::{Config, Source};

    #[test]
    fn the_mock_config_starts_on_the_mock() {
        let startup = start(Config::Mock);
        let Startup::Ready(state) = startup else {
            panic!("the mock always starts");
        };
        assert_eq!(state.source(), &Source::Mock);
    }

    #[test]
    fn a_daemon_url_without_a_token_starts_on_the_open_daemon() {
        let startup = start(Config::Daemon {
            url: "http://host:9090".into(),
            token: None,
        });
        let Startup::Ready(state) = startup else {
            panic!("an open daemon needs no token");
        };
        assert_eq!(state.source(), &Source::Daemon("http://host:9090".into()));
    }

    #[test]
    fn a_daemon_url_that_is_not_http_produces_the_failure_state() {
        let startup = start(Config::Daemon {
            url: "ftp://host".into(),
            token: None,
        });
        let Startup::Failed(FailureView { source, error: _ }) = startup else {
            panic!("a non-http URL must not start");
        };
        assert_eq!(source, SharedString::from("ftp://host"));
    }

    #[gpui::test]
    fn drawing_the_failure_view_does_not_panic(cx: &mut TestAppContext) {
        let view = FailureView {
            source: SharedString::new_static("http://host:9090"),
            error: SharedString::new_static("invalid request"),
        };
        let (view, cx) = cx.add_window_view(move |_window, _cx| view);
        cx.run_until_parked();
        view.read_with(cx, |view, _cx| {
            assert_eq!(view.error, SharedString::new_static("invalid request"))
        });
    }
}
