use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Div, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px, relative,
};
use time::OffsetDateTime;
use time::macros::format_description;
use tuclaw_core::model::{Agent, Author, Message, MessageId, Span, Voice};

use crate::rich::{self, Ink, Parts};
use crate::state::Player;
use crate::theme;

const REASON: usize = 80;

pub type OnToggle = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;
pub type OnPlay = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;

pub struct Look {
    pub fold: Fold,
    pub player: Player,
}

pub struct Actions {
    pub on_toggle: OnToggle,
    pub on_play: OnPlay,
}

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
    look: Look,
    actions: &Actions,
) -> impl IntoElement {
    let Message {
        id,
        author,
        body,
        sent_at,
        voice,
    } = message;
    let Look { fold, player } = look;
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
    if let Some(voice) = voice {
        column = column.child(voice_row(*id, voice, player, actions.on_play.clone()));
    }
    if let Some(thinking) = thinking {
        column = column.child(thinking_fold(
            *id,
            thinking,
            fold,
            actions.on_toggle.clone(),
        ));
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

fn voice_row(id: MessageId, voice: &Voice, player: Player, on_play: OnPlay) -> Div {
    let MessageId(raw) = id;
    let selector = format!("voice-{raw}");
    let known = voice.duration.unwrap_or(Duration::ZERO);
    let (glyph, elapsed, total, failure) = match player {
        Player::Stopped => ("▶", Duration::ZERO, known, None),
        Player::Loading => ("…", Duration::ZERO, known, None),
        Player::Playing { position, total } => ("■", position, total, None),
        Player::Failed(reason) => ("▶", Duration::ZERO, known, Some(reason)),
    };
    let button = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(px(28.))
        .h(px(28.))
        .rounded_full()
        .bg(theme::accent())
        .text_size(px(11.))
        .text_color(theme::chip_text())
        .cursor_pointer()
        .on_click(move |_event, window, cx| on_play(id, window, cx))
        .child(glyph);
    let bar = div()
        .flex_none()
        .w(px(180.))
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme::sunken())
        .child(
            div()
                .h_full()
                .w(relative(progress(elapsed, total)))
                .rounded(px(2.))
                .bg(theme::accent()),
        );
    let label = match failure {
        Some(reason) => div()
            .text_color(theme::accent())
            .child(format!("can't play: {}", clamp_reason(&reason))),
        None => div()
            .text_color(theme::text_muted())
            .child(timing(elapsed, total)),
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .py(px(4.))
        .child(button)
        .child(bar)
        .child(label.text_size(px(11.5)))
}

fn progress(elapsed: Duration, total: Duration) -> f32 {
    if total.is_zero() {
        return 0.0;
    }
    (elapsed.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
}

fn timing(elapsed: Duration, total: Duration) -> String {
    if total.is_zero() {
        return minutes(elapsed);
    }
    format!("{} / {}", minutes(elapsed), minutes(total))
}

fn minutes(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn clamp_reason(reason: &str) -> String {
    let line = reason.lines().next().unwrap_or_default();
    let mut clamped = String::new();
    for (count, letter) in line.chars().enumerate() {
        if count == REASON {
            clamped.push('…');
            break;
        }
        clamped.push(letter);
    }
    clamped
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_reads_as_minutes_and_seconds() {
        assert_eq!(
            timing(Duration::from_secs(3), Duration::from_millis(10_320)),
            "0:03 / 0:10"
        );
        assert_eq!(
            timing(Duration::from_secs(61), Duration::from_secs(294)),
            "1:01 / 4:54"
        );
        assert_eq!(timing(Duration::from_secs(5), Duration::ZERO), "0:05");
    }

    #[test]
    fn progress_stays_within_the_bar() {
        assert_eq!(progress(Duration::ZERO, Duration::ZERO), 0.0);
        assert_eq!(
            progress(Duration::from_secs(5), Duration::from_secs(10)),
            0.5
        );
        assert_eq!(
            progress(Duration::from_secs(12), Duration::from_secs(10)),
            1.0
        );
    }

    #[test]
    fn a_failure_reason_is_one_short_line() {
        assert_eq!(clamp_reason("no device\nmore"), "no device");
        let long = "x".repeat(200);
        assert_eq!(clamp_reason(&long).chars().count(), REASON + 1);
    }
}
