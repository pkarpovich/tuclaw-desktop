use gpui::{FontWeight, InteractiveElement, SharedString, Styled, phi};
use gpui_kit::base::{Button, Toggle, ToggleGroup};

use crate::theme;

pub fn button(selector: impl Into<SharedString>) -> Button {
    let selector = selector.into();
    let id = selector.clone();
    Button::new(id)
        .debug_selector(move || selector.to_string())
        .focusable(false)
        .cursor_pointer()
}

pub fn row_button(selector: impl Into<SharedString>) -> Button {
    button(selector).justify_start().line_height(phi())
}

pub fn segments(selector: &'static str) -> ToggleGroup {
    ToggleGroup::new(selector)
        .debug_selector(move || selector.to_string())
        .flex()
        .items_center()
}

pub fn segment(selector: &'static str, pressed: bool) -> Toggle {
    Toggle::new(selector)
        .debug_selector(move || selector.to_string())
        .pressed(pressed)
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_secondary())
        .styles(|styles| {
            styles.pressed(|style| style.bg(theme::raised()).text_color(theme::text_primary()))
        })
}
