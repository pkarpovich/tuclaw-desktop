use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, BoxShadow, Div, FontWeight, HighlightStyle, Image, ImageSource,
    IntoElement, ObjectFit, Pixels, SharedString, Stateful, StyledText, Window, canvas, div, fill,
    img, point, prelude::*, px, relative, size,
};
use time::OffsetDateTime;
use tuclaw_core::model::{Agent, AgentId, Author, Message, MessageId, RecordingId, Span, Voice};
use tuclaw_core::v3::PublicUrl;

use gpui_kit::base::Avatar;

use crate::audio::{PEAKS, Peaks, Waveform};
use crate::card::{CardActions, with_card};
use crate::control::{self, AvatarSize, Face, button, row_button};
use crate::icon::{Glyph, icon, spinner};
use crate::link;
use crate::local::clock;
use crate::people::People;
use crate::pictures::{MAX_WIDTH, Remote, Shelf, Viewed, fit};
use crate::rich::{self, Ink, Parts, Picture, Segment};
use crate::runlog::{self, OnDisclose, Pane};
use crate::state::Player;
use crate::theme;

const REASON: usize = 80;
const BAR_WIDTH: f32 = 3.0;
const BAR_GAP: f32 = 2.5;
const MIN_BAR: f32 = 4.0;
const MAX_BAR: f32 = 26.0;
const TRANSCRIPT: &str = "Transcript";

pub type OnToggle = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;
pub type OnPlay = Rc<dyn Fn(MessageId, &mut Window, &mut App)>;

pub struct Look {
    pub fold: Fold,
    pub player: Player,
    pub waveform: Option<Waveform>,
    pub run: Option<Pane>,
    pub trigger: Option<AnyElement>,
}

struct Controls {
    player: Player,
    waveform: Option<Waveform>,
    on_play: OnPlay,
}

pub struct Actions {
    pub on_toggle: OnToggle,
    pub on_play: OnPlay,
    pub on_disclose: OnDisclose,
    pub on_picture: OnPicture,
    pub card: CardActions,
}

pub type OnPicture = Rc<dyn Fn(Viewed, &mut Window, &mut App)>;

pub enum Fold {
    Collapsed,
    Expanded,
}

pub struct Writer {
    pub name: SharedString,
    initials: SharedString,
    tone: Tone,
    badge: Badge,
    picture: Option<Arc<Image>>,
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
    people: &People,
    look: Look,
    actions: &Actions,
    shelf: &Shelf,
) -> impl IntoElement {
    let Message {
        id,
        author,
        body,
        sent_at,
        voice,
        run,
    } = message;
    let Look {
        fold,
        player,
        waveform,
        run: pane,
        trigger,
    } = look;
    let writer = writer(*author, people);
    let MessageId(raw) = *id;
    let selector = format!("message-{raw}");
    let face = match author {
        Author::Agent(agent) => with_card(
            format!("card-{raw}"),
            *agent,
            avatar(&writer),
            people,
            &actions.card,
        ),
        Author::User => avatar(&writer).into_any_element(),
        Author::System => avatar(&writer).into_any_element(),
    };
    let Parts { thinking, answer } = rich::split_thinking(&source(body));
    let quick = run.as_ref().and_then(runlog::quick_duration);
    let mut line = byline(&writer, *sent_at, quick);
    if voice.is_some() {
        line = line.child(voice_tag());
    }
    if let Some(trigger) = trigger {
        line = line.child(trigger);
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
            waveform,
            on_play: actions.on_play.clone(),
        };
        column = column.child(voice_card(*id, voice, answer, controls));
        return row(selector, face, column);
    }
    if let Some(thinking) = thinking.filter(|_| pane.is_none()) {
        column = column.child(thinking_fold(
            *id,
            thinking,
            fold,
            actions.on_toggle.clone(),
        ));
    }
    for (index, segment) in rich::split_pictures(&answer).into_iter().enumerate() {
        let key = match index {
            0 => format!("{selector}-md"),
            index => format!("{selector}-md-{index}"),
        };
        column = match segment {
            Segment::Text(text) => column.child(div().text_size(px(14.5)).child(rich::markdown(
                SharedString::from(key),
                text,
                Ink::Body,
            ))),
            Segment::Picture(picture) => column.child(picture_block(
                &key,
                &picture,
                shelf,
                actions.on_picture.clone(),
            )),
        };
    }
    if let Some(pane) = pane {
        column = column.child(runlog::render(*id, pane, actions.on_disclose.clone()));
    }
    row(selector, face, column)
}

