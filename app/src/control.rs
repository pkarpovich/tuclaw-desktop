use gpui::{InteractiveElement, SharedString, Styled, phi};
use gpui_kit::base::Button;

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
