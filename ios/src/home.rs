use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, Focusable, FontWeight, Hsla, IntoElement, MouseButton,
    Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::input::{Input, InputState};
use time::OffsetDateTime;
use tuclaw_core::model::{Author, Channel, ChannelId, ChannelKind};
use tuclaw_core::v3::{Run, StepKind};
use tuclaw_desktop::badge::{self, Indicator};
use tuclaw_desktop::chrome::on_long_press;
use tuclaw_desktop::control::{AvatarShape, AvatarSize, Face, avatar, shaped_avatar};
use tuclaw_desktop::icon::{Glyph, icon, spinner};
use tuclaw_desktop::local;
use tuclaw_desktop::message::writer;
use tuclaw_desktop::people::People;
use tuclaw_desktop::runlog::short_name;
use tuclaw_desktop::state::{AppState, Link};
use tuclaw_desktop::{link, theme};

use crate::frame;
use crate::keyboard;
use crate::navigator::{Menu, Navigator, Tab};

const UNGROUPED: &str = "Channels";
pub const TAB_BAR_HEIGHT: f32 = 62.;

pub struct Home {
    state: Entity<AppState>,
    navigator: Entity<Navigator>,
    search: Entity<InputState>,
    _observation: Subscription,
    _search: Subscription,
    _keyboard: Subscription,
}

struct Row {
    channel: ChannelId,
    name: SharedString,
    indicator: Indicator,
    attention: bool,
    working: Vec<String>,
    preview: Option<Line>,
    time: Option<SharedString>,
}

struct Line {
    author: SharedString,
    text: SharedString,
}

struct Section {
    title: SharedString,
    rows: Vec<Row>,
}

struct Running {
    channel: ChannelId,
    face: Face,
    agent: SharedString,
    doing: SharedString,
}

impl Home {
    pub fn new(
        state: Entity<AppState>,
        navigator: Entity<Navigator>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Home {
        let observation = cx.observe(&state, |_home, _state, cx| cx.notify());
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let searching = cx.observe(&search, |_home, _search, cx| cx.notify());
        let focus = search.read(cx).focus_handle(cx);
        let keyboard = keyboard::hide_on_blur(&focus, window, cx);
        Home {
            state,
            navigator,
            search,
            _observation: observation,
            _search: searching,
            _keyboard: keyboard,
        }
    }

    fn search_field(&self) -> impl IntoElement {
        div()
            .id("home-search")
            .debug_selector(|| "home-search".to_string())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .mx(px(18.))
            .mb(px(6.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(11.))
            .bg(theme::sunken())
            .text_size(px(15.))
            .on_mouse_up(MouseButton::Left, |_event, _window, _cx| keyboard::show())
            .child(icon(Glyph::Search, px(15.), theme::text_label()))
            .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.search)))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let status = link_status(state.link());
        let people = state.people();
        let me = Face {
            initials: SharedString::from(link::initials(&people.me.name)),
            color: theme::accent(),
            picture: people.picture(people.me.picture.as_ref()),
        };
        let navigator = self.navigator.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .pt(frame::insets().top)
            .px(px(18.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(30.))
                            .font_weight(FontWeight::BOLD)
                            .child("tuclaw"),
                    )
                    .children(status.map(|status| {
                        div()
                            .id("home-link")
                            .debug_selector(|| "home-link".to_string())
                            .text_size(px(12.5))
                            .text_color(theme::text_label())
                            .child(status)
                    })),
            )
            .child(div().flex_1())
            .child({
                let navigator = self.navigator.clone();
                div()
                    .id("home-channels")
                    .debug_selector(|| "home-channels".to_string())
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(34.))
                    .rounded_full()
                    .bg(theme::sunken())
                    .on_click(move |_event, _window, cx| {
                        navigator.update(cx, |navigator, cx| navigator.open_channels(cx))
                    })
                    .child(icon(Glyph::Rename, px(17.), theme::ink_soft()))
            })
            .child(
                div()
                    .id("home-me")
                    .debug_selector(|| "home-me".to_string())
                    .rounded_full()
                    .overflow_hidden()
                    .on_click(move |_event, _window, cx| {
                        navigator.update(cx, |navigator, cx| navigator.switch(Tab::You, cx))
                    })
                    .child(shaped_avatar(me, AvatarSize::Pocket, AvatarShape::Round)),
            )
    }

    fn running(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let state = self.state.read(cx);
        let running = running_rows(state);
        if running.is_empty() {
            return None;
        }
        let count = running.len();
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(px(14.))
            .bg(theme::voice_card())
            .border_1()
            .border_color(theme::hairline())
            .overflow_hidden();
        for (index, row) in running.into_iter().enumerate() {
            let navigator = self.navigator.clone();
            let Running {
                channel,
                face,
                agent,
                doing,
            } = row;
            let ChannelId(raw) = channel;
            let mut line = div()
                .id(SharedString::from(format!("home-running-{raw}-{index}")))
                .debug_selector(move || format!("home-running-{raw}"))
                .flex()
                .items_center()
                .gap(px(11.))
                .px(px(13.))
                .py(px(11.))
                .on_click(move |_event, _window, cx| {
                    navigator.update(cx, |navigator, cx| navigator.open_channel(channel, cx))
                })
                .child(avatar(face, AvatarSize::Pocket))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(14.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(agent),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme::text_label())
                                .truncate()
                                .child(doing),
                        ),
                )
                .child(spinner(px(16.), theme::accent()));
            if index + 1 < count {
                line = line.border_b_1().border_color(theme::hairline());
            }
            card = card.child(line);
        }
        Some(
            div()
                .flex()
                .flex_col()
                .pb(px(18.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .px(px(6.))
                        .pt(px(8.))
                        .pb(px(6.))
                        .child(div().size(px(7.)).rounded_full().bg(theme::accent()))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::accent())
                                .child("Running now"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::text_muted())
                                .child(SharedString::from(count.to_string())),
                        ),
                )
                .child(card),
        )
    }

    fn row(&self, row: Row) -> Stateful<Div> {
        let Row {
            channel,
            name,
            indicator,
            attention,
            working,
            preview,
            time,
        } = row;
        let ChannelId(raw) = channel;
        let selector = format!("home-row-{name}");
        let opener = self.navigator.clone();
        let presser = self.navigator.clone();
        let (name_weight, name_color, line_color) = if attention {
            (
                FontWeight::SEMIBOLD,
                theme::text_primary(),
                theme::ink_soft(),
            )
        } else {
            (FontWeight::NORMAL, theme::ink_soft(), theme::text_label())
        };
        let second = match working.first() {
            Some(agent) => div()
                .text_size(px(14.))
                .text_color(theme::accent())
                .truncate()
                .child(SharedString::from(format!("{agent} is working…"))),
            None => match preview {
                Some(Line { author, text }) => div()
                    .flex()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .text_color(line_color)
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(format!("{author}: "))),
                    )
                    .child(div().min_w(px(0.)).truncate().child(text)),
                None => div(),
            },
        };
        let element = div()
            .id(SharedString::from(format!("home-row-{raw}")))
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(6.))
            .py(px(9.))
            .on_click(move |_event, _window, cx| {
                opener.update(cx, |navigator, cx| navigator.open_channel(channel, cx))
            })
            .child(tile(attention))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(1.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(15.5))
                                    .font_weight(name_weight)
                                    .text_color(name_color)
                                    .child(name),
                            )
                            .children(time.map(|time| {
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(time)
                            })),
                    )
                    .child(second),
            )
            .children(indicator_element(indicator));
        on_long_press(
            element,
            Rc::new(move |_window, cx| {
                presser.update(cx, |navigator, cx| {
                    navigator.open_menu(Menu::Channel(channel), cx)
                })
            }),
        )
    }
}

