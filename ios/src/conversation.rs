use std::rc::Rc;

use gpui::{
    Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use tuclaw_desktop::chrome::Touch;
use tuclaw_desktop::feed::Feed;
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::theme;

use crate::frame;
use crate::keyboard;
use crate::navigator::{Menu, Navigator};

pub struct Conversation {
    state: Entity<AppState>,
    navigator: Entity<Navigator>,
    feed: Entity<Feed>,
    _observation: Subscription,
    _keyboard: Subscription,
}

impl Conversation {
    pub fn new(
        state: Entity<AppState>,
        navigator: Entity<Navigator>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Conversation {
        let observation = cx.observe(&state, |_conversation, _state, cx| cx.notify());
        let presser = navigator.clone();
        let touch = Touch {
            on_press: Rc::new(move |message, _window, cx| {
                presser.update(cx, |navigator, cx| {
                    navigator.open_menu(Menu::Message(message), cx)
                })
            }),
            on_field: Rc::new(|_window, _cx| keyboard::show()),
            on_drag: Rc::new(|_window, _cx| keyboard::hide()),
        };
        let built = state.clone();
        let feed = cx.new(|cx| Feed::phone(built, touch, window, cx));
        let focus = feed.read(cx).composer().read(cx).focus_handle(cx);
        let keyboard = keyboard::hide_on_blur(&focus, window, cx);
        Conversation {
            state,
            navigator,
            feed,
            _observation: observation,
            _keyboard: keyboard,
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let mut name = String::new();
        let mut subtitle = String::new();
        if let Some(channel) = state.selected() {
            for candidate in state.channels() {
                if candidate.id == channel {
                    name = candidate.name.clone();
                }
            }
            subtitle = subtitle_text(state.wired_agents(channel), state.working(channel).len());
        }
        let navigator = self.navigator.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pt(frame::insets().top)
            .px(px(12.))
            .pb(px(10.))
            .bg(theme::card())
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .id("conversation-back")
                    .debug_selector(|| "conversation-back".to_string())
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(34.))
                    .on_click(move |_event, _window, cx| {
                        navigator.update(cx, |navigator, cx| navigator.back(cx))
                    })
                    .child(icon(Glyph::Back, px(22.), theme::accent())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .text_color(theme::text_muted())
                                    .child("#"),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(SharedString::from(name)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_label())
                            .child(SharedString::from(subtitle)),
                    ),
            )
    }
}

impl Render for Conversation {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let insets = frame::insets();
        let keyboard = frame::keyboard_height();
        let bottom = if keyboard > px(0.) {
            keyboard
        } else {
            insets.bottom
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::card())
            .child(self.header(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.feed.clone()),
            )
            .child(div().flex_none().h(bottom).bg(theme::card()))
    }
}

fn subtitle_text(agents: usize, running: usize) -> String {
    let agents = match agents {
        1 => "1 agent".to_string(),
        count => format!("{count} agents"),
    };
    match running {
        0 => agents,
        count => format!("{agents} · {count} running"),
    }
}

#[cfg(test)]
mod tests {
    use super::subtitle_text;

    #[test]
    fn the_subtitle_counts_agents_and_runs() {
        assert_eq!(subtitle_text(1, 0), "1 agent");
        assert_eq!(subtitle_text(3, 2), "3 agents · 2 running");
    }
}
