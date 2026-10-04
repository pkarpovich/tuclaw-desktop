use gpui::{
    Context, Div, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use gpui_kit::base::Button;
use tuclaw_core::model::{Agent, AgentId, AgentStatus, Channel, ChannelId, ChannelKind};

use crate::control::{AvatarSize, avatar, row_button};
use crate::icon::{Glyph, icon};
use crate::state::{AppState, Segment, View};
use crate::theme;

const DIRECT_MESSAGES: &str = "Direct messages";

pub struct Sidebar {
    state: Entity<AppState>,
    _observation: Subscription,
}

struct Section {
    title: Option<SharedString>,
    rows: Vec<Row>,
}

struct Row {
    channel: ChannelId,
    name: SharedString,
    lead: Lead,
    unread: usize,
    highlight: Highlight,
}

#[derive(PartialEq, Eq)]
enum Highlight {
    On,
    Off,
}

enum Lead {
    Hash,
    Chip {
        initials: SharedString,
        tone: usize,
        status: Status,
    },
}

enum Status {
    Idle,
    Busy,
}

impl Sidebar {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Sidebar {
        let observation = cx.observe(&state, |_sidebar, _state, cx| cx.notify());
        Sidebar {
            state,
            _observation: observation,
        }
    }

    fn agents_row(
        &self,
        agents: usize,
        highlight: Highlight,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row = row_frame("sidebar-agents")
            .py(px(6.))
            .on_click(cx.listener(|sidebar, _event, _window, cx| {
                sidebar
                    .state
                    .update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
            }))
            .child(icon(Glyph::Agents, px(16.), theme::text_secondary()))
            .child(
                div()
                    .font_weight(match highlight {
                        Highlight::On => FontWeight::SEMIBOLD,
                        Highlight::Off => FontWeight::NORMAL,
                    })
                    .child("Agents"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme::text_muted())
                    .child(agents.to_string()),
            );
        match highlight {
            Highlight::On => row.bg(theme::selection()),
            Highlight::Off => row,
        }
    }

    fn channel_row(&self, row: Row, cx: &mut Context<Self>) -> impl IntoElement {
        let Row {
            channel,
            name,
            lead,
            unread,
            highlight,
        } = row;
        let selector = format!("sidebar-row-{name}");
        let element = row_frame(selector)
            .py(px(5.))
            .on_click(cx.listener(move |sidebar, _event, _window, cx| {
                sidebar
                    .state
                    .update(cx, |state, cx| state.select(channel, cx));
            }))
            .child(lead_element(lead))
            .child(
                div()
                    .font_weight(if unread > 0 {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if unread > 0 {
                        theme::text_primary()
                    } else {
                        theme::text_secondary()
                    })
                    .child(name),
            )
            .child(div().flex_1());
        let element = if unread > 0 {
            element.child(unread_badge(unread))
        } else {
            element
        };
        match highlight {
            Highlight::On => element.bg(theme::selection()),
            Highlight::Off => element,
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let agents = state.agents().len();
        let on_agents = match state.view() {
            View::Agents => Highlight::On,
            View::Conversation => Highlight::Off,
        };
        let sections = sections(state);
        let mut rows = div()
            .id("sidebar-rows")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(6.))
            .pt(px(2.))
            .pb(px(14.))
            .child(self.agents_row(agents, on_agents, cx));
        for Section { title, rows: group } in sections {
            if let Some(title) = title {
                rows = rows.child(section_title(title));
            }
            for row in group {
                rows = rows.child(self.channel_row(row, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(search_field())
            .child(rows)
            .child(footer())
    }
}

fn sections(state: &AppState) -> Vec<Section> {
    let selected = match state.view() {
        View::Conversation => state.selected(),
        View::Agents => None,
    };
    let agents = state.agents();
    let mut sections: Vec<Section> = Vec::new();
    for Channel {
        id,
        name,
        group,
        kind,
        unread,
        sort_index: _,
    } in state.channels()
    {
        let title = match kind {
            ChannelKind::Channel => group.clone().map(SharedString::from),
            ChannelKind::Direct(_) => Some(SharedString::new_static(DIRECT_MESSAGES)),
        };
        let lead = match kind {
            ChannelKind::Channel => Lead::Hash,
            ChannelKind::Direct(agent) => lead_of(agents, *agent),
        };
        let continues = match sections.last() {
            Some(Section {
                title: open,
                rows: _,
            }) => *open == title,
            None => false,
        };
        if !continues {
            sections.push(Section {
                title,
                rows: Vec::new(),
            });
        }
        let Some(Section { title: _, rows }) = sections.last_mut() else {
            continue;
        };
        let highlight = if Some(*id) == selected {
            Highlight::On
        } else {
            Highlight::Off
        };
        rows.push(Row {
            channel: *id,
            name: SharedString::from(name.clone()),
            lead,
            unread: *unread,
            highlight,
        });
    }
    sections
}

fn lead_of(agents: &[Agent], agent: AgentId) -> Lead {
    let mut found = None;
    for candidate in agents {
        if candidate.id == agent {
            found = Some(candidate);
            break;
        }
    }
    let Some(Agent {
        id: _,
        name: _,
        initials,
        role: _,
        status,
        sort_index,
    }) = found
    else {
        return Lead::Hash;
    };
    let status = match status {
        AgentStatus::Idle => Status::Idle,
        AgentStatus::Busy(_) => Status::Busy,
    };
    Lead::Chip {
        initials: SharedString::from(initials.clone()),
        tone: *sort_index as usize,
        status,
    }
}

fn row_frame(selector: impl Into<SharedString>) -> Button {
    row_button(selector)
        .gap(px(10.))
        .px(px(10.))
        .rounded(px(8.))
        .text_size(px(13.5))
        .hover(|style| style.bg(theme::sunken()))
}

fn lead_element(lead: Lead) -> Div {
    match lead {
        Lead::Hash => div()
            .w(px(16.))
            .flex_none()
            .flex()
            .justify_center()
            .child(icon(Glyph::Channel, px(14.), theme::text_label())),
        Lead::Chip {
            initials,
            tone,
            status,
        } => div()
            .relative()
            .flex_none()
            .child(avatar(initials, theme::agent_chip(tone), AvatarSize::Row))
            .child(status_dot(status)),
    }
}

fn status_dot(status: Status) -> Div {
    let tone = match status {
        Status::Idle => theme::status_idle(),
        Status::Busy => theme::status_busy(),
    };
    div()
        .absolute()
        .right(px(-3.))
        .bottom(px(-3.))
        .w(px(9.))
        .h(px(9.))
        .rounded_full()
        .bg(tone)
        .border_2()
        .border_color(theme::window())
}

fn unread_badge(unread: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(19.))
        .h(px(19.))
        .px(px(6.))
        .rounded_full()
        .bg(theme::badge())
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::chip_text())
        .child(unread.to_string())
}

fn section_title(title: SharedString) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .pt(px(18.))
        .px(px(10.))
        .pb(px(6.))
        .text_size(px(12.))
        .text_color(theme::text_label())
        .child(title)
}

fn search_field() -> impl IntoElement {
    div().flex_none().px(px(6.)).pb(px(8.)).child(
        div()
            .id("sidebar-search")
            .debug_selector(|| "sidebar-search".to_string())
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .px(px(10.))
            .rounded(px(9.))
            .bg(theme::field())
            .border_1()
            .border_color(theme::border())
            .cursor_pointer()
            .child(icon(Glyph::Search, px(14.), theme::text_label()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_label())
                    .child("Search everything"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("⌘K"),
            ),
    )
}

fn footer() -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .pt(px(10.))
        .px(px(12.))
        .pb(px(12.))
        .child(
            div()
                .relative()
                .flex_none()
                .child(avatar("YO", theme::accent(), AvatarSize::Account))
                .child(status_dot(Status::Idle)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("You"),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme::text_label())
                        .child("the only human here"),
                ),
        )
        .child(div().flex_1())
        .child(gear())
}

fn gear() -> impl IntoElement {
    div().flex_none().cursor_pointer().child(icon(
        Glyph::Settings,
        px(15.),
        theme::text_secondary(),
    ))
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

    use super::{Highlight, Lead, Section, Sidebar, sections};
    use crate::state::{AppState, Segment};
    use crate::testing::loaded;

    fn sidebar(cx: &mut TestAppContext) -> (Entity<AppState>, &mut VisualTestContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_sidebar, cx) = cx.add_window_view(move |_window, cx| Sidebar::new(built, cx));
        (state, cx)
    }

    #[gpui::test]
    fn drawing_the_sidebar_does_not_panic(cx: &mut TestAppContext) {
        let (state, cx) = sidebar(cx);
        state.read_with(cx, |state, _cx| assert_eq!(state.channels().len(), 3));
    }

    #[gpui::test]
    fn the_surfaces_form_one_untitled_section_in_order(cx: &mut TestAppContext) {
        let (state, cx) = sidebar(cx);
        state.read_with(cx, |state, _cx| {
            let sections = sections(state);
            assert_eq!(sections.len(), 1);
            let Some(Section { title, rows }) = sections.first() else {
                panic!("the section is built");
            };
            assert_eq!(*title, None);
            let mut names = Vec::new();
            for row in rows {
                names.push(row.name.to_string());
                match &row.lead {
                    Lead::Hash => {}
                    Lead::Chip {
                        initials: _,
                        tone: _,
                        status: _,
                    } => panic!("a surface row carries a hash"),
                }
            }
            assert_eq!(names, vec!["General", "Magnet Feed", "Smart Home"]);
            assert!(rows[0].highlight == Highlight::On);
            assert!(rows[1].highlight == Highlight::Off);
        });
    }

    #[gpui::test]
    fn the_agents_view_highlights_no_channel_row(cx: &mut TestAppContext) {
        let (state, cx) = sidebar(cx);
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.read_with(cx, |state, _cx| {
            for Section { title: _, rows } in sections(state) {
                for row in rows {
                    assert!(
                        row.highlight == Highlight::Off,
                        "{} is highlighted",
                        row.name
                    );
                }
            }
        });
    }

    #[gpui::test]
    fn clicking_a_channel_row_selects_it_and_loads_its_history(cx: &mut TestAppContext) {
        let (state, cx) = sidebar(cx);
        let home = cx
            .debug_bounds("sidebar-row-Smart Home")
            .expect("the Smart Home row is drawn");
        cx.simulate_click(home.center(), Modifiers::default());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let mut name = None;
            for channel in state.channels() {
                if Some(channel.id) == state.selected() {
                    name = Some(channel.name.clone());
                    break;
                }
            }
            assert_eq!(name, Some("Smart Home".to_string()));
            assert_eq!(state.messages().len(), 12);
        });
    }

    #[gpui::test]
    fn clicking_the_agents_row_activates_the_segment(cx: &mut TestAppContext) {
        let (state, cx) = sidebar(cx);
        let agents = cx
            .debug_bounds("sidebar-agents")
            .expect("the agents row is drawn");
        cx.simulate_click(agents.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Agents)
        });
    }
}
