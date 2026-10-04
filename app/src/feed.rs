use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, FollowMode, FontWeight, IntoElement, ListAlignment,
    ListOffset, ListScrollEvent, ListState, Render, SharedString, Subscription, Window, div, list,
    prelude::*, px,
};
use time::{OffsetDateTime, UtcOffset};
use tuclaw_core::grouping::{DaySection, group_by_day};
use tuclaw_core::model::{Agent, AgentId, AgentStatus, Channel, ChannelKind, Message};

use crate::card;
use crate::composer::Composer;
use crate::control::{AvatarSize, Face, avatar};
use crate::icon::{Glyph, icon};
use crate::live::{LiveLook, OnStop, RunView, owner, run_card, run_view};
use crate::message::{Actions, Fold, Look, OnPlay, OnToggle, message_row};
use crate::people::People;
use crate::runlog::{self, OnDisclose};
use crate::state::{AppState, History, StateEvent};

const PREFETCH: usize = 3;
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
    Run(RunView),
}

enum Resync {
    Reset,
    Runs,
}

enum Header {
    Channel {
        name: SharedString,
        agents: usize,
    },
    Direct {
        face: Face,
        name: SharedString,
        role: SharedString,
    },
}

struct Busy {
    name: SharedString,
    task: SharedString,
}

impl Feed {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Feed {
        let observation = cx.observe(&state, |_feed, _state, cx| cx.notify());
        let events = cx.subscribe_in(
            &state,
            window,
            |feed, _state, event: &StateEvent, window, cx| match event {
                StateEvent::SelectionChanged => {
                    feed.resync(Resync::Reset, cx);
                    feed.refresh_placeholder(window, cx);
                }
                StateEvent::MessagesLoaded => {
                    feed.resync(Resync::Reset, cx);
                    feed.list.scroll_to_end();
                }
                StateEvent::MessageAppended => {
                    feed.resync(Resync::Reset, cx);
                    feed.list.scroll_to_end();
                }
                StateEvent::RunsChanged => feed.resync(Resync::Runs, cx),
                StateEvent::FoldToggled => {
                    feed.list.remeasure();
                    cx.notify();
                }
                StateEvent::OlderLoaded => feed.keep_position(cx),
                StateEvent::SendFailed(text) => {
                    let text = text.clone();
                    feed.composer
                        .update(cx, |composer, cx| composer.restore(text, window, cx));
                }
                StateEvent::Mention(text) => {
                    let text = text.clone();
                    feed.composer
                        .update(cx, |composer, cx| composer.insert(&text, window, cx));
                }
            },
        );
        let items = items(state.read(cx), OffsetDateTime::now_utc());
        let list = ListState::new(items.len(), ListAlignment::Bottom, px(320.));
        list.set_follow_mode(FollowMode::Tail);
        let pager = state.downgrade();
        list.set_scroll_handler(move |event: &ListScrollEvent, _window, cx| {
            if !near_top(event) {
                return;
            }
            pager
                .update(cx, |state, cx| match state.history() {
                    History::More => state.load_older(cx),
                    History::Unknown => {}
                    History::Loading => {}
                    History::Complete => {}
                })
                .ok();
        });
        let placeholder = placeholder(state.read(cx));
        let sender = state.clone();
        let composer = cx.new(|cx| {
            Composer::new(
                placeholder,
                Box::new(move |body, cx| sender.update(cx, |state, cx| state.send(body, cx))),
                window,
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

    fn refresh_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = placeholder(self.state.read(cx));
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(placeholder, window, cx)
        });
    }

