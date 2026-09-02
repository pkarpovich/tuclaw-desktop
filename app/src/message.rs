use std::rc::Rc;

use gpui::{
    App, Div, FontWeight, IntoElement, SharedString, Stateful, Window, div, prelude::*, px,
};
use time::OffsetDateTime;
use time::macros::format_description;
use tuclaw_core::model::{Agent, Author, Message, MessageId, Span};

use crate::theme;

pub type OnOpen = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;

struct Writer {
    name: SharedString,
    initials: SharedString,
    tone: Tone,
    badge: Badge,
}

enum Tone {
    User,
    Agent(usize),
}

enum Badge {
    Agent,
    None,
}

pub fn message_row(message: &Message, agents: &[Agent], on_open: OnOpen) -> impl IntoElement {
    let Message {
        id,
        author,
        body,
        sent_at,
        reply_count,
    } = message;
    let writer = writer(*author, agents);
    let MessageId(raw) = *id;
    let group = SharedString::from(format!("message-{raw}"));
    let column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .gap(px(2.))
        .child(byline(&writer, *sent_at))
        .child(paragraph(body));
    let row = div()
        .group(group.clone())
        .relative()
        .flex()
        .gap(px(12.))
        .px(px(20.))
        .py(px(8.))
        .child(avatar(&writer));
    if *reply_count == 0 {
        return row.child(column).child(hover_reply(*id, group, on_open));
    }
    row.child(column.child(replies_pill(*id, *reply_count, on_open)))
}

pub fn reply_label(count: usize) -> String {
    if count == 1 {
        return "1 reply".to_string();
    }
    format!("{count} replies")
}

fn writer(author: Author, agents: &[Agent]) -> Writer {
    let Author::Agent(author) = author else {
        return Writer {
            name: SharedString::new_static("You"),
            initials: SharedString::new_static("YO"),
            tone: Tone::User,
            badge: Badge::None,
        };
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
        initials,
        role: _,
        status: _,
        sort_index,
    }) = found
    else {
        return Writer {
            name: SharedString::new_static("unknown agent"),
            initials: SharedString::new_static("··"),
            tone: Tone::Agent(0),
            badge: Badge::Agent,
        };
    };
    Writer {
        name: SharedString::from(name.clone()),
        initials: SharedString::from(initials.clone()),
        tone: Tone::Agent(*sort_index as usize),
        badge: Badge::Agent,
    }
}

fn avatar(writer: &Writer) -> Div {
    let tone = match writer.tone {
        Tone::User => theme::accent(),
        Tone::Agent(index) => theme::agent_chip(index),
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

fn agent_badge() -> Div {
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

fn paragraph(body: &[Span]) -> Div {
    let mut paragraph = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(4.))
        .gap_y(px(2.))
        .text_size(px(14.5));
    for span in body {
        match span {
            Span::Text(text) => {
                for word in text.split_whitespace() {
                    paragraph = paragraph.child(div().child(word.to_string()));
                }
            }
            Span::Mention(name) => paragraph = paragraph.child(mention(name.clone())),
            Span::Code(code) => paragraph = paragraph.child(code_chip(code.clone())),
        }
    }
    paragraph
}

fn mention(name: String) -> Div {
    div()
        .flex_none()
        .px(px(5.))
        .rounded(px(5.))
        .bg(theme::mention_field())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::mention_text())
        .child(name)
}

fn code_chip(code: String) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(6.))
        .bg(theme::sunken())
        .text_size(px(12.5))
        .child(code)
}

fn replies_pill(id: MessageId, count: usize, on_open: OnOpen) -> Stateful<Div> {
    let MessageId(raw) = id;
    let selector = format!("message-reply-{raw}");
    pill()
        .mt(px(9.))
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .on_click(move |_event, window, cx| on_open(id, window, cx))
        .child(bubble())
        .child(reply_label(count))
}

fn hover_reply(id: MessageId, group: SharedString, on_open: OnOpen) -> Stateful<Div> {
    let MessageId(raw) = id;
    let selector = format!("message-reply-{raw}");
    pill()
        .absolute()
        .right(px(20.))
        .top(px(4.))
        .invisible()
        .group_hover(group, |style| style.visible())
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .on_click(move |_event, window, cx| on_open(id, window, cx))
        .child(bubble())
        .child("Reply")
}

fn pill() -> Div {
    div()
        .flex()
        .flex_none()
        .self_start()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .py(px(4.))
        .rounded_full()
        .bg(theme::raised())
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_secondary())
        .cursor_pointer()
        .hover(|style| style.bg(theme::sunken()))
}

fn bubble() -> Div {
    div()
        .flex_none()
        .w(px(12.))
        .h(px(10.))
        .rounded(px(3.))
        .border_1()
        .border_color(theme::text_muted())
}

#[cfg(test)]
mod tests {
    use super::reply_label;

    #[test]
    fn one_reply_reads_in_the_singular() {
        assert_eq!(reply_label(1), "1 reply");
    }

    #[test]
    fn every_other_count_reads_in_the_plural() {
        assert_eq!(reply_label(0), "0 replies");
        assert_eq!(reply_label(2), "2 replies");
        assert_eq!(reply_label(4), "4 replies");
    }
}
