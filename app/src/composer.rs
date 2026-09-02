use anyhow::Result;
use gpui::{
    App, BoxShadow, Context, Div, Entity, FocusHandle, FontWeight, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::input::{Submitted, TextInput};
use crate::theme;

pub enum ComposerKind {
    Feed,
    Thread,
}

pub type OnSubmit = Box<dyn Fn(String, &mut App) -> Result<()>>;

enum Sendable {
    Blank,
    Ready,
}

pub struct Composer {
    input: Entity<TextInput>,
    kind: ComposerKind,
    on_submit: OnSubmit,
    _observation: Subscription,
    _submissions: Subscription,
}

impl Composer {
    pub fn new(
        kind: ComposerKind,
        placeholder: impl Into<SharedString>,
        on_submit: OnSubmit,
        cx: &mut Context<Self>,
    ) -> Composer {
        let selector = match kind {
            ComposerKind::Feed => "input-feed",
            ComposerKind::Thread => "input-thread",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, selector, cx));
        let observation = cx.observe(&input, |_composer, _input, cx| cx.notify());
        let submissions = cx.subscribe(&input, |composer, _input, _event: &Submitted, cx| {
            composer.submit(cx);
        });
        Composer {
            input,
            kind,
            on_submit,
            _observation: observation,
            _submissions: submissions,
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle().clone()
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        if input.is_blank() {
            return;
        }
        let body = input.text().to_string();
        let Ok(()) = (self.on_submit)(body, cx) else {
            return;
        };
        self.input.update(cx, |input, cx| input.clear(cx));
    }

    fn send_button(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        let (selector, size) = match self.kind {
            ComposerKind::Feed => ("composer-send-feed", px(32.)),
            ComposerKind::Thread => ("composer-send-thread", px(30.)),
        };
        let button = div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(size)
            .h(size)
            .ml(px(7.))
            .rounded_full()
            .text_size(px(14.))
            .font_weight(FontWeight::BOLD)
            .child("↑");
        match sendable {
            Sendable::Blank => button.bg(theme::sunken()).text_color(theme::text_muted()),
            Sendable::Ready => button
                .bg(theme::accent())
                .text_color(theme::chip_text())
                .cursor_pointer()
                .shadow(vec![
                    BoxShadow::new(px(0.), px(3.), theme::shadow())
                        .blur_radius(px(8.))
                        .spread_radius(px(-3.)),
                ])
                .on_click(cx.listener(|composer, _event, _window, cx| composer.submit(cx))),
        }
    }

    fn feed_shape(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .flex_col()
            .px(px(14.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(14.))
                    .bg(theme::raised())
                    .border_1()
                    .border_color(theme::border())
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(2.), theme::shadow())
                            .blur_radius(px(10.))
                            .spread_radius(px(-4.)),
                    ])
                    .child(
                        div()
                            .flex()
                            .px(px(15.))
                            .pt(px(14.))
                            .pb(px(8.))
                            .min_w(px(0.))
                            .text_size(px(14.5))
                            .line_height(px(21.))
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .px(px(9.))
                            .pt(px(6.))
                            .pb(px(9.))
                            .child(mention_icon())
                            .child(attachment_icon())
                            .child(emoji_icon())
                            .child(format_icon())
                            .child(div().flex_1())
                            .child(hint())
                            .child(talk_chip())
                            .child(self.send_button(sendable, cx)),
                    ),
            )
    }

    fn thread_shape(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .flex_col()
            .px(px(14.))
            .pt(px(8.))
            .pb(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(12.))
                    .bg(theme::raised())
                    .border_1()
                    .border_color(theme::border())
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(2.), theme::shadow())
                            .blur_radius(px(8.))
                            .spread_radius(px(-4.)),
                    ])
                    .child(
                        div()
                            .flex()
                            .px(px(13.))
                            .pt(px(12.))
                            .pb(px(6.))
                            .min_w(px(0.))
                            .text_size(px(13.5))
                            .line_height(px(20.))
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .px(px(8.))
                            .pt(px(4.))
                            .pb(px(8.))
                            .child(mention_icon())
                            .child(microphone_icon())
                            .child(div().flex_1())
                            .child(self.send_button(sendable, cx)),
                    ),
            )
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sendable = if self.input.read(cx).is_blank() {
            Sendable::Blank
        } else {
            Sendable::Ready
        };
        match self.kind {
            ComposerKind::Feed => self.feed_shape(sendable, cx).into_any_element(),
            ComposerKind::Thread => self.thread_shape(sendable, cx).into_any_element(),
        }
    }
}

