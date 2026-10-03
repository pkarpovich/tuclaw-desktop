use std::rc::Rc;

use gpui::{App, Div, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px};
use time::OffsetDateTime;
use time::macros::format_description;
use tuclaw_core::model::{Agent, Author, Message, MessageId, Span};

use crate::rich::{self, Ink, Parts};
use crate::theme;

pub type OnToggle = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;

pub enum Fold {
    Collapsed,
    Expanded,
}

pub struct Writer {
    pub name: SharedString,
    initials: SharedString,
    tone: Tone,
    badge: Badge,
}

enum Tone {
    User,
    Agent(usize),
    System,
}

enum Badge {
    Agent,
    None,
}

pub fn message_row(
    message: &Message,
    agents: &[Agent],
    fold: Fold,
    on_toggle: OnToggle,
) -> impl IntoElement {
    let Message {
        id,
        author,
        body,
        sent_at,
    } = message;
    let writer = writer(*author, agents);
    let MessageId(raw) = *id;
    let selector = format!("message-{raw}");
    let Parts { thinking, answer } = rich::split_thinking(&source(body));
    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .gap(px(2.))
        .child(byline(&writer, *sent_at));
    if let Some(thinking) = thinking {
        column = column.child(thinking_fold(*id, thinking, fold, on_toggle));
    }
    if !answer.is_empty() {
        column = column.child(div().text_size(px(14.5)).child(rich::markdown(
            SharedString::from(format!("{selector}-md")),
            answer,
            Ink::Body,
        )));
    }
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .gap(px(12.))
        .px(px(20.))
        .py(px(8.))
        .child(avatar(&writer))
        .child(column)
}

pub fn source(body: &[Span]) -> String {
    let mut text = String::new();
    for span in body {
        match span {
            Span::Text(part) => text.push_str(part),
            Span::Mention(name) => {
                text.push('@');
                text.push_str(name);
            }
            Span::Code(code) => {
                text.push('`');
                text.push_str(code);
                text.push('`');
            }
        }
    }
    text
}

fn thinking_fold(id: MessageId, thinking: String, fold: Fold, on_toggle: OnToggle) -> Div {
    let MessageId(raw) = id;
    let selector = format!("thinking-{raw}");
    let marker = match fold {
        Fold::Collapsed => "▸ Thinking",
        Fold::Expanded => "▾ Thinking",
    };
    let toggle = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_none()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .cursor_pointer()
        .on_click(move |_event, window, cx| on_toggle(id, window, cx))
        .child(marker);
    let fold_box = div().flex().flex_col().gap(px(2.)).child(toggle);
    match fold {
        Fold::Collapsed => fold_box,
        Fold::Expanded => fold_box.child(
            div()
                .pl(px(10.))
                .border_l_2()
                .border_color(theme::hairline())
                .text_size(px(13.))
                .child(rich::markdown(
                    SharedString::from(format!("thinking-{raw}-md")),
                    thinking,
                    Ink::Muted,
                )),
        ),
    }
}

pub fn author_name(author: Author, agents: &[Agent]) -> SharedString {
    let author = match author {
        Author::User => return SharedString::new_static("You"),
        Author::System => return SharedString::new_static("tuclaw"),
        Author::Agent(author) => author,
    };
    let mut found = None;
    for candidate in agents {
        if candidate.id == author {
            found = Some(candidate);
            break;
        }
    }
    let Some(Agent {
        id: _,
        name,
        initials: _,
        role: _,
        status: _,
        sort_index: _,
    }) = found
    else {
        return SharedString::new_static("unknown agent");
    };
    SharedString::from(name.clone())
}

pub fn writer(author: Author, agents: &[Agent]) -> Writer {
    let name = author_name(author, agents);
    let author = match author {
        Author::User => {
            return Writer {
                name,
                initials: SharedString::new_static("YO"),
                tone: Tone::User,
                badge: Badge::None,
            };
        }
        Author::System => {
            return Writer {
                name,
                initials: SharedString::new_static("TC"),
                tone: Tone::System,
                badge: Badge::None,
            };
        }
        Author::Agent(author) => author,
    };
    let mut found = None;
    for candidate in agents {
        if candidate.id == author {
            found = Some(candidate);
            break;
        }
    }
    let Some(Agent {
        id: _,
        name: _,
        initials,
        role: _,
        status: _,
        sort_index,
    }) = found
    else {
        return Writer {
            name,
            initials: SharedString::new_static("··"),
            tone: Tone::Agent(0),
            badge: Badge::Agent,
        };
    };
    Writer {
        name,
        initials: SharedString::from(initials.clone()),
        tone: Tone::Agent(*sort_index as usize),
        badge: Badge::Agent,
    }
}

pub fn avatar(writer: &Writer) -> Div {
    let tone = match writer.tone {
        Tone::User => theme::accent(),
        Tone::Agent(index) => theme::agent_chip(index),
        Tone::System => theme::status_idle(),
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(px(34.))
        .h(px(34.))
        .rounded(px(10.))
        .bg(tone)
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::chip_text())
        .child(writer.initials.clone())
}

fn byline(writer: &Writer, sent_at: OffsetDateTime) -> Div {
    let line = div().flex().items_center().gap(px(8.)).child(
        div()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(writer.name.clone()),
    );
    let line = match writer.badge {
        Badge::Agent => line.child(agent_badge()),
        Badge::None => line,
    };
    line.child(
        div()
            .text_size(px(11.5))
            .text_color(theme::text_muted())
            .child(clock(sent_at)),
    )
}

pub fn agent_badge() -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(5.))
        .bg(theme::sunken())
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_secondary())
        .child("AGENT")
}

fn clock(sent_at: OffsetDateTime) -> String {
    let description = format_description!("[hour repr:12 padding:none]:[minute] [period]");
    sent_at.format(&description).unwrap_or_default()
}
