use gpui::{Div, FontWeight, SharedString, Stateful, div, prelude::*, px};

use crate::agent_settings::Saving;
use crate::icon::{Glyph, icon};
use crate::theme;

pub fn label(text: &'static str) -> Div {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

pub fn field_frame(invalid: bool) -> Div {
    div()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(8.))
        .bg(theme::field())
        .border_1()
        .border_color(if invalid {
            theme::accent()
        } else {
            theme::border()
        })
}

pub fn error_line(text: String) -> Div {
    div()
        .text_size(px(11.))
        .text_color(theme::accent())
        .child(SharedString::from(format!("{text}. Not saved.")))
}

pub fn saving_label(saving: &Saving) -> Div {
    let line = div().flex().items_center().gap(px(4.)).text_size(px(11.5));
    match saving {
        Saving::Idle => line,
        Saving::Saving => line.text_color(theme::text_muted()).child("Saving…"),
        Saving::Saved => line
            .text_color(theme::text_muted())
            .child(icon(Glyph::Done, px(11.), theme::status_idle()))
            .child("Saved"),
        Saving::Failed(_) => line.text_color(theme::accent()).child("Not saved"),
    }
}

pub fn upload_failure(saving: &Saving, field_error: bool) -> Option<Stateful<Div>> {
    if field_error {
        return None;
    }
    match saving {
        Saving::Failed(reason) => Some(
            div()
                .id("upload-failure")
                .debug_selector(|| "upload-failure".to_string())
                .child(error_line(reason.clone())),
        ),
        Saving::Idle => None,
        Saving::Saving => None,
        Saving::Saved => None,
    }
}
