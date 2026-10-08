use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Context, Entity, FontWeight, IntoElement, Pixels, Render, SharedString, Subscription, Window,
    div, prelude::*, px,
};
use tuclaw_desktop::chrome::{Arming, Hold, Touch, on_touch_drag};
use tuclaw_desktop::feed::Feed;
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::theme;

use crate::frame;
use crate::keyboard;
use crate::navigator::{Menu, Navigator};
use crate::talk::Talk;

const EDGE: f32 = 16.;
const SWIPE_BACK: f32 = 80.;

pub struct Conversation {
    state: Entity<AppState>,
    navigator: Entity<Navigator>,
    feed: Entity<Feed>,
    edge_held: Rc<Cell<bool>>,
    edge_travel: Rc<Cell<Pixels>>,
    _observation: Subscription,
    _keyboard: Subscription,
}

impl Conversation {
    pub fn new(
        state: Entity<AppState>,
        navigator: Entity<Navigator>,
        talk: Entity<Talk>,
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
            on_hold: Rc::new(move |hold, _window, cx| {
                keyboard::hide();
                talk.update(cx, |talk, cx| talk.hold(hold, cx));
            }),
        };
        let built = state.clone();
        let feed = cx.new(|cx| Feed::phone(built, touch, window, cx));
        let focus = feed.read(cx).composer().read(cx).focus_handle(cx);
        let keyboard = keyboard::hide_on_blur(&focus, window, cx);
        Conversation {
            state,
            navigator,
            feed,
            edge_held: Rc::new(Cell::new(false)),
            edge_travel: Rc::new(Cell::new(px(0.))),
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
        let automations = if state.channel_tasks().is_empty() {
            None
        } else {
            let open = state.automations_open();
            let failed = state.unseen_failures() > 0;
            let toggler = self.state.clone();
            Some(
                div()
                    .id("conversation-automations")
                    .debug_selector(|| "conversation-automations".to_string())
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(34.))
                    .rounded_full()
                    .bg(if open {
                        theme::selection()
                    } else {
                        theme::sunken()
                    })
                    .on_click(move |_event, _window, cx| {
                        toggler.update(cx, |state, cx| {
                            if state.automations_open() {
                                state.close_automations(cx);
                            } else {
                                state.open_automations(cx);
                            }
                        })
                    })
                    .child(icon(Glyph::Automation, px(17.), theme::ink_soft()))
                    .children(failed.then(|| {
                        div()
                            .absolute()
                            .top(px(2.))
                            .right(px(2.))
                            .size(px(9.))
                            .rounded_full()
                            .bg(theme::accent())
                    })),
            )
        };
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
            .children(automations)
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
        let travel = self.edge_travel.clone();
        let navigator = self.navigator.clone();
        let edge = on_touch_drag(
            div()
                .id("conversation-edge")
                .debug_selector(|| "conversation-edge".to_string())
                .absolute()
                .left_0()
                .top(insets.top + px(56.))
                .bottom(px(120.))
                .w(px(EDGE)),
            Arming::Armed,
            self.edge_held.clone(),
            Rc::new(move |hold, _window, cx| match hold {
                Hold::Pressed => travel.set(px(0.)),
                Hold::Moved(offset) => travel.set(offset.x),
                Hold::Released => {
                    if travel.get() > px(SWIPE_BACK) {
                        navigator.update(cx, |navigator, cx| navigator.back(cx));
                    }
                }
            }),
        );
        div()
            .relative()
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
            .child(edge)
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
