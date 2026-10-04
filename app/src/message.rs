use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, BoxShadow, Div, FontWeight, HighlightStyle, IntoElement, SharedString, Stateful,
    StyledText, Window, div, point, prelude::*, px, relative,
};
use time::OffsetDateTime;
use time::macros::format_description;
use tuclaw_core::model::{Agent, Author, Message, MessageId, RecordingId, Span, Voice};

use crate::audio::{PEAKS, Peaks};
use crate::rich::{self, Ink, Parts};
use crate::state::Player;
use crate::theme;

const REASON: usize = 80;
const BARS: usize = 22;
const MIN_BAR: f32 = 4.0;
const MAX_BAR: f32 = 26.0;
const TRANSCRIPT: &str = "Transcript";

pub type OnToggle = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;
pub type OnPlay = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;

pub struct Look {
    pub fold: Fold,
    pub player: Player,
    pub peaks: Option<Peaks>,
}

struct Controls {
    player: Player,
    peaks: Option<Peaks>,
    on_play: OnPlay,
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
    let Look {
        fold,
        player,
        peaks,
    } = look;
    let writer = writer(*author, agents);
    let MessageId(raw) = *id;
    let selector = format!("message-{raw}");
    let Parts { thinking, answer } = rich::split_thinking(&source(body));
    let mut line = byline(&writer, *sent_at);
    if voice.is_some() {
        line = line.child(voice_tag());
    }
    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .gap(px(2.))
        .child(line);
    if let Some(voice) = voice {
        let controls = Controls {
            player,
            peaks,
            on_play: actions.on_play.clone(),
        };
        column = column.child(voice_card(*id, voice, answer, controls));
        return row(selector, &writer, column);
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
    row(selector, &writer, column)
}

fn row(selector: String, writer: &Writer, column: Div) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .gap(px(12.))
        .px(px(20.))
        .py(px(8.))
        .child(avatar(writer))
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

fn voice_card(id: MessageId, voice: &Voice, transcript: String, controls: Controls) -> Div {
    let Controls {
        player,
        peaks,
        on_play,
    } = controls;
    let MessageId(raw) = id;
    let selector = format!("voice-{raw}");
    let known = voice.duration.unwrap_or(Duration::ZERO);
    let (glyph, elapsed, total, failure) = match player {
        Player::Stopped => ("▶", None, known, None),
        Player::Loading => ("…", None, known, None),
        Player::Playing { position, total } => ("■", Some(position), total, None),
        Player::Failed(reason) => ("▶", None, known, Some(reason)),
    };
    let button = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(px(30.))
        .h(px(30.))
        .rounded_full()
        .bg(theme::accent())
        .shadow(vec![BoxShadow {
            color: theme::shadow(),
            offset: point(px(0.), px(2.)),
            blur_radius: px(6.),
            spread_radius: px(-2.),
            inset: false,
        }])
        .text_size(px(11.))
        .text_color(theme::chip_text())
        .cursor_pointer()
        .on_click(move |_event, window, cx| on_play(id, window, cx))
        .child(glyph);
    let label = match (failure, elapsed) {
        (Some(reason), _) => div()
            .text_color(theme::accent())
            .child(format!("can't play: {}", clamp_reason(&reason))),
        (None, Some(elapsed)) => div()
            .text_color(theme::text_label())
            .child(timing(elapsed, total)),
        (None, None) if total.is_zero() => div(),
        (None, None) => div().text_color(theme::text_label()).child(minutes(total)),
    };
    let top = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(13.))
        .py(px(11.))
        .child(button)
        .child(waveform(
            bar_heights(voice.recording, peaks),
            progress(elapsed.unwrap_or(Duration::ZERO), total),
        ))
        .child(label.flex_none().text_size(px(12.)));
    let card = div()
        .max_w(px(540.))
        .flex()
        .flex_col()
        .rounded(px(14.))
        .bg(theme::voice_card())
        .border_1()
        .border_color(theme::hairline())
        .overflow_hidden()
        .child(top);
    if transcript.is_empty() {
        return card;
    }
    card.child(
        div()
            .px(px(13.))
            .pt(px(9.))
            .pb(px(11.))
            .border_t_1()
            .border_color(theme::hairline())
            .text_size(px(13.))
            .line_height(relative(1.45))
            .text_color(theme::ink_soft())
            .child(transcript_text(transcript)),
    )
}

