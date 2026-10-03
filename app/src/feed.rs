use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, IntoElement, ListAlignment, ListState, Render,
    SharedString, Subscription, Window, div, list, prelude::*, px,
};
use time::{OffsetDateTime, UtcOffset};
use tuclaw_core::grouping::{DaySection, group_by_day};
use tuclaw_core::model::{Agent, AgentId, AgentStatus, Channel, ChannelKind, Message};

use crate::composer::Composer;
use crate::message::message_row;
use crate::state::{AppState, StateEvent};
use crate::theme;

pub struct Feed {
    state: Entity<AppState>,
    list: ListState,
    items: Rc<Vec<Item>>,
    composer: Entity<Composer>,
    focus: Focus,
    _observation: Subscription,
    _events: Subscription,
}

enum Focus {
    Requested,
    Taken,
}

enum Item {
    Separator(SharedString),
    Message(Message),
}

enum Resync {
    Reset,
    Repaint,
}

enum Header {
    Channel {
        name: SharedString,
        agents: usize,
    },
    Direct {
        initials: SharedString,
        tone: usize,
        name: SharedString,
        role: SharedString,
    },
}

struct Busy {
    name: SharedString,
    task: SharedString,
}

impl Feed {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Feed {
        let observation = cx.observe(&state, |_feed, _state, cx| cx.notify());
        let events = cx.subscribe(&state, |feed, _state, event: &StateEvent, cx| match event {
            StateEvent::SelectionChanged => {
                feed.resync(Resync::Reset, cx);
                feed.refresh_placeholder(cx);
            }
            StateEvent::MessagesLoaded => {
                feed.resync(Resync::Reset, cx);
                feed.list.scroll_to_end();
            }
            StateEvent::MessageAppended => {
                feed.resync(Resync::Reset, cx);
                feed.list.scroll_to_end();
            }
            StateEvent::RunsChanged => feed.resync(Resync::Repaint, cx),
            StateEvent::SendFailed(text) => {
                let text = text.clone();
                feed.composer
                    .update(cx, |composer, cx| composer.restore(text, cx));
            }
        });
        let items = items(state.read(cx), OffsetDateTime::now_utc());
        let list = ListState::new(items.len(), ListAlignment::Bottom, px(320.));
        let placeholder = placeholder(state.read(cx));
        let sender = state.clone();
        let composer = cx.new(|cx| {
            Composer::new(
                placeholder,
                Box::new(move |body, cx| sender.update(cx, |state, cx| state.send(body, cx))),
                cx,
            )
        });
        Feed {
            state,
            list,
            items: Rc::new(items),
            composer,
            focus: Focus::Requested,
            _observation: observation,
            _events: events,
        }
    }

    fn refresh_placeholder(&mut self, cx: &mut Context<Self>) {
        let placeholder = placeholder(self.state.read(cx));
        self.composer
            .update(cx, |composer, cx| composer.set_placeholder(placeholder, cx));
    }

    fn resync(&mut self, resync: Resync, cx: &mut Context<Self>) {
        let items = items(self.state.read(cx), OffsetDateTime::now_utc());
        self.items = Rc::new(items);
        match resync {
            Resync::Reset => self.list.reset(self.items.len()),
            Resync::Repaint => {}
        }
        cx.notify();
    }

    fn body(&self) -> AnyElement {
        if self.items.is_empty() {
            return empty_state().into_any_element();
        }
        let items = self.items.clone();
        let state = self.state.clone();
        list(self.list.clone(), move |index, _window, cx| {
            let Some(item) = items.get(index) else {
                return div().into_any_element();
            };
            match item {
                Item::Separator(title) => day_separator(title.clone()).into_any_element(),
                Item::Message(message) => {
                    message_row(message, state.read(cx).agents()).into_any_element()
                }
            }
        })
        .flex_1()
        .min_h(px(0.))
        .into_any_element()
    }
}

impl Render for Feed {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.focus {
            Focus::Requested => {
                let focus = self.composer.read(cx).focus_handle(cx);
                focus.focus(window, cx);
                self.focus = Focus::Taken;
            }
            Focus::Taken => {}
        }
        let state = self.state.read(cx);
        let header = header(state);
        let agents = state.agents();
        let busy = busy_agents(agents);
        let total = agents.len();
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(header_element(header))
            .child(self.body())
            .child(self.composer.clone())
            .child(status_bar(busy, total))
    }
}

fn items(state: &AppState, now: OffsetDateTime) -> Vec<Item> {
    let sections = group_by_day(state.messages(), UtcOffset::UTC, now);
    let mut items = Vec::new();
    for DaySection {
        date: _,
        title,
        messages,
    } in sections
    {
        items.push(Item::Separator(SharedString::from(title)));
        for message in messages {
            items.push(Item::Message(message));
        }
    }
    items
}