fn picture_block(key: &str, picture: &Picture, shelf: &Shelf, on_picture: OnPicture) -> AnyElement {
    let Picture { alt, url } = picture;
    let caption = if alt.is_empty() {
        url.clone()
    } else {
        alt.clone()
    };
    let Some(public) = PublicUrl::parse(url) else {
        return picture_fallback(key, &caption, None);
    };
    match shelf.get(&public) {
        Some(Remote::Ready(shown)) => {
            let (width, height) = fit(shown.width, shown.height);
            let viewed = Viewed {
                url: public.clone(),
                caption: alt.clone(),
            };
            let mut block = div().flex().flex_col().gap(px(4.)).py(px(4.)).child(
                row_button(format!("{key}-picture"))
                    .accessibility_label(format!("Open {caption}"))
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::hairline())
                    .on_click(move |_event, window, cx| on_picture(viewed.clone(), window, cx))
                    .child(
                        img(ImageSource::Image(shown.image.clone()))
                            .w(px(width))
                            .h(px(height))
                            .object_fit(ObjectFit::Cover),
                    ),
            );
            if !alt.is_empty() {
                block = block.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(SharedString::from(alt.clone())),
                );
            }
            block.into_any_element()
        }
        Some(Remote::Failed) => picture_fallback(key, &caption, Some(url)),
        Some(Remote::Loading) | None => div()
            .id(SharedString::from(format!("{key}-loading")))
            .debug_selector({
                let key = format!("{key}-loading");
                move || key
            })
            .flex()
            .items_center()
            .gap(px(8.))
            .my(px(4.))
            .w(px(MAX_WIDTH))
            .h(px(96.))
            .px(px(14.))
            .rounded(px(10.))
            .bg(theme::sunken())
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(spinner(px(13.), theme::text_muted()))
            .child(SharedString::from(caption))
            .into_any_element(),
    }
}

fn picture_fallback(key: &str, caption: &str, link: Option<&String>) -> AnyElement {
    let text = match link {
        Some(url) => format!("Picture: [{caption}]({url})"),
        None => format!("Picture: {caption}"),
    };
    div()
        .text_size(px(13.))
        .child(rich::markdown(
            SharedString::from(format!("{key}-fallback")),
            text,
            Ink::Muted,
        ))
        .into_any_element()
}