    fn keep_position(&mut self, cx: &mut Context<Self>) {
        let ListOffset {
            item_ix,
            offset_in_item,
        } = self.list.logical_scroll_top();
        let (anchor, offset_in_item) = match anchor_from(&self.items, item_ix) {
            Some((anchor, moved)) if moved => (Some(anchor), px(0.)),
            Some((anchor, _)) => (Some(anchor), offset_in_item),
            None => (None, offset_in_item),
        };
        let items = items(self.state.read(cx), OffsetDateTime::now_utc());
        let mut found = None;
        if let Some(anchor) = anchor {
            for (index, item) in items.iter().enumerate() {
                if key(item) == anchor {
                    found = Some(index);
                    break;
                }
            }
        }
        let count = items.len();
        self.items = Rc::new(items);
        self.list.reset(count);
        if let Some(index) = found {
            self.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item,
            });
        }
        cx.notify();
    }

    fn resync(&mut self, resync: Resync, cx: &mut Context<Self>) {
        let items = items(self.state.read(cx), OffsetDateTime::now_utc());
        let before = self.items.len();
        let first_run = first_run(&items);
        self.items = Rc::new(items);
        match resync {
            Resync::Reset => self.list.reset(self.items.len()),
            Resync::Runs => {
                if self.items.len() == before {
                    self.list.remeasure_items(first_run..self.items.len());
                } else {
                    self.list.reset(self.items.len());
                }
            }
        }
        cx.notify();
    }

    fn body(&self) -> AnyElement {
        if self.items.is_empty() {
            return empty_state().into_any_element();
        }
        let items = self.items.clone();
        let state = self.state.clone();
        let folder = self.state.clone();
        let on_toggle: OnToggle = Rc::new(move |message, _window, cx| {
            folder.update(cx, |state, cx| state.toggle_thinking(message, cx));
        });
        let player = self.state.clone();
        let on_play: OnPlay = Rc::new(move |message, _window, cx| {
            player.update(cx, |state, cx| state.toggle_voice(message, cx));
        });
        let discloser = self.state.clone();
        let on_disclose: OnDisclose = Rc::new(move |disclosure, _window, cx| {
            discloser.update(cx, |state, cx| state.toggle(disclosure, cx));
        });
        let actions = Actions {
            on_toggle,
            on_play,
            on_disclose,
            card: card::actions(&self.state),
        };
        let stopper = self.state.clone();
        let on_stop: OnStop = Rc::new(move |run, _window, cx| {
            stopper.update(cx, |state, cx| state.interrupt(run, cx));
        });
        list(self.list.clone(), move |index, _window, cx| {
            let Some(item) = items.get(index) else {
                return div().into_any_element();
            };
            match item {
                Item::Separator(title) => day_separator(title.clone()).into_any_element(),
                Item::Message(message) => {
                    let state = state.read(cx);
                    let fold = if state.is_expanded(message.id) {
                        Fold::Expanded
                    } else {
                        Fold::Collapsed
                    };
                    let waveform = match &message.voice {
                        Some(voice) => state.waveform(voice.recording),
                        None => None,
                    };
                    let run = match &message.run {
                        Some(run) => {
                            let answer =
                                crate::rich::split_thinking(&crate::message::source(&message.body))
                                    .answer;
                            runlog::pane(
                                runlog::PaneInput {
                                    message: message.id,
                                    run,
                                    answer: &answer,
                                    log: state.run_log(&run.id),
                                },
                                &|disclosure, by_default| state.is_open(disclosure, by_default),
                            )
                        }
                        None => None,
                    };
                    let look = Look {
                        fold,
                        player: state.player(message.id),
                        waveform,
                        run,
                    };
                    message_row(message, &state.people(), look, &actions).into_any_element()
                }
                Item::Run(run) => {
                    let state = state.read(cx);
                    let rows =
                        runlog::views(owner(run), run.steps.clone(), &|disclosure, by_default| {
                            state.is_open(disclosure, by_default)
                        });
                    let look = LiveLook {
                        rows,
                        on_stop: on_stop.clone(),
                        on_disclose: actions.on_disclose.clone(),
                    };
                    run_card(run, &state.people(), look).into_any_element()
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
    for run in state.live_runs() {
        items.push(Item::Run(run_view(run)));
    }
    items
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Separator(SharedString),
    Message(tuclaw_core::model::MessageId),
    Run(Option<tuclaw_core::v3::RunId>),
}

fn key(item: &Item) -> Key {
    match item {
        Item::Separator(title) => Key::Separator(title.clone()),
        Item::Message(message) => Key::Message(message.id),
        Item::Run(run) => Key::Run(run.id.clone()),
    }
}

fn anchor_from(items: &[Item], from: usize) -> Option<(Key, bool)> {
    for (index, item) in items.iter().enumerate().skip(from) {
        match item {
            Item::Message(message) => return Some((Key::Message(message.id), index != from)),
            Item::Run(run) => return Some((Key::Run(run.id.clone()), index != from)),
            Item::Separator(_) => {}
        }
    }
    None
}

fn near_top(event: &ListScrollEvent) -> bool {
    event.is_scrolled && event.visible_range.start <= PREFETCH
}

fn first_run(items: &[Item]) -> usize {
    let mut first = items.len();
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Run(_) => {
                first = index;
                break;
            }
            Item::Separator(_) => {}
            Item::Message(_) => {}
        }
    }
    first
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
        ChannelKind::Direct(agent) => direct_header(&state.people(), *agent, name),
    }
}

fn direct_header(people: &People, agent: AgentId, channel: &str) -> Header {
    let mut found = None;
    for candidate in people.agents {
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
        picture,
    }) = found
    else {
        return Header::Channel {
            name: SharedString::from(channel.to_string()),
            agents: 0,
        };
    };
    Header::Direct {
        face: Face {
            initials: SharedString::from(initials.clone()),
            color: theme::agent_chip(*sort_index as usize),
            picture: people.picture(picture.as_ref()),
        },
        name: SharedString::from(name.clone()),
        role: SharedString::from(role.clone()),
    }
}

