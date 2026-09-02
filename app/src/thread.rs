use gpui::{
    Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use tuclaw_core::model::{Agent, Author, ChannelId, Message};

use crate::composer::{Composer, ComposerKind};
use crate::message::{Replies, author_name, message_row};
use crate::state::{AppState, OpenThread, StateEvent};
use crate::theme;

pub struct ThreadPanel {
    state: Entity<AppState>,
    composer: Entity<Composer>,
    focus: Focus,
    _observation: Subscription,
    _events: Subscription,
}

enum Focus {
    Requested,
    Taken,
}

impl ThreadPanel {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> ThreadPanel {
        let observation = cx.observe(&state, |_panel, _state, cx| cx.notify());
        let events = cx.subscribe(
            &state,
            |panel, _state, event: &StateEvent, cx| match event {
                StateEvent::ThreadOpened => {
                    panel.focus = Focus::Requested;
                    cx.notify();
                }
                StateEvent::ReplyAppended => cx.notify(),
                StateEvent::SelectionChanged => {}
                StateEvent::MessageAppended => {}
                StateEvent::ThreadClosed => {}
            },
        );
        let sender = state.clone();
        let composer = cx.new(|cx| {
            Composer::new(
                ComposerKind::Thread,
                "Reply in thread",
                Box::new(move |body, cx| {
                    sender.update(cx, |state, cx| state.reply_in_thread(body, cx))
                }),
                cx,
            )
        });
        ThreadPanel {
            state,
            composer,
            focus: Focus::Taken,
            _observation: observation,
            _events: events,
        }
    }

    #[cfg(test)]
    pub fn input_focus(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.composer.read(cx).focus_handle(cx)
    }

    fn take_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.focus {
            Focus::Requested => {
                let focus = self.composer.read(cx).focus_handle(cx);
                focus.focus(window, cx);
                self.focus = Focus::Taken;
            }
            Focus::Taken => {}
        }
    }

    fn header(&self, subtitle: SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(52.))
            .px(px(14.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::BOLD)
                            .child("Thread"),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme::text_label())
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("thread-close")
                    .debug_selector(|| "thread-close".to_string())
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(28.))
                    .h(px(28.))
                    .rounded(px(8.))
                    .text_size(px(15.))
                    .text_color(theme::text_secondary())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::sunken()))
                    .on_click(cx.listener(|panel, _event, _window, cx| {
                        panel.state.update(cx, |state, cx| state.close_thread(cx));
                    }))
                    .child("✕"),
            )
    }
}

impl Render for ThreadPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let Some(OpenThread {
            root,
            channel,
            replies,
        }) = state.thread()
        else {
            return div().into_any_element();
        };
        let subtitle = subtitle(state, *channel, root.author);
        let agents = state.agents().to_vec();
        let root = root.clone();
        let replies = replies.clone();
        self.take_focus(window, cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(self.header(subtitle, cx))
            .child(body(root, replies, agents))
            .child(self.composer.clone())
            .into_any_element()
    }
}

fn body(root: Message, replies: Vec<Message>, agents: Vec<Agent>) -> impl IntoElement {
    let mut column = div()
        .id("thread-replies")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .pt(px(6.))
        .child(
            div()
                .flex()
                .flex_col()
                .pb(px(8.))
                .border_b_1()
                .border_color(theme::hairline())
                .child(message_row(&root, &agents, Replies::Hidden)),
        );
    for reply in replies {
        column = column.child(message_row(&reply, &agents, Replies::Hidden));
    }
    column
}

fn subtitle(state: &AppState, channel: ChannelId, author: Author) -> SharedString {
    let mut name = SharedString::new_static("");
    for candidate in state.channels() {
        if candidate.id == channel {
            name = SharedString::from(candidate.name.clone());
            break;
        }
    }
    let author = author_name(author, state.agents());
    SharedString::from(format!("#{name} · {author}"))
}

#[cfg(test)]
mod tests {
    use gpui::{
        AppContext, Context, Entity, IntoElement, Modifiers, Render, TestAppContext,
        VisualTestContext, Window, div, prelude::*, px,
    };
    use time::macros::datetime;
    use tuclaw_core::model::{ChannelId, MessageId, Span};
    use tuclaw_core::store::Store;

    use super::ThreadPanel;
    use crate::feed::Feed;
    use crate::input::bind_keys;
    use crate::state::AppState;