fn row(selector: String, face: AnyElement, column: Div) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .items_start()
        .gap(px(12.))
        .px(px(20.))
        .py(px(8.))
        .child(face)
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
        waveform,
        on_play,
    } = controls;
    let MessageId(raw) = id;
    let selector = format!("voice-{raw}");
    let measured = waveform.map(|waveform| waveform.duration);
    let known = voice.duration.or(measured).unwrap_or(Duration::ZERO);
    let peaks = waveform.map(|waveform| waveform.peaks);
    let (glyph, elapsed, total, failure) = match player {
        Player::Stopped => (Some(Glyph::Play), None, known, None),
        Player::Loading => (None, None, known, None),
        Player::Playing { position, total } => (Some(Glyph::Stop), Some(position), total, None),
        Player::Failed(reason) => (Some(Glyph::Play), None, known, Some(reason)),
    };
    let label = match glyph {
        Some(Glyph::Stop) => "Stop the recording",
        Some(_) => "Play the recording",
        None => "Loading the recording",
    };
    let play = button(selector)
        .accessibility_label(label)
        .flex_none()
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
        .on_click(move |_event, window, cx| on_play(id, window, cx))
        .child(match glyph {
            Some(glyph) => icon(glyph, px(13.), theme::chip_text()).into_any_element(),
            None => spinner(px(14.), theme::chip_text()).into_any_element(),
        });
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
        .child(play)
        .child(wave_bars(
            voice.recording,
            peaks,
            progress(elapsed.unwrap_or(Duration::ZERO), total),
        ))
        .child(label.flex_none().text_size(px(12.)));
    let card = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(14.))
        .bg(theme::voice_card())
        .shadow(vec![BoxShadow {
            color: theme::hairline(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(0.5),
            inset: false,
        }])
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

fn wave_bars(recording: RecordingId, peaks: Option<Peaks>, played: f32) -> Div {
    let paint = move |bounds: Bounds<Pixels>, (): (), window: &mut Window, _cx: &mut App| {
        let count = bar_count(bounds.size.width / px(1.));
        let lit = (played * count as f32).round() as usize;
        for (index, height) in bar_heights(recording, peaks, count).into_iter().enumerate() {
            let tone = if index < lit {
                theme::ink_soft()
            } else {
                theme::wave_rest()
            };
            let bar = Bounds {
                origin: point(
                    bounds.origin.x + px(index as f32 * (BAR_WIDTH + BAR_GAP)),
                    bounds.origin.y + (bounds.size.height - px(height)) / 2.,
                ),
                size: size(px(BAR_WIDTH), px(height)),
            };
            window.paint_quad(fill(bar, tone).corner_radii(px(BAR_WIDTH / 2.)));
        }
    };
    div()
        .flex_1()
        .min_w(px(0.))
        .h(px(MAX_BAR))
        .child(canvas(|_bounds, _window, _cx| (), paint).size_full())
}

fn bar_count(width: f32) -> usize {
    (((width + BAR_GAP) / (BAR_WIDTH + BAR_GAP)).floor() as usize).max(1)
}

fn bar_heights(recording: RecordingId, peaks: Option<Peaks>, count: usize) -> Vec<f32> {
    match peaks {
        Some(peaks) => measured_heights(peaks, count),
        None => placeholder_heights(recording, count),
    }
}

fn measured_heights(peaks: Peaks, count: usize) -> Vec<f32> {
    let Peaks(levels) = peaks;
    let mut loudest = 0u8;
    for level in levels {
        loudest = loudest.max(level);
    }
    let mut heights = Vec::new();
    for bar in 0..count {
        let level = if count > PEAKS {
            interpolated(&levels, bar, count)
        } else {
            grouped(&levels, bar, count)
        };
        let share = if loudest == 0 {
            0.0
        } else {
            level / f32::from(loudest)
        };
        heights.push(MIN_BAR + (MAX_BAR - MIN_BAR) * share);
    }
    heights
}

fn grouped(levels: &[u8; PEAKS], bar: usize, count: usize) -> f32 {
    let start = bar * PEAKS / count;
    let end = ((bar + 1) * PEAKS / count).max(start + 1);
    let mut level = 0u8;
    for value in &levels[start..end] {
        level = level.max(*value);
    }
    f32::from(level)
}

fn interpolated(levels: &[u8; PEAKS], bar: usize, count: usize) -> f32 {
    let position = bar as f32 * (PEAKS - 1) as f32 / (count - 1) as f32;
    let below = position.floor() as usize;
    let above = (below + 1).min(PEAKS - 1);
    let weight = position - below as f32;
    f32::from(levels[below]) * (1.0 - weight) + f32::from(levels[above]) * weight
}

fn placeholder_heights(recording: RecordingId, count: usize) -> Vec<f32> {
    let RecordingId(raw) = recording;
    let mut seed = (raw as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut heights = Vec::new();
    for _ in 0..count {
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
        Fold::Collapsed => Glyph::Closed,
        Fold::Expanded => Glyph::Open,
    };
    let toggle = row_button(selector)
        .flex_none()
        .self_start()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .on_click(move |_event, window, cx| on_toggle(id, window, cx))
        .gap(px(3.))
        .child(icon(marker, px(12.), theme::text_muted()))
        .child("Thinking");
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

fn author_name(author: Author, people: &People) -> SharedString {
    let author = match author {
        Author::User => return SharedString::from(people.me.name.clone()),
        Author::System => return SharedString::new_static("tuclaw"),
        Author::Agent(author) => author,
    };
    let Some(agent) = find_agent(people.agents, author) else {
        return SharedString::new_static("unknown agent");
    };
    SharedString::from(agent.name.clone())
}

fn find_agent(agents: &[Agent], id: AgentId) -> Option<&Agent> {
    let mut found = None;
    for candidate in agents {
        if candidate.id == id {
            found = Some(candidate);
            break;
        }
    }
    found
}

pub fn writer(author: Author, people: &People) -> Writer {
    let name = author_name(author, people);
    let author = match author {
        Author::User => {
            return Writer {
                initials: SharedString::from(link::initials(&people.me.name)),
                name,
                tone: Tone::User,
                badge: Badge::None,
                picture: people.picture(people.me.picture.as_ref()),
            };
        }
        Author::System => {
            return Writer {
                name,
                initials: SharedString::new_static("TC"),
                tone: Tone::System,
                badge: Badge::None,
                picture: None,
            };
        }
        Author::Agent(author) => author,
    };
    let Some(Agent {
        id: _,
        name: _,
        initials,
        role: _,
        status: _,
        sort_index,
        picture,
    }) = find_agent(people.agents, author)
    else {
        return Writer {
            name,
            initials: SharedString::new_static("··"),
            tone: Tone::Agent(0),
            badge: Badge::Agent,
            picture: None,
        };
    };
    Writer {
        name,
        initials: SharedString::from(initials.clone()),
        tone: Tone::Agent(*sort_index as usize),
        badge: Badge::Agent,
        picture: people.picture(picture.as_ref()),
    }
}

pub fn avatar(writer: &Writer) -> Avatar {
    let color = match writer.tone {
        Tone::User => theme::accent(),
        Tone::Agent(index) => theme::agent_chip(index),
        Tone::System => theme::status_idle(),
    };
    control::avatar(
        Face {
            initials: writer.initials.clone(),
            color,
            picture: writer.picture.clone(),
        },
        AvatarSize::Message,
    )
}

fn byline(writer: &Writer, sent_at: OffsetDateTime, quick: Option<String>) -> Div {
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
            .child(match quick {
                Some(took) => format!("{} · {took}", clock(sent_at)),
                None => clock(sent_at),
            }),
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
        let first = bar_heights(RecordingId(5), None, 22);
        assert_eq!(first, bar_heights(RecordingId(5), None, 22));
        assert_ne!(first, bar_heights(RecordingId(6), None, 22));
        assert_eq!(first.len(), 22);
        for height in first {
            assert!((MIN_BAR..=MAX_BAR).contains(&height), "{height}");
        }
    }

    #[test]
    fn the_bar_count_follows_the_width() {
        assert_eq!(bar_count(0.0), 1);
        assert_eq!(bar_count(3.0), 1);
        assert_eq!(bar_count(8.5), 2);
        assert_eq!(bar_count(118.5), 22);
        assert_eq!(bar_count(800.0), 145);
    }

    #[test]
    fn measured_peaks_scale_to_the_loudest_bar_at_any_count() {
        let mut levels = [0u8; PEAKS];
        levels[0] = 40;
        levels[PEAKS - 1] = 80;
        for count in [22, PEAKS, 145] {
            let heights = bar_heights(RecordingId(1), Some(Peaks(levels)), count);
            assert_eq!(heights.len(), count);
            assert_eq!(heights[count - 1], MAX_BAR);
            assert_eq!(heights[0], MIN_BAR + (MAX_BAR - MIN_BAR) / 2.0);
            assert_eq!(heights[count / 2], MIN_BAR);
            for height in &heights {
                assert!((MIN_BAR..=MAX_BAR).contains(height), "{height}");
            }
        }
        let silent = bar_heights(RecordingId(1), Some(Peaks([0; PEAKS])), 145);
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
