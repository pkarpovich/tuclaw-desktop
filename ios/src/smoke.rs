use gpui::{
    Context, Entity, Focusable, IntoElement, Render, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::input::{Textarea, TextareaState};
use tuclaw_desktop::theme;

use crate::keyboard;

pub struct Smoke {
    input: Entity<TextareaState>,
    _keyboard: [Subscription; 2],
}

impl Smoke {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Smoke {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder("Message")
        });
        let focus = input.focus_handle(cx);
        let keyboard = keyboard::follow_focus(&focus, window, cx);
        Smoke {
            input,
            _keyboard: keyboard,
        }
    }
}

impl Render for Smoke {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(80.))
            .px(px(18.))
            .bg(theme::window())
            .text_color(theme::text_primary())
            .child(div().text_size(px(30.)).child("tuclaw"))
            .child(
                div()
                    .text_size(px(15.))
                    .child("Привет, это кириллица: ёжик в тумане"),
            )
            .child(div().text_size(px(15.)).child("Emoji: 🎬 🍿 🤖 ✅ 👩‍💻"))
            .child(
                div()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(theme::border())
                    .px(px(14.))
                    .py(px(8.))
                    .child(Textarea::new(&self.input)),
            )
    }
}