fn transcript_text(transcript: String) -> StyledText {
    let text = format!("{TRANSCRIPT}  {transcript}");
    let label = HighlightStyle {
        color: Some(theme::text_muted()),
        ..HighlightStyle::default()
    };
    StyledText::new(text).with_highlights(vec![(0..TRANSCRIPT.len(), label)])
}

fn waveform(heights: Vec<f32>, played: f32) -> Div {
    let lit = (played * BARS as f32).round() as usize;
    let mut bars = div().flex_1().flex().items_center().gap(px(2.5)).h(px(26.));
    for (index, height) in heights.into_iter().enumerate() {
        let tone = if index < lit {
            theme::ink_soft()
        } else {
            theme::wave_rest()
        };
        bars = bars.child(
            div()
                .flex_none()
                .w(px(3.))
                .h(px(height))
                .rounded(px(2.))
                .bg(tone),
        );
    }
    bars
}

fn bar_heights(recording: RecordingId, peaks: Option<Peaks>) -> Vec<f32> {
    match peaks {
        Some(peaks) => measured_heights(peaks),
        None => placeholder_heights(recording),
    }
}

fn measured_heights(peaks: Peaks) -> Vec<f32> {
    let Peaks(levels) = peaks;
    let mut groups = Vec::new();
    for bar in 0..BARS {
        let start = bar * PEAKS / BARS;
        let end = ((bar + 1) * PEAKS / BARS).max(start + 1);
        let mut level = 0u8;
        for value in &levels[start..end] {
            level = level.max(*value);
        }
        groups.push(level);
    }
    let mut loudest = 0u8;
    for level in &groups {
        loudest = loudest.max(*level);
    }
    let mut heights = Vec::new();
    for level in groups {
        let share = if loudest == 0 {
            0.0
        } else {
            f32::from(level) / f32::from(loudest)
        };
        heights.push(MIN_BAR + (MAX_BAR - MIN_BAR) * share);
    }
    heights
}

fn placeholder_heights(recording: RecordingId) -> Vec<f32> {
    let RecordingId(raw) = recording;
    let mut seed = (raw as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut heights = Vec::new();
    for _ in 0..BARS {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        heights.push(6.0 + (seed % 21) as f32);
    }
    heights
}

fn voice_tag() -> Div {
    div()
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .child("voice")
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
    fn the_waveform_is_stable_per_recording_and_fits_the_row() {
        let first = bar_heights(RecordingId(5), None);
        assert_eq!(first, bar_heights(RecordingId(5), None));
        assert_ne!(first, bar_heights(RecordingId(6), None));
        assert_eq!(first.len(), BARS);
        for height in first {
            assert!((6.0..=26.0).contains(&height), "{height}");
        }
    }

    #[test]
    fn measured_peaks_scale_to_the_loudest_bar() {
        let mut levels = [0u8; PEAKS];
        levels[0] = 40;
        levels[PEAKS - 1] = 80;
        let heights = bar_heights(RecordingId(1), Some(Peaks(levels)));
        assert_eq!(heights.len(), BARS);
        assert_eq!(heights[BARS - 1], MAX_BAR);
        assert_eq!(heights[0], MIN_BAR + (MAX_BAR - MIN_BAR) / 2.0);
        assert_eq!(heights[BARS / 2], MIN_BAR);
        let silent = bar_heights(RecordingId(1), Some(Peaks([0; PEAKS])));
        for height in silent {
            assert_eq!(height, MIN_BAR);
        }
    }

    #[test]
    fn a_failure_reason_is_one_short_line() {
        assert_eq!(clamp_reason("no device\nmore"), "no device");
        let long = "x".repeat(200);
        assert_eq!(clamp_reason(&long).chars().count(), REASON + 1);
    }
}