fn header(state: &AppState) -> Header {
    let Some(selected) = state.selected() else {
        return Header::Channel {
            name: SharedString::new_static(""),
            agents: 0,
        };
    };
    let mut found = None;
    for channel in state.channels() {
        if channel.id == selected {
            found = Some(channel);
            break;
        }
    }
    let Some(Channel {
        id: _,
        name,
        group: _,
        kind,
        unread: _,
        sort_index: _,
    }) = found
    else {
        return Header::Channel {
            name: SharedString::new_static(""),
            agents: 0,
        };
    };
    match kind {
        ChannelKind::Channel => Header::Channel {
            name: SharedString::from(name.clone()),
            agents: state.wired_agents(selected),
        },
        ChannelKind::Direct(agent) => direct_header(state.agents(), *agent, name),
    }
}

fn direct_header(agents: &[Agent], agent: AgentId, channel: &str) -> Header {
    let mut found = None;
    for candidate in agents {
        if candidate.id == agent {
            found = Some(candidate);
            break;
        }
    }
    let Some(Agent {
        id: _,
        name,
        initials,
        role,
        status: _,
        sort_index,
    }) = found
    else {
        return Header::Channel {
            name: SharedString::from(channel.to_string()),
            agents: 0,
        };
    };
    Header::Direct {
        initials: SharedString::from(initials.clone()),
        tone: *sort_index as usize,
        name: SharedString::from(name.clone()),
        role: SharedString::from(role.clone()),
    }
}

fn placeholder(state: &AppState) -> SharedString {
    match header(state) {
        Header::Channel { name, agents: _ } => SharedString::from(format!("Message #{name}")),
        Header::Direct {
            initials: _,
            tone: _,
            name,
            role: _,
        } => SharedString::from(format!("Message {name}")),
    }
}

fn agent_count(agents: usize) -> String {
    match agents {
        1 => "1 agent".to_string(),
        count => format!("{count} agents"),
    }
}

fn busy_agents(agents: &[Agent]) -> Vec<Busy> {
    let mut busy = Vec::new();
    for Agent {
        id: _,
        name,
        initials: _,
        role: _,
        status,
        sort_index: _,
    } in agents
    {
        let AgentStatus::Busy(task) = status else {
            continue;
        };
        busy.push(Busy {
            name: SharedString::from(name.clone()),
            task: SharedString::from(task.clone()),
        });
    }
    busy
}

fn header_element(header: Header) -> impl IntoElement {
    let lead = match header {
        Header::Channel { name, agents } => div()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_w(px(0.))
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(theme::text_label())
                    .child("#"),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(agent_count(agents)),
            ),
        Header::Direct {
            initials,
            tone,
            name,
            role,
        } => div()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_w(px(0.))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(26.))
                    .h(px(26.))
                    .rounded(px(8.))
                    .bg(theme::agent_chip(tone))
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::chip_text())
                    .child(initials),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(role),
            ),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(px(52.))
        .px(px(20.))
        .border_b_1()
        .border_color(theme::hairline())
        .child(lead)
        .child(div().flex_1())
        .child(
            chip()
                .text_size(px(14.))
                .text_color(theme::text_secondary())
                .child("···"),
        )
}

fn chip() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(30.))
        .h(px(26.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border())
}

fn day_separator(title: SharedString) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(20.))
        .pt(px(18.))
        .pb(px(10.))
        .child(rule())
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_label())
                .child(title),
        )
        .child(rule())
}

fn rule() -> Div {
    div().flex_1().h(px(1.)).bg(theme::hairline())
}

fn empty_state() -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_secondary())
                .child("Nothing here yet"),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::text_muted())
                .child("Say something to start this conversation."),
        )
}

