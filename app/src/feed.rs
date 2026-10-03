use std::rc::Rc;

use gpui::{
    AnyElement, Context, Div, Entity, FollowMode, FontWeight, IntoElement, ListAlignment,
    ListState, Render, SharedString, Subscription, Window, div, list, prelude::*, px,
};
use time::{OffsetDateTime, UtcOffset};
use tuclaw_core::grouping::{DaySection, group_by_day};
use tuclaw_core::model::{Agent, AgentId, AgentStatus, Channel, ChannelKind, Message};

use crate::composer::Composer;
use crate::live::{OnStop, RunView, run_card, run_view};
use crate::message::{Fold, OnToggle, message_row};
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
            StateEvent::RunsChanged => feed.resync(Resync::Runs, cx),
            StateEvent::FoldToggled => {
                feed.list.remeasure();
                cx.notify();
            }
            StateEvent::SendFailed(text) => {
                let text = text.clone();
                feed.composer
                    .update(cx, |composer, cx| composer.restore(text, cx));
            }
        });
        let items = items(state.read(cx), OffsetDateTime::now_utc());
        let list = ListState::new(items.len(), ListAlignment::Bottom, px(320.));
        list.set_follow_mode(FollowMode::Tail);
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
                    message_row(message, state.agents(), fold, on_toggle.clone()).into_any_element()
                }
                Item::Run(run) => {
                    run_card(run, state.read(cx).agents(), on_stop.clone()).into_any_element()
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

    use tuclaw_core::v3::{RunState, ToolStatus};

    use super::{Busy, Feed, Header, Item, busy_agents, header};
    use crate::live::{RunView, StepView};
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
        assert_eq!(
            live[0].steps,
            vec![
                StepView::Thought("Проверяю новые релизы.".to_string()),
                StepView::Tool {
                    name: "WebFetch".to_string(),
                    detail: "https://example.org/releases".to_string(),
                    status: ToolStatus::Ok,
                },
                StepView::Status {
                    status: "compacting".to_string(),
                    detail: "context 91%".to_string(),
                },
            ]
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
        }
    }

    #[gpui::test]
    fn a_thinking_fold_starts_collapsed_and_toggles(cx: &mut TestAppContext) {
        let (_mock, state) = crate::testing::seeded(cx, folded_world());
        let built = state.clone();
        let (_feed, cx) = cx.add_window_view(move |_window, cx| Feed::new(built, cx));
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
}
