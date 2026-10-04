use gpui::{FontWeight, Hsla, InteractiveElement, ParentElement, SharedString, Styled, phi, px};
use gpui_kit::base::{Avatar, AvatarFallback, Button, Toggle, ToggleGroup};

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

#[derive(Clone, Copy)]
pub enum AvatarSize {
    Row,
    Header,
    Account,
    Message,
}

pub fn avatar(initials: impl Into<SharedString>, color: Hsla, size: AvatarSize) -> Avatar {
    let (side, radius, text) = match size {
        AvatarSize::Row => (24., 7., 9.5),
        AvatarSize::Header => (26., 8., 10.),
        AvatarSize::Account => (30., 9., 11.),
        AvatarSize::Message => (34., 10., 11.),
    };
    Avatar::new()
        .flex_none()
        .size(px(side))
        .rounded(px(radius))
        .bg(color)
        .fallback(
            AvatarFallback::new()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(text))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::chip_text())
                .child(initials.into()),
        )
}
