use anyhow::Result;
use gpui::{
    App, BoxShadow, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::input::{Enter, Textarea, TextareaState};

use crate::icon::{Glyph, icon};
use crate::theme;

pub type OnSubmit = Box<dyn Fn(String, &mut App) -> Result<()>>;

const MAX_ROWS: usize = 10;

enum Sendable {
    Blank,
    Ready,
}

pub struct Composer {
    input: Entity<TextareaState>,
    on_submit: OnSubmit,
    _observation: Subscription,
}

impl Composer {
    pub fn new(
        placeholder: impl Into<SharedString>,
        on_submit: OnSubmit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Composer {
        let placeholder = placeholder.into();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, MAX_ROWS)
                .submit_on_enter(true)
                .placeholder(placeholder)
        });
        let observation = cx.observe(&input, |_composer, _input, cx| cx.notify());
        Composer {
            input,
            on_submit,
            _observation: observation,
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }

    #[cfg(test)]
    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub fn restore(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
        });
    }

    fn enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        let Enter {
            secondary: _,
            shift,
        } = action;
        if *shift {
            cx.propagate();
            return;
        }
        self.submit(window, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().to_string();
        if body.trim().is_empty() {
            return;
        }
        let Ok(()) = (self.on_submit)(body, cx) else {
            return;
        };
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn send_button(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        let (selector, size) = ("composer-send-feed", px(32.));
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
            .rounded_full();
        match sendable {
            Sendable::Blank => {
                button
                    .bg(theme::sunken())
                    .child(icon(Glyph::Send, px(16.), theme::text_muted()))
            }
            Sendable::Ready => button
                .bg(theme::accent())
                .child(icon(Glyph::Send, px(16.), theme::chip_text()))
                .cursor_pointer()
                .shadow(vec![
                    BoxShadow::new(px(0.), px(3.), theme::shadow())
                        .blur_radius(px(8.))
                        .spread_radius(px(-3.)),
                ])
                .on_click(cx.listener(|composer, _event, window, cx| composer.submit(window, cx))),
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
                            .child(
                                div()
                                    .id("input-feed")
                                    .debug_selector(|| "input-feed".to_string())
                                    .flex_1()
                                    .min_w(px(0.))
                                    .on_action(cx.listener(Self::enter))
                                    .child(Textarea::new(&self.input)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .px(px(9.))
                            .pt(px(6.))
                            .pb(px(9.))
                            .child(tool(Glyph::Mention))
                            .child(tool(Glyph::Attach))
                            .child(tool(Glyph::Emoji))
                            .child(tool(Glyph::Format))
                            .child(div().flex_1())
                            .child(hint())
                            .child(talk_chip())
                            .child(self.send_button(sendable, cx)),
                    ),
            )
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sendable = if self.input.read(cx).value().trim().is_empty() {
            Sendable::Blank
        } else {
            Sendable::Ready
        };
        self.feed_shape(sendable, cx)
    }
}

fn tool(glyph: Glyph) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(30.))
        .h(px(30.))
        .rounded(px(8.))
        .child(icon(glyph, px(16.), theme::text_secondary()))
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
        .child(icon(Glyph::Voice, px(13.), theme::accent()))
        .child("Talk")
}

#[cfg(test)]
mod tests {
    use anyhow::bail;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use tuclaw_core::model::Span;

    use super::{Composer, OnSubmit};
    use crate::state::AppState;
    use crate::testing::loaded;

    fn mount(
        cx: &mut TestAppContext,
        on_submit: OnSubmit,
    ) -> (Entity<Composer>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(move |window, cx| {
            Composer::new("Message #General", on_submit, window, cx)
        })
    }

    fn sending(
        cx: &mut TestAppContext,
    ) -> (Entity<AppState>, Entity<Composer>, &mut VisualTestContext) {
        let (_mock, state) = loaded(cx);
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
        composer.read_with(cx, |composer, cx| composer.text(cx))
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

    #[gpui::test]
    fn restore_puts_a_failed_body_back(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        composer.update_in(cx, |composer, window, cx| {
            composer.restore("Лисички 🍄".to_string(), window, cx)
        });
        assert_eq!(typed(&composer, cx), "Лисички 🍄");
        focus(&composer, cx);
        cx.simulate_input("!");
        assert_eq!(typed(&composer, cx), "Лисички 🍄!");
    }

    #[gpui::test]
    fn pasting_inserts_the_clipboard_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        focus(&composer, cx);
        cx.simulate_input("ask ");
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            "лисички со сливками\nи луком".to_string(),
        ));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(typed(&composer, cx), "ask лисички со сливками\nи луком");
    }

    #[gpui::test]
    fn select_all_then_typing_replaces_the_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        focus(&composer, cx);
        cx.simulate_input("draft one");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("final");
        assert_eq!(typed(&composer, cx), "final");
        cx.simulate_keystrokes("shift-left shift-left backspace");
        assert_eq!(typed(&composer, cx), "fin");
    }
}
