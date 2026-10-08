use std::sync::Arc;

use gpui::{
    FontWeight, Hsla, Image, InteractiveElement, ParentElement, SharedString, Styled, phi, px,
};
use gpui_kit::base::{
    Avatar, AvatarFallback, AvatarImage, Button, Switch, SwitchThumb, Toggle, ToggleGroup,
};

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
    Profile,
    Tile,
    Pocket,
    Card,
}

#[derive(Clone)]
pub struct Face {
    pub initials: SharedString,
    pub color: Hsla,
    pub picture: Option<Arc<Image>>,
}

#[derive(Clone, Copy)]
pub enum AvatarShape {
    Square,
    Round,
}

pub fn avatar(face: Face, size: AvatarSize) -> Avatar {
    shaped_avatar(face, size, AvatarShape::Square)
}

pub fn shaped_avatar(face: Face, size: AvatarSize, shape: AvatarShape) -> Avatar {
    let Face {
        initials,
        color,
        picture,
    } = face;
    let (side, radius, text) = match size {
        AvatarSize::Row => (24., 7., 9.5),
        AvatarSize::Header => (26., 8., 10.),
        AvatarSize::Account => (30., 9., 11.),
        AvatarSize::Message => (34., 10., 11.),
        AvatarSize::Profile => (56., 15., 17.),
        AvatarSize::Tile => (44., 13., 12.),
        AvatarSize::Pocket => (32., 10., 10.5),
        AvatarSize::Card => (38., 12., 11.),
    };
    let radius = match shape {
        AvatarShape::Square => radius,
        AvatarShape::Round => side / 2.,
    };
    let avatar = Avatar::new()
        .flex_none()
        .size(px(side))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(color);
    if let Some(picture) = picture {
        return avatar.image(AvatarImage::new(picture).size_full().rounded(px(radius)));
    }
    avatar.fallback(
        AvatarFallback::new()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(text))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::chip_text())
            .child(initials),
    )
}

pub fn switch(selector: impl Into<SharedString>, checked: bool) -> Switch {
    let selector = selector.into();
    let id = selector.clone();
    Switch::new(id)
        .debug_selector(move || selector.to_string())
        .checked(checked)
        .flex_none()
        .flex()
        .items_center()
        .w(px(28.))
        .h(px(16.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(theme::border())
        .styles(|styles| styles.checked(|style| style.bg(theme::status_idle())))
        .child(
            SwitchThumb::new(checked)
                .size(px(12.))
                .rounded_full()
                .bg(theme::raised())
                .styles(|styles| styles.checked(|style| style.ml(px(12.)))),
        )
}