fn icon() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(30.))
        .h(px(30.))
        .rounded(px(8.))
        .text_color(theme::text_secondary())
}

fn mention_icon() -> impl IntoElement {
    icon().text_size(px(15.)).child("@")
}

fn attachment_icon() -> impl IntoElement {
    icon().child(
        div()
            .w(px(7.))
            .h(px(15.))
            .rounded(px(4.))
            .border_1()
            .border_color(theme::text_secondary()),
    )
}

fn emoji_icon() -> impl IntoElement {
    icon().child(
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .w(px(15.))
            .h(px(15.))
            .rounded_full()
            .border_1()
            .border_color(theme::text_secondary())
            .child(eye())
            .child(eye()),
    )
}

fn eye() -> Div {
    div()
        .w(px(2.))
        .h(px(2.))
        .rounded_full()
        .bg(theme::text_secondary())
}

fn microphone_icon() -> impl IntoElement {
    icon().child(
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .child(
                div()
                    .w(px(6.))
                    .h(px(9.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(theme::text_secondary()),
            )
            .child(div().w(px(10.)).h(px(1.)).bg(theme::text_secondary())),
    )
}

fn format_icon() -> impl IntoElement {
    icon()
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .child("Aa")
}

fn hint() -> impl IntoElement {
    div()
        .flex_none()
        .mr(px(6.))
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .child("Hold ⌥Space to talk")
}

fn talk_chip() -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .px(px(12.))
        .py(px(6.))
        .rounded_full()
        .border_1()
        .border_color(theme::border())
        .bg(theme::raised())
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_secondary())
        .child(
            div()
                .flex_none()
                .w(px(6.))
                .h(px(11.))
                .rounded(px(3.))
                .bg(theme::accent()),
        )
        .child("Talk")
}

#[cfg(test)]
mod tests {
    use anyhow::bail;
    use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
    use time::macros::datetime;
    use tuclaw_core::model::Span;
    use tuclaw_core::store::Store;

    use super::{Composer, ComposerKind, OnSubmit};
    use crate::input::bind_keys;
    use crate::state::AppState;

    fn mount(
        cx: &mut TestAppContext,
        on_submit: OnSubmit,
    ) -> (Entity<Composer>, &mut VisualTestContext) {
        cx.update(bind_keys);
        cx.add_window_view(move |_window, cx| {
            Composer::new(ComposerKind::Feed, "Message #movie-night", on_submit, cx)
        })
    }

    fn sending(
        cx: &mut TestAppContext,
    ) -> (Entity<AppState>, Entity<Composer>, &mut VisualTestContext) {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        let state = AppState::new(store).expect("the workspace loads");
        let state = cx.new(|_| state);
        let sender = state.clone();
        let (composer, cx) = mount(
            cx,
            Box::new(move |body, cx| sender.update(cx, |state, cx| state.send(body, cx))),
        );
        (state, composer, cx)
    }

    fn focus(composer: &Entity<Composer>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = composer.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        });
        cx.run_until_parked();
    }

    fn typed(composer: &Entity<Composer>, cx: &mut VisualTestContext) -> String {
        composer.read_with(cx, |composer, cx| {
            composer.input.read(cx).text().to_string()
        })
    }

    #[gpui::test]
    fn enter_sends_the_body_and_clears_the_input(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before + 1);
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("hi".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }

    #[gpui::test]
    fn enter_on_a_blank_input_sends_nothing(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before);
        });
    }

    #[gpui::test]
    fn shift_enter_sends_a_body_carrying_the_break(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        cx.simulate_input("a");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("b");
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("a\nb".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }

    #[gpui::test]
    fn a_rejected_submission_keeps_the_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| bail!("the write was rejected")));
        focus(&composer, cx);
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        assert_eq!(typed(&composer, cx), "hi");
    }

    #[gpui::test]
    fn the_send_button_takes_the_same_path(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_input("hi");
        let button = cx
            .debug_bounds("composer-send-feed")
            .expect("the send button is drawn");
        cx.simulate_click(button.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before + 1);
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("hi".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }
}