    struct Harness {
        feed: Entity<Feed>,
        thread: Entity<ThreadPanel>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .child(div().flex_1().child(self.feed.clone()))
                .child(div().w(px(360.)).child(self.thread.clone()))
        }
    }

    fn seeded(cx: &mut TestAppContext) -> Entity<AppState> {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        let state = AppState::new(store).expect("the workspace loads");
        cx.new(|_| state)
    }

    fn panel(
        cx: &mut TestAppContext,
    ) -> (
        Entity<AppState>,
        Entity<ThreadPanel>,
        &mut VisualTestContext,
    ) {
        cx.update(bind_keys);
        let state = seeded(cx);
        let built = state.clone();
        let (panel, cx) = cx.add_window_view(move |_window, cx| ThreadPanel::new(built, cx));
        (state, panel, cx)
    }

    fn harness(
        cx: &mut TestAppContext,
    ) -> (Entity<AppState>, Entity<Harness>, &mut VisualTestContext) {
        cx.update(bind_keys);
        let state = seeded(cx);
        let built = state.clone();
        let (harness, cx) = cx.add_window_view(move |_window, cx| {
            let feed = cx.new(|cx| Feed::new(built.clone(), cx));
            let thread = cx.new(|cx| ThreadPanel::new(built, cx));
            Harness { feed, thread }
        });
        (state, harness, cx)
    }

    fn thread_root(state: &Entity<AppState>, cx: &mut TestAppContext) -> MessageId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_count > 0 {
                    found = Some(message.id);
                    break;
                }
            }
            found.expect("movie-night carries a thread root")
        })
    }

    fn channel_named(state: &Entity<AppState>, cx: &mut TestAppContext, name: &str) -> ChannelId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for channel in state.channels() {
                if channel.name == name {
                    found = Some(channel.id);
                    break;
                }
            }
            found.expect("the fixtures carry that channel")
        })
    }

    #[gpui::test]
    fn drawing_an_open_thread_does_not_panic(cx: &mut TestAppContext) {
        let (state, _panel, cx) = panel(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.root.id, root);
            assert_eq!(thread.replies.len(), 4);
        });
    }

    #[gpui::test]
    fn the_panel_survives_a_channel_switch(cx: &mut TestAppContext) {
        let (state, _panel, cx) = panel(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        let personal = channel_named(&state, cx, "personal");
        state.update(cx, |state, cx| state.select(personal, cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert!(state.messages().is_empty());
            let thread = state.thread().expect("the thread stays open");
            assert_eq!(thread.root.id, root);
        });
    }

    #[gpui::test]
    fn replying_appends_to_the_thread_and_raises_the_count(cx: &mut TestAppContext) {
        let (state, panel, cx) = panel(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let focus = panel.read(cx).input_focus(cx);
            focus.focus(window, cx);
        });
        cx.run_until_parked();
        cx.simulate_input("on it");
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.replies.len(), 5);
            assert_eq!(thread.root.reply_count, 5);
            let last = thread.replies.last().expect("the reply was appended");
            assert_eq!(last.body, vec![Span::Text("on it".to_string())]);
            let mut counted = None;
            for message in state.messages() {
                if message.id == root {
                    counted = Some(message.reply_count);
                    break;
                }
            }
            assert_eq!(counted, Some(5));
        });
    }

    #[gpui::test]
    fn clicking_the_close_control_closes_the_thread(cx: &mut TestAppContext) {
        let (state, _panel, cx) = panel(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        let close = cx
            .debug_bounds("thread-close")
            .expect("the close control is drawn");
        cx.simulate_click(close.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| assert!(state.thread().is_none()));
    }

    #[gpui::test]
    fn opening_and_closing_a_thread_moves_the_focus(cx: &mut TestAppContext) {
        let (state, harness, cx) = harness(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        let focused = cx.update(|window, cx| {
            let thread = harness.read(cx).thread.read(cx).input_focus(cx);
            thread.is_focused(window)
        });
        assert!(focused, "the thread composer takes the focus");
        state.update(cx, |state, cx| state.close_thread(cx));
        cx.run_until_parked();
        let (feed, thread) = cx.update(|window, cx| {
            let harness = harness.read(cx);
            let feed = harness.feed.read(cx).input_focus(cx);
            let thread = harness.thread.read(cx).input_focus(cx);
            (feed.is_focused(window), thread.is_focused(window))
        });
        assert!(feed, "the feed composer takes the focus back");
        assert!(!thread, "the thread composer gives the focus up");
    }
}