fn status_bar(busy: Vec<Busy>, total: usize) -> impl IntoElement {
    let count = busy.len();
    let mut left = div().flex().items_center().gap(px(14.)).min_w(px(0.));
    for Busy { name, task } in busy {
        left = left.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(theme::status_busy()),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_secondary())
                        .child(name),
                )
                .child(div().text_color(theme::text_muted()).child(task)),
        );
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(px(30.))
        .px(px(20.))
        .border_t_1()
        .border_color(theme::hairline())
        .text_size(px(11.5))
        .child(left)
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .text_color(theme::text_muted())
                .child(format!("{count} of {total} agents busy")),
        )
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{Agent, AgentId, AgentStatus, Author, Span};
    use tuclaw_core::v3::MockTransport;

    use super::{Busy, Feed, Header, busy_agents, header};
    use crate::state::AppState;
    use crate::testing::{channel_named, loaded, play};

    fn agent(id: i64, status: AgentStatus) -> Agent {
        Agent {
            id: AgentId(id),
            name: format!("agent {id}"),
            initials: "AG".to_string(),
            role: "role".to_string(),
            status,
            sort_index: id,
        }
    }

    fn feed(
        cx: &mut TestAppContext,
    ) -> (
        MockTransport,
        Entity<AppState>,
        Entity<Feed>,
        &mut VisualTestContext,
    ) {
        let (mock, state) = loaded(cx);
        let built = state.clone();
        let (feed, cx) = cx.add_window_view(move |_window, cx| Feed::new(built, cx));
        (mock, state, feed, cx)
    }

    fn typed(feed: &Entity<Feed>, cx: &mut VisualTestContext) -> String {
        feed.read_with(cx, |feed, cx| feed.composer.read(cx).text(cx).to_string())
    }

    #[test]
    fn the_status_bar_lists_only_the_busy_agents() {
        let agents = vec![
            agent(1, AgentStatus::Busy("Syncing subtitles".to_string())),
            agent(2, AgentStatus::Idle),
            agent(3, AgentStatus::Busy("Downloading".to_string())),
        ];
        let busy = busy_agents(&agents);
        let mut named = Vec::new();
        for Busy { name, task } in &busy {
            named.push((name.clone(), task.clone()));
        }
        assert_eq!(
            named,
            vec![
                (
                    SharedString::new_static("agent 1"),
                    SharedString::new_static("Syncing subtitles")
                ),
                (
                    SharedString::new_static("agent 3"),
                    SharedString::new_static("Downloading")
                ),
            ]
        );
        assert!(busy_agents(&[agent(4, AgentStatus::Idle)]).is_empty());
    }

    #[test]
    fn the_agent_count_agrees_in_number() {
        assert_eq!(super::agent_count(0), "0 agents");
        assert_eq!(super::agent_count(1), "1 agent");
        assert_eq!(super::agent_count(4), "4 agents");
    }

    #[gpui::test]
    fn the_first_surface_is_drawn_with_its_history(cx: &mut TestAppContext) {
        let (_mock, state, feed, cx) = feed(cx);
        state.read_with(cx, |state, _cx| assert_eq!(state.messages().len(), 30));
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), feed.items.len());
            assert_eq!(feed.items.len(), 31);
        });
    }

    #[gpui::test]
    fn the_header_counts_the_wired_agents(cx: &mut TestAppContext) {
        let (_mock, state, _feed, cx) = feed(cx);
        let Header::Channel { name, agents } = state.read_with(cx, |state, _cx| header(state))
        else {
            panic!("a surface draws a channel header");
        };
        assert_eq!(name.as_ref(), "General");
        assert_eq!(agents, 2);
    }

    #[gpui::test]
    fn selecting_another_surface_resets_the_list(cx: &mut TestAppContext) {
        let (_mock, state, feed, cx) = feed(cx);
        let home = channel_named(&state, cx, "Smart Home");
        state.update(cx, |state, cx| state.select(home, cx));
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.items.len(), 13);
            assert_eq!(feed.list.item_count(), feed.items.len());
        });
    }

    #[gpui::test]
    fn a_sent_message_is_answered_over_the_socket(cx: &mut TestAppContext) {
        let (mock, state, feed, cx) = feed(cx);
        state.update(cx, |state, cx| {
            state
                .send("Лисички появились, что приготовить?".to_string(), cx)
                .expect("the post is queued")
        });
        cx.run_until_parked();
        play(&mock, cx);
        state.read_with(cx, |state, _cx| {
            let messages = state.messages();
            assert_eq!(messages.len(), 32);
            let question = &messages[30];
            assert_eq!(question.author, Author::User);
            assert!(question.id.0 > 0, "the optimistic row took the daemon's id");
            assert_eq!(
                question.body,
                vec![Span::Text(
                    "Лисички появились, что приготовить?".to_string()
                )]
            );
            let answer = &messages[31];
            assert_eq!(answer.author, Author::Agent(AgentId(1)));
        });
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), feed.items.len());
        });
    }

    #[gpui::test]
    fn a_failed_post_hands_the_text_back(cx: &mut TestAppContext) {
        let (mock, state, feed, cx) = feed(cx);
        mock.fail_next_call();
        state.update(cx, |state, cx| {
            state
                .send("не дойдёт".to_string(), cx)
                .expect("the post is queued")
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| assert_eq!(state.messages().len(), 30));
        assert_eq!(typed(&feed, cx), "не дойдёт");
    }
}