impl Render for Home {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_lowercase();
        let sections = sections(self.state.read(cx), &query, local::now());
        let running = self.running(cx);
        let mut list = div()
            .id("home-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(12.))
            .pt(px(6.))
            .pb(px(TAB_BAR_HEIGHT + 24.) + frame::insets().bottom)
            .children(running);
        for Section { title, rows } in sections {
            list = list.child(
                div()
                    .px(px(6.))
                    .pt(px(14.))
                    .pb(px(8.))
                    .text_size(px(12.5))
                    .text_color(theme::text_label())
                    .child(title),
            );
            for row in rows {
                list = list.child(self.row(row));
            }
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::card())
            .child(self.header(cx))
            .child(self.search_field())
            .child(list)
    }
}

fn link_status(link: &Link) -> Option<SharedString> {
    match link {
        Link::Live => None,
        Link::Connecting => Some(SharedString::new_static("Connecting…")),
        Link::Reconnecting => Some(SharedString::new_static("Reconnecting…")),
        Link::Failed(reason) => Some(SharedString::from(format!("Offline: {reason}"))),
    }
}

fn tile(attention: bool) -> Div {
    let color = if attention {
        theme::ink_soft()
    } else {
        theme::text_label()
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(44.))
        .rounded(px(13.))
        .bg(theme::sunken())
        .child(icon(Glyph::Channel, px(19.), color))
}

fn indicator_element(indicator: Indicator) -> Option<AnyElement> {
    match indicator {
        Indicator::Replies(count) => Some(count_badge(count, theme::accent())),
        Indicator::Unread(count) => Some(count_badge(count, theme::accent())),
        Indicator::Activity(count) => Some(count_badge(count, theme::activity())),
        Indicator::Marked => Some(
            div()
                .flex_none()
                .size(px(10.))
                .rounded_full()
                .bg(theme::accent())
                .into_any_element(),
        ),
        Indicator::Nothing => None,
    }
}

