use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, IntoElement, ListAlignment, ListState, Render,
    SharedString, Subscription, Window, div, list, prelude::*, px,
};
use time::{OffsetDateTime, UtcOffset};
use tuclaw_core::grouping::{DaySection, group_by_day};
use tuclaw_core::model::{Agent, AgentId, AgentStatus, Author, Channel, ChannelKind, Message};

use crate::composer::{Composer, ComposerKind};
use crate::message::{OnOpen, Replies, message_row};
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
            StateEvent::MessageAppended => {
                feed.resync(Resync::Reset, cx);
                feed.list.scroll_to_end();
            }
            StateEvent::ReplyAppended => feed.resync(Resync::Repaint, cx),
            StateEvent::ThreadOpened => {}
            StateEvent::ThreadClosed => {
                feed.focus = Focus::Requested;
                cx.notify();
            }
        });
        let items = items(state.read(cx), OffsetDateTime::now_utc());
        let list = ListState::new(items.len(), ListAlignment::Bottom, px(320.));
        let placeholder = placeholder(state.read(cx));
        let sender = state.clone();
        let composer = cx.new(|cx| {
            Composer::new(
                ComposerKind::Feed,
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

    #[cfg(test)]
    pub fn input_focus(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.composer.read(cx).focus_handle(cx)
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
        let opener = self.state.clone();
        let on_open: OnOpen = Rc::new(move |root, _window, cx| {
            opener.update(cx, |state, cx| state.open_thread(root, cx));
        });
        list(self.list.clone(), move |index, _window, cx| {
            let Some(item) = items.get(index) else {
                return div().into_any_element();
            };
            match item {
                Item::Separator(title) => day_separator(title.clone()).into_any_element(),
                Item::Message(message) => message_row(
                    message,
                    state.read(cx).agents(),
                    Replies::Affordance(on_open.clone()),
                )
                .into_any_element(),
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
    let selected = state.selected();
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
            agents: agent_authors(state.messages()),
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

fn agent_authors(messages: &[Message]) -> usize {
    let mut seen: Vec<AgentId> = Vec::new();
    for Message {
        id: _,
        author,
        body: _,
        sent_at: _,
        reply_count: _,
    } in messages
    {
        let Author::Agent(agent) = author else {
            continue;
        };
        if !seen.contains(agent) {
            seen.push(*agent);
        }
    }
    seen.len()
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
        .child(chip().child(thread_glyph()))
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

fn thread_glyph() -> impl IntoElement {
    div()
        .w(px(13.))
        .h(px(11.))
        .rounded(px(3.))
        .border_1()
        .border_color(theme::text_secondary())
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
    use gpui::{
        AppContext, Entity, Modifiers, SharedString, TestAppContext, VisualTestContext, px,
    };
    use time::macros::datetime;
    use tuclaw_core::model::{
        Agent, AgentId, AgentStatus, Author, ChannelId, ChannelKind, Message, MessageId, Span,
    };
    use tuclaw_core::store::Store;

    use super::{Busy, Feed, Item, agent_authors, busy_agents};
    use crate::state::AppState;

    fn from(author: Author, id: i64) -> Message {
        Message {
            id: MessageId(id),
            author,
            body: vec![Span::Text("hi".to_string())],
            sent_at: datetime!(2026-08-26 09:00 UTC),
            reply_count: 0,
        }
    }

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

    fn feed(cx: &mut TestAppContext) -> (Entity<AppState>, Entity<Feed>, &mut VisualTestContext) {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        let state = AppState::new(store).expect("the workspace loads");
        let state = cx.new(|_| state);
        let built = state.clone();
        let (feed, cx) = cx.add_window_view(move |_window, cx| Feed::new(built, cx));
        (state, feed, cx)
    }

    fn channel_named(state: &Entity<AppState>, cx: &mut TestAppContext, name: &str) -> ChannelId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for channel in state.channels() {
                if channel.name == name {
                    found = Some(channel.id);
                    break;
                }
            }
            found.expect("the fixtures carry that channel")
        })
    }

    fn first_direct(state: &Entity<AppState>, cx: &mut TestAppContext) -> ChannelId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for channel in state.channels() {
                match channel.kind {
                    ChannelKind::Channel => {}
                    ChannelKind::Direct(_) => {
                        found = Some(channel.id);
                        break;
                    }
                }
            }
            found.expect("the fixtures carry a direct channel")
        })
    }

    #[test]
    fn the_header_counts_each_agent_once_and_skips_the_user() {
        let messages = vec![
            from(Author::Agent(AgentId(2)), 1),
            from(Author::User, 2),
            from(Author::Agent(AgentId(2)), 3),
            from(Author::Agent(AgentId(5)), 4),
        ];
        assert_eq!(agent_authors(&messages), 2);
        assert_eq!(agent_authors(&[]), 0);
        assert_eq!(agent_authors(&[from(Author::User, 1)]), 0);
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

    #[gpui::test]
    fn drawing_the_busiest_channel_does_not_panic(cx: &mut TestAppContext) {
        let (state, feed, cx) = feed(cx);
        state.read_with(cx, |state, _cx| assert_eq!(state.messages().len(), 58));
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), feed.items.len());
            assert_eq!(feed.items.len(), 58 + 15);
        });
    }

    #[gpui::test]
    fn selecting_the_empty_channel_empties_the_list(cx: &mut TestAppContext) {
        let (state, feed, cx) = feed(cx);
        let personal = channel_named(&state, cx, "personal");
        state.update(cx, |state, cx| state.select(personal, cx));
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), 0);
            assert!(feed.items.is_empty());
        });
    }

    #[gpui::test]
    fn selecting_a_direct_channel_draws_its_agent(cx: &mut TestAppContext) {
        let (state, feed, cx) = feed(cx);
        let direct = first_direct(&state, cx);
        state.update(cx, |state, cx| state.select(direct, cx));
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            assert!(!feed.items.is_empty());
            assert_eq!(feed.list.item_count(), feed.items.len());
        });
    }

    #[gpui::test]
    fn the_hover_reply_sits_at_the_right_edge_of_its_row(cx: &mut TestAppContext) {
        let (state, _feed, cx) = feed(cx);
        let plain = state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_count == 0 {
                    found = Some(message.id);
                }
            }
            found.expect("movie-night carries a message without replies")
        });
        let MessageId(raw) = plain;
        let selector: &'static str = format!("message-reply-{raw}").leak();
        let affordance = cx
            .debug_bounds(selector)
            .expect("the hover reply is laid out");
        let width = cx.update(|window, _cx| window.viewport_size().width);
        assert!(
            affordance.right() > width - px(60.),
            "the hover reply ends at {:?} in a {:?} wide feed",
            affordance.right(),
            width
        );
    }

    #[gpui::test]
    fn clicking_the_reply_affordance_opens_that_thread(cx: &mut TestAppContext) {
        let (state, _feed, cx) = feed(cx);
        let root = state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_count > 0 {
                    found = Some(message.id);
                    break;
                }
            }
            found.expect("movie-night carries a thread root")
        });
        let MessageId(raw) = root;
        let selector: &'static str = format!("message-reply-{raw}").leak();
        let affordance = cx
            .debug_bounds(selector)
            .expect("the replies affordance is drawn");
        cx.simulate_click(affordance.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.root.id, root);
            assert_eq!(thread.replies.len(), 4);
        });
    }

    #[gpui::test]
    fn a_reply_keeps_the_list_length_and_raises_the_root_count(cx: &mut TestAppContext) {
        let (state, feed, cx) = feed(cx);
        let root = state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_count > 0 {
                    found = Some(message.id);
                    break;
                }
            }
            found.expect("movie-night carries a thread root")
        });
        state.update(cx, |state, cx| state.open_thread(root, cx));
        cx.run_until_parked();
        let before = feed.read_with(cx, |feed, _cx| feed.items.len());
        state.update(cx, |state, cx| {
            state
                .reply_in_thread("me too".to_string(), cx)
                .expect("the reply is written")
        });
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.items.len(), before);
            assert_eq!(feed.list.item_count(), feed.items.len());
            let mut counted = None;
            for item in feed.items.iter() {
                match item {
                    Item::Message(message) => {
                        if message.id == root {
                            counted = Some(message.reply_count);
                        }
                    }
                    Item::Separator(_) => {}
                }
            }
            assert_eq!(counted, Some(5));
        });
    }

    #[gpui::test]
    fn sending_a_message_grows_the_list_and_resyncs_it(cx: &mut TestAppContext) {
        let (state, feed, cx) = feed(cx);
        let before = feed.read_with(cx, |feed, _cx| feed.items.len());
        state.update(cx, |state, cx| {
            state.send("on it".to_string(), cx).expect("it is written")
        });
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            assert!(feed.items.len() > before);
            assert_eq!(feed.list.item_count(), feed.items.len());
            match feed.items.last() {
                Some(Item::Message(message)) => {
                    assert_eq!(message.body, vec![Span::Text("on it".to_string())])
                }
                Some(Item::Separator(_)) => panic!("the sent message is the last item"),
                None => panic!("the sent message is the last item"),
            }
        });
    }

    #[test]
    fn the_agent_count_agrees_in_number() {
        assert_eq!(super::agent_count(0), "0 agents");
        assert_eq!(super::agent_count(1), "1 agent");
        assert_eq!(super::agent_count(4), "4 agents");
    }
}