fn placeholder(state: &AppState) -> SharedString {
    match header(state) {
        Header::Channel { name, agents: _ } => SharedString::from(format!("Message #{name}")),
        Header::Direct {
            face: _,
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
        picture: _,
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
            .child(icon(Glyph::Channel, px(16.), theme::text_label()))
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
        Header::Direct { face, name, role } => div()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_w(px(0.))
            .child(avatar(face, AvatarSize::Header))
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
        .child(chip().child(icon(Glyph::More, px(15.), theme::text_secondary())))
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

    use tuclaw_core::v3::RunState;

    use super::{Busy, Feed, Header, Item, busy_agents, header};
    use crate::live::RunView;
    use crate::runlog::{Row, StepStatus};
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
            picture: None,
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
        let (feed, cx) = cx.add_window_view(move |window, cx| Feed::new(built, window, cx));
        (mock, state, feed, cx)
    }

    fn last_agent_message(state: &Entity<AppState>, cx: &mut VisualTestContext) -> i64 {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if let Author::Agent(_) = message.author {
                    let tuclaw_core::model::MessageId(raw) = message.id;
                    found = Some(raw);
                }
            }
            found.expect("the surface has an agent message")
        })
    }

    fn click(cx: &mut VisualTestContext, selector: String) {
        let bounds = cx
            .debug_bounds(Box::leak(selector.clone().into_boxed_str()))
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_avatar_card_mentions_the_agent_in_the_composer(cx: &mut TestAppContext) {
        let (_mock, state, feed, cx) = feed(cx);
        cx.run_until_parked();
        let raw = last_agent_message(&state, cx);
        click(cx, format!("card-{raw}-trigger"));
        assert!(cx.debug_bounds("agent-card").is_some());
        click(cx, "card-mention".to_string());
        assert_eq!(typed(&feed, cx), "@tuclaw ");
        assert!(cx.debug_bounds("agent-card").is_none());
    }

    #[gpui::test]
    fn the_avatar_card_opens_the_agent_settings(cx: &mut TestAppContext) {
        let (_mock, state, _feed, cx) = feed(cx);
        cx.run_until_parked();
        let raw = last_agent_message(&state, cx);
        click(cx, format!("card-{raw}-trigger"));
        click(cx, "card-settings".to_string());
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.settings().and_then(|settings| settings.agent()),
                Some(tuclaw_core::model::AgentId(1))
            );
        });
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

    fn runs(feed: &Entity<Feed>, cx: &mut VisualTestContext) -> Vec<RunView> {
        feed.read_with(cx, |feed, _cx| {
            let mut runs = Vec::new();
            for item in feed.items.iter() {
                match item {
                    Item::Run(run) => runs.push(run.clone()),
                    Item::Separator(_) => {}
                    Item::Message(_) => {}
                }
            }
            runs
        })
    }

    #[gpui::test]
    fn a_streaming_run_is_drawn_and_then_replaced_by_its_answer(cx: &mut TestAppContext) {
        let (mock, state, feed, cx) = feed(cx);
        state.update(cx, |state, cx| {
            state.send("Лисички?".to_string(), cx).expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        mock.step();
        cx.run_until_parked();
        let queued = runs(&feed, cx);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].state, RunState::Queued);
        assert_eq!(queued[0].id, None);
        for _ in 0..3 {
            mock.step();
        }
        cx.run_until_parked();
        let streaming = runs(&feed, cx);
        assert_eq!(streaming.len(), 1);
        assert_eq!(streaming[0].state, RunState::Running);
        assert_eq!(streaming[0].segment, "Посмотрю, ");
        let Some(tuclaw_core::v3::RunId(id)) = streaming[0].id.clone() else {
            panic!("a started run carries its id");
        };
        let selector: &'static str = format!("run-{id}").leak();
        assert!(cx.debug_bounds(selector).is_some(), "the run card is drawn");
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), feed.items.len())
        });
        play(&mock, cx);
        assert!(runs(&feed, cx).is_empty());
        state.read_with(cx, |state, _cx| {
            let last = state.messages().last().expect("the answer arrived");
            assert_eq!(last.author, Author::Agent(AgentId(1)));
        });
        feed.read_with(cx, |feed, _cx| {
            assert_eq!(feed.list.item_count(), feed.items.len())
        });
    }

    #[gpui::test]
    fn the_live_run_of_a_surface_shows_its_steps(cx: &mut TestAppContext) {
        let (_mock, state, feed, cx) = feed(cx);
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let live = runs(&feed, cx);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].author, Author::Agent(AgentId(3)));
        let steps = &live[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0],
            Row::Thought {
                seq: 0,
                text: "Проверяю новые релизы.".to_string()
            }
        );
        let Row::Tool(call) = &steps[1] else {
            panic!("the second step is the tool, got {:?}", steps[1]);
        };
        assert_eq!(call.name, "WebFetch");
        assert_eq!(call.arg, "example.org/releases");
        assert_eq!(call.status, StepStatus::Ok);
        assert_eq!(
            steps[2],
            Row::Status {
                seq: 2,
                text: "compacting · context 91%".to_string()
            }
        );
        assert_eq!(live[0].segment, "Нашёл три новых релиза, ");
    }

    #[gpui::test]
    fn stop_interrupts_the_run_and_keeps_its_text(cx: &mut TestAppContext) {
        let (mock, state, feed, cx) = feed(cx);
        state.update(cx, |state, cx| {
            state.send("Лисички?".to_string(), cx).expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        for _ in 0..4 {
            mock.step();
        }
        cx.run_until_parked();
        let streaming = runs(&feed, cx);
        let Some(tuclaw_core::v3::RunId(id)) = streaming[0].id.clone() else {
            panic!("a started run carries its id");
        };
        let selector: &'static str = format!("run-stop-{id}").leak();
        let stop = cx
            .debug_bounds(selector)
            .expect("a working run offers Stop");
        cx.simulate_click(stop.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(runs(&feed, cx)[0].state, RunState::Stopping);
        assert!(
            cx.debug_bounds(selector).is_none(),
            "Stop leaves once it is pressed"
        );
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        play(&mock, cx);
        let stopped = runs(&feed, cx);
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].state, RunState::Interrupted);
        assert_eq!(stopped[0].segment, "Посмотрю, ");
        state.read_with(cx, |state, _cx| assert_eq!(state.messages().len(), before));
    }

    fn folded_world() -> tuclaw_core::v3::Seed {
        let surfaces = serde_json::from_str(include_str!("../../core/testdata/v3/surfaces.json"))
            .expect("surfaces");
        let agents = serde_json::from_str(include_str!("../../core/testdata/v3/agents.json"))
            .expect("agents");
        let message = serde_json::json!({
            "id": 7, "surface_id": 1, "kind": "answer", "author": {"kind": "agent", "agent_id": 1},
            "text": "<details><summary>Thinking</summary>\n\n- checked the notes\n\n</details>\n\n## Готово\n\n- **лисички** со сливками\n- [рецепт](https://example.org)",
            "created_at": "2026-10-03T15:26:13Z"
        });
        tuclaw_core::v3::Seed {
            surfaces,
            agents,
            messages: vec![serde_json::from_value(message).expect("message")],
            runs: Vec::new(),
            media: Vec::new(),
            me: None,
        }
    }

    #[gpui::test]
    fn a_thinking_fold_starts_collapsed_and_toggles(cx: &mut TestAppContext) {
        let (_mock, state) = crate::testing::seeded(cx, folded_world());
        let built = state.clone();
        let (_feed, cx) = cx.add_window_view(move |window, cx| Feed::new(built, window, cx));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("message-7").is_some(),
            "the answer is drawn"
        );
        let toggle = cx.debug_bounds("thinking-7").expect("the fold is drawn");
        let collapsed = cx.debug_bounds("message-7").expect("drawn").size.height;
        cx.simulate_click(toggle.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert!(state.is_expanded(tuclaw_core::model::MessageId(7)))
        });
        let expanded = cx.debug_bounds("message-7").expect("drawn").size.height;
        assert!(expanded > collapsed, "{expanded:?} > {collapsed:?}");
        let toggle = cx
            .debug_bounds("thinking-7")
            .expect("the fold is still drawn");
        cx.simulate_click(toggle.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert!(!state.is_expanded(tuclaw_core::model::MessageId(7)))
        });
    }

    fn spoken_world() -> tuclaw_core::v3::Seed {
        let mut world = folded_world();
        let message = serde_json::json!({
            "id": 8, "surface_id": 1, "kind": "user", "author": {"kind": "user"},
            "text": "[Voice message]\nПоставь кроваво-красный везде.",
            "created_at": "2026-10-03T15:27:00Z",
            "attachments": [{"id": 5, "kind": "voice", "mime": "audio/mp4", "size_bytes": 1, "duration_ms": 2000}]
        });
        world
            .messages
            .push(serde_json::from_value(message).expect("message"));
        world.media.push(tuclaw_core::v3::SeedMedia {
            id: tuclaw_core::v3::AttachmentId(5),
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../core/testdata/v3/media/tone.m4a"),
        });
        world
    }

    #[gpui::test]
    fn the_play_button_plays_the_original_recording(cx: &mut TestAppContext) {
        let (_mock, state) = crate::testing::seeded(cx, spoken_world());
        let built = state.clone();
        let (_feed, cx) = cx.add_window_view(move |window, cx| Feed::new(built, window, cx));
        cx.run_until_parked();
        let button = cx
            .debug_bounds("voice-8")
            .expect("the play button is drawn");
        cx.simulate_click(button.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        let player = state.read_with(cx, |state, _cx| {
            state.player(tuclaw_core::model::MessageId(8))
        });
        let crate::state::Player::Playing { position: _, total } = player else {
            panic!("the click plays the recording, got {player:?}");
        };
        assert!((total.as_secs_f64() - 2.0).abs() < 0.1, "{total:?}");
        assert!(
            cx.debug_bounds("voice-7").is_none(),
            "a message without a recording has no player"
        );
    }

    #[test]
    fn only_a_scroll_near_the_top_asks_for_older_messages() {
        let event = |start: usize, is_scrolled: bool| gpui::ListScrollEvent {
            visible_range: start..start + 10,
            count: 10,
            is_scrolled,
            is_following_tail: false,
        };
        assert!(super::near_top(&event(0, true)));
        assert!(super::near_top(&event(super::PREFETCH, true)));
        assert!(!super::near_top(&event(super::PREFETCH + 1, true)));
        assert!(!super::near_top(&event(0, false)));
    }

    #[gpui::test]
    fn loading_older_messages_keeps_the_visible_message_in_place(cx: &mut TestAppContext) {
        let (_mock, state) = crate::testing::seeded(cx, crate::testing::long_world(120));
        let built = state.clone();
        let (feed, cx) = cx.add_window_view(move |window, cx| Feed::new(built, window, cx));
        cx.run_until_parked();
        let anchored = tuclaw_core::model::MessageId(75);
        feed.update(cx, |feed, _cx| {
            let mut index = None;
            for (position, item) in feed.items.iter().enumerate() {
                if super::key(item) == super::Key::Message(anchored) {
                    index = Some(position);
                }
            }
            feed.list.scroll_to(gpui::ListOffset {
                item_ix: index.expect("message 75 is on the first page"),
                offset_in_item: gpui::px(4.),
            });
        });
        state.update(cx, |state, cx| state.load_older(cx));
        cx.run_until_parked();
        feed.read_with(cx, |feed, _cx| {
            let gpui::ListOffset {
                item_ix,
                offset_in_item,
            } = feed.list.logical_scroll_top();
            assert_eq!(
                feed.items.get(item_ix).map(super::key),
                Some(super::Key::Message(anchored))
            );
            assert_eq!(offset_in_item, gpui::px(4.));
            assert!(feed.items.len() > 100);
        });
    }
}