fn count_badge(count: usize, color: Hsla) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(21.))
        .h(px(21.))
        .px(px(6.))
        .rounded_full()
        .bg(color)
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::chip_text())
        .child(SharedString::from(count.to_string()))
        .into_any_element()
}

fn sections(state: &AppState, query: &str, now: OffsetDateTime) -> Vec<Section> {
    let people = state.people();
    let mut sections: Vec<Section> = Vec::new();
    for channel in state.channels() {
        let Channel {
            id,
            name,
            group,
            kind,
            unread: _,
            replies: _,
            marked: _,
            sort_index: _,
        } = channel;
        match kind {
            ChannelKind::Channel => {}
            ChannelKind::Direct(_) => continue,
        }
        if !name.to_lowercase().contains(query) {
            continue;
        }
        let title = SharedString::from(group.clone().unwrap_or_else(|| UNGROUPED.to_string()));
        let continues = match sections.last() {
            Some(section) => section.title == title,
            None => false,
        };
        if !continues {
            sections.push(Section {
                title,
                rows: Vec::new(),
            });
        }
        let Some(section) = sections.last_mut() else {
            continue;
        };
        let preview = state.preview(*id);
        let at = match preview {
            Some(preview) => Some(preview.at),
            None => last_message_at(state, *id),
        };
        section.rows.push(Row {
            channel: *id,
            name: SharedString::from(name.clone()),
            indicator: badge::indicator(channel),
            attention: badge::needs_attention(channel),
            working: state.working(*id),
            preview: preview.map(|preview| Line {
                author: author_name(preview.author, &people),
                text: SharedString::from(preview.text.clone()),
            }),
            time: at.map(|at| SharedString::from(list_time(at, now))),
        });
    }
    sections
}

fn author_name(author: Author, people: &People) -> SharedString {
    writer(author, people).name
}

fn last_message_at(state: &AppState, channel: ChannelId) -> Option<OffsetDateTime> {
    let surface = link::surface_id(channel);
    for candidate in state.surfaces() {
        if candidate.id == surface {
            return candidate.last_message_at;
        }
    }
    None
}

fn running_rows(state: &AppState) -> Vec<Running> {
    let people = state.people();
    let mut rows = Vec::new();
    for run in state.running() {
        let Some(surface) = run.surface_id else {
            continue;
        };
        let agent = link::agent_id(run.agent_id);
        let Some(known) = people.agent(agent) else {
            continue;
        };
        let channel = link::channel_id(surface);
        let place = state.surface_name(surface).unwrap_or_default();
        rows.push(Running {
            channel,
            face: Face {
                initials: SharedString::from(known.initials.clone()),
                color: theme::agent_chip(known.sort_index as usize),
                picture: people.picture(known.picture.as_ref()),
            },
            agent: SharedString::from(known.name.clone()),
            doing: SharedString::from(format!("#{place} · {}", doing(run))),
        });
    }
    rows
}

fn doing(run: &Run) -> String {
    if !run.segment.trim().is_empty() {
        return "Writing".to_string();
    }
    let Some(step) = run.steps.last() else {
        return "Thinking".to_string();
    };
    match &step.kind {
        StepKind::Tool {
            tool_use_id: _,
            name,
            input: _,
            output: _,
            status: _,
            finished_at: _,
        } => {
            if name.is_empty() {
                "Working".to_string()
            } else {
                short_name(name)
            }
        }
        StepKind::Text { text: _ } => "Writing".to_string(),
        StepKind::Task {
            task_id: _,
            task_type: _,
            state: _,
            description,
            summary: _,
        } => description
            .clone()
            .unwrap_or_else(|| "Background task".to_string()),
        StepKind::Status { status, detail: _ } => status.clone(),
        StepKind::Other { name, output: _ } => {
            name.clone().unwrap_or_else(|| "Working".to_string())
        }
    }
}

pub fn list_time(at: OffsetDateTime, now: OffsetDateTime) -> String {
    let day = local::local(at).date();
    let today = local::local(now).date();
    let distance = (today - day).whole_days();
    match distance {
        0 => local::clock(at),
        1 => "Yesterday".to_string(),
        2..=6 => {
            let format = time::macros::format_description!("[weekday repr:long]");
            local::local(at).format(&format).unwrap_or_default()
        }
        _ => {
            let format = time::macros::format_description!("[month repr:short] [day padding:none]");
            local::local(at).format(&format).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;
    use tuclaw_desktop::local;

    use super::list_time;

    #[test]
    fn list_times_read_like_a_messenger() {
        let now = datetime!(2026-10-08 12:00 UTC);
        let earlier = datetime!(2026-10-08 11:00 UTC);
        assert_eq!(list_time(earlier, now), local::clock(earlier));
        assert_eq!(list_time(datetime!(2026-10-07 12:00 UTC), now), "Yesterday");
        assert_eq!(list_time(datetime!(2026-10-04 12:00 UTC), now), "Sunday");
        assert_eq!(list_time(datetime!(2026-09-20 12:00 UTC), now), "Sep 20");
    }
}
