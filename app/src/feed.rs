use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, Entity, FollowMode, FontWeight, IntoElement, ListAlignment,
    ListOffset, ListScrollEvent, ListState, Pixels, Render, SharedString, Subscription, Task,
    Window, canvas, div, list, prelude::*, px,
};
use time::OffsetDateTime;
use tuclaw_core::grouping::day_title;
use tuclaw_core::model::{Agent, AgentId, Channel, ChannelKind, Message, MessageId, Weight};
use tuclaw_core::v3;

use crate::automation::{FireRow, OnTask, Quiet, failed_card, quiet_divider, trigger_tag};
use crate::card;
use crate::composer::Composer;
use crate::control::{AvatarSize, Face, avatar};
use crate::icon::{Glyph, icon};
use crate::live::{LiveLook, OnStop, RunView, owner, run_card, run_view};
use crate::local;
use crate::message::{
    Actions, Fold, Look, OnChoose, OnPicture, OnPlay, OnToggle, Quote, Stripe, message_row,
};
use crate::people::People;
use crate::plain;
use crate::runlog::{self, OnDisclose};
use crate::state::{AppState, History, StateEvent};

const PREFETCH: usize = 3;
const QUIET_GAP: time::Duration = time::Duration::hours(1);
const SEEN_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
const QUOTE_LIMIT: usize = 80;
use crate::theme;

pub struct Feed {
    state: Entity<AppState>,
    list: ListState,
    items: Rc<Vec<Item>>,
    composer: Entity<Composer>,
    focus: Focus,
    _observation: Subscription,
    _events: Subscription,
    _activation: Subscription,
    _seen: Option<Task<()>>,
    painted: Painted,
}

type Painted = Rc<RefCell<HashMap<MessageId, Bounds<Pixels>>>>;

enum Focus {
    Requested,
    Taken,
}

enum Item {
    Separator(SharedString),
    Message(Box<Message>, Option<FireRow>),
    Unread(Fresh),
    Failed(FireRow),
    Quiet(Quiet),
    Run(RunView),
}

enum Resync {
    Reset,
    Runs,
    Labels,
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
                    feed.read_to_newest(cx);
                    feed.schedule_seen(cx);
                }
                StateEvent::MessageAppended => {
                    let follow = {
                        let state = feed.state.read(cx);
                        state.following()
                            || state
                                .messages()
                                .last()
                                .is_some_and(|message| message.weight == Weight::Mine)
                    };
                    feed.resync(Resync::Reset, cx);
                    if follow {
                        feed.list.scroll_to_end();
                        feed.read_to_newest(cx);
                    }
                    feed.schedule_seen(cx);
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
                StateEvent::Mention(mention) => {
                    let mention = mention.clone();
                    feed.composer
                        .update(cx, |composer, cx| composer.mention(mention, window, cx));
                }
                StateEvent::TasksLoaded => feed.resync(Resync::Labels, cx),
                StateEvent::ChannelsChanged => {}
                StateEvent::Alert(_) => {}
                StateEvent::ReplyStarted => {
                    let focus = feed.composer.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                StateEvent::PictureOpened => {}
                StateEvent::PicturesLoaded => {
                    feed.list.remeasure();
                    cx.notify();
                }
            },
        );
        let watcher = state.clone();
        let activation = cx.observe_window_activation(window, move |feed, window, cx| {
            let active = window.is_window_active();
            watcher.update(cx, |state, cx| state.set_window_active(active, cx));
            if active {
                feed.schedule_seen(cx);
            }
        });
        let items = items(state.read(cx), local::now());
        let list = ListState::new(items.len(), ListAlignment::Bottom, px(320.));
        list.set_follow_mode(FollowMode::Tail);
        let pager = state.downgrade();
        let scrolled = cx.weak_entity();
        list.set_scroll_handler(move |event: &ListScrollEvent, _window, cx| {
            let following = event.is_following_tail;
            scrolled
                .update(cx, |feed, cx| feed.scrolled(following, cx))
                .ok();
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
                Box::new(move |draft, cx| {
                    sender.update(cx, |state, cx| state.send_draft(draft, cx))
                }),
                window,
                cx,
            )
            .with_state(state.clone(), cx)
        });
        state.update(cx, |state, cx| state.read_to_newest(cx));
        Feed {
            state,
            list,
            items: Rc::new(items),
            composer,
            focus: Focus::Requested,
            _observation: observation,
            _events: events,
            _activation: activation,
            _seen: None,
            painted: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    fn reveal(&mut self, target: MessageId) {
        for (index, item) in self.items.iter().enumerate() {
            let Item::Message(message, _trigger) = item else {
                continue;
            };
            if message.id == target {
                self.list.scroll_to_reveal_item(index);
                return;
            }
        }
    }

    fn scrolled(&mut self, following: bool, cx: &mut Context<Self>) {
        let was = self.state.read(cx).following();
        self.state
            .update(cx, |state, _cx| state.set_following(following));
        if following && !was {
            self.read_to_newest(cx);
        }
        self.schedule_seen(cx);
        cx.notify();
    }

    fn schedule_seen(&mut self, cx: &mut Context<Self>) {
        self._seen = Some(cx.spawn(async move |feed, cx| {
            cx.background_executor().timer(SEEN_AFTER).await;
            feed.update(cx, |feed, cx| feed.mark_visible_seen(cx)).ok();
        }));
    }

    fn mark_visible_seen(&mut self, cx: &mut Context<Self>) {
        let viewport = self.list.viewport_bounds();
        let mut seen = Vec::new();
        {
            let state = self.state.read(cx);
            for item in self.items.iter() {
                let Item::Message(message, _trigger) = item else {
                    continue;
                };
                if !state.is_fresh(message) {
                    continue;
                }
                let Some(bounds) = self.painted.borrow().get(&message.id).copied() else {
                    continue;
                };
                if on_screen(bounds, viewport) {
                    seen.push(message.id);
                }
            }
        }
        self.state.update(cx, |state, cx| state.mark_seen(seen, cx));
    }

    fn pill(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let state = self.state.read(cx);
        if state.following() {
            return None;
        }
        let mut reply: Option<&Message> = None;
        let mut posts = 0;
        for item in self.items.iter() {
            let Item::Message(message, _trigger) = item else {
                continue;
            };
            if !state.is_fresh(message) {
                continue;
            }
            match message.weight {
                Weight::Reply => reply = Some(message),
                Weight::Activity => posts += 1,
                Weight::Mine => {}
            }
        }
        let (text, tone) = match reply {
            Some(message) => {
                let name = match message.author {
                    tuclaw_core::model::Author::Agent(agent) => state
                        .people()
                        .agent(agent)
                        .map(|agent| agent.name.clone())
                        .unwrap_or_else(|| "An agent".to_string()),
                    tuclaw_core::model::Author::User => "You".to_string(),
                    tuclaw_core::model::Author::System => "tuclaw".to_string(),
                };
                (
                    format!("{name} replied · {}", local::clock(message.sent_at)),
                    theme::text_primary(),
                )
            }
            None if posts == 1 => ("1 new post".to_string(), theme::text_muted()),
            None if posts > 1 => (format!("{posts} new posts"), theme::text_muted()),
            None => return None,
        };
        let list = self.list.clone();
        let reader = self.state.clone();
        Some(
            div()
                .absolute()
                .bottom(px(12.))
                .left(px(0.))
                .right(px(0.))
                .flex()
                .justify_center()
                .child(
                    crate::control::button("feed-new-pill")
                        .gap(px(6.))
                        .px(px(12.))
                        .h(px(28.))
                        .rounded_full()
                        .bg(tone)
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::window())
                        .on_click(move |_event, _window, cx| {
                            list.scroll_to_end();
                            reader.update(cx, |state, cx| {
                                state.set_following(true);
                                state.read_to_newest(cx);
                            });
                        })
                        .child(SharedString::from(text))
                        .child(icon(Glyph::Down, px(12.), theme::window())),
                ),
        )
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
        let items = items(self.state.read(cx), local::now());
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

    fn read_to_newest(&mut self, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.read_to_newest(cx));
    }

    fn resync(&mut self, resync: Resync, cx: &mut Context<Self>) {
        let items = items(self.state.read(cx), local::now());
        let before = self.items.len();
        let first_run = first_run(&items);
        self.items = Rc::new(items);
        match resync {
            Resync::Reset => self.list.reset(self.items.len()),
            Resync::Labels => {
                if self.items.len() == before {
                    self.list.remeasure();
                } else {
                    self.list.reset(self.items.len());
                }
            }
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

    fn body(&self, cx: &Context<Self>) -> AnyElement {
        if self.items.is_empty() {
            return empty_state().into_any_element();
        }
        let items = self.items.clone();
        let state = self.state.clone();
        self.painted.borrow_mut().clear();
        let painted = self.painted.clone();
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
        let viewer = self.state.clone();
        let on_picture: OnPicture = Rc::new(move |viewed, _window, cx| {
            viewer.update(cx, |state, cx| state.view_picture(viewed, cx));
        });
        let chooser = self.state.clone();
        let on_choose: OnChoose = Rc::new(move |message, option, _window, cx| {
            chooser.update(cx, |state, cx| state.choose_reply(message, option, cx));
        });
        let replier = self.state.clone();
        let on_reply: OnToggle = Rc::new(move |message, _window, cx| {
            replier.update(cx, |state, cx| state.start_reply(message, cx));
        });
        let jumper = cx.entity().downgrade();
        let on_jump: OnToggle = Rc::new(move |message, _window, cx| {
            jumper.update(cx, |feed, _cx| feed.reveal(message)).ok();
        });
        let actions = Actions {
            on_toggle,
            on_play,
            on_disclose,
            on_picture,
            on_choose,
            on_reply,
            on_jump,
            card: card::actions(&self.state),
        };
        let stopper = self.state.clone();
        let on_stop: OnStop = Rc::new(move |run, _window, cx| {
            stopper.update(cx, |state, cx| state.interrupt(run, cx));
        });
        let opener = self.state.clone();
        let on_task: OnTask = Rc::new(move |task, _window, cx| {
            let task = task.clone();
            opener.update(cx, |state, cx| state.open_task(task, cx));
        });
        list(self.list.clone(), move |index, _window, cx| {
            let Some(item) = items.get(index) else {
                return div().into_any_element();
            };
            match item {
                Item::Separator(title) => day_separator(title.clone()).into_any_element(),
                Item::Unread(fresh) => unread_divider(*fresh).into_any_element(),
                Item::Failed(row) => failed_card(row, on_task.clone()).into_any_element(),
                Item::Quiet(quiet) => quiet_divider(quiet).into_any_element(),
                Item::Message(message, trigger) => {
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
                        stripe: stripe_of(state, message),
                        quote: quote_of(state, message),
                        fold,
                        player: state.player(message.id),
                        waveform,
                        run,
                        trigger: trigger
                            .as_ref()
                            .map(|row| trigger_tag(row, on_task.clone()).into_any_element()),
                    };
                    let id = message.id;
                    let painted = painted.clone();
                    div()
                        .relative()
                        .child(message_row(
                            message,
                            &state.people(),
                            look,
                            &actions,
                            state.pictures(),
                        ))
                        .child(
                            canvas(
                                |_bounds, _window, _cx| {},
                                move |bounds, _state, _window, _cx| {
                                    painted.borrow_mut().insert(id, bounds);
                                },
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        )
                        .into_any_element()
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
        let header = header(self.state.read(cx));
        let pulse = pulse(self.state.read(cx));
        let opener = self.state.clone();
        let on_pulse: OnPulse = Rc::new(move |_window, cx| {
            opener.update(cx, |state, cx| {
                if state.automations_open() {
                    state.close_automations(cx);
                } else {
                    state.open_automations(cx);
                }
            });
        });
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(header_element(header, pulse, on_pulse))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.body(cx))
                    .children(self.pill(cx)),
            )
            .child(self.composer.clone())
    }
}

fn items(state: &AppState, now: OffsetDateTime) -> Vec<Item> {
    let today = local::local(now).date();
    let mut triggers: HashMap<MessageId, FireRow> = HashMap::new();
    let mut entries = Vec::new();
    for mark in state.fires() {
        let Some(at) = mark.at else {
            continue;
        };
        let row = FireRow::new(mark, at, state.tasks());
        let answer = match mark.outcome {
            v3::Outcome::Ran => mark.message_id.map(|v3::MessageId(id)| MessageId(id)),
            v3::Outcome::Silent => None,
            v3::Outcome::Skipped => None,
            v3::Outcome::Failed => None,
            v3::Outcome::Unknown => None,
        };
        if let Some(answer) = answer.filter(|answer| has_message(state, *answer)) {
            triggers.insert(answer, row);
            continue;
        }
        let entry = match mark.outcome {
            v3::Outcome::Failed => Entry::Item(at, Box::new(Item::Failed(row))),
            v3::Outcome::Ran => Entry::Check(at, mark.outcome),
            v3::Outcome::Silent => Entry::Check(at, mark.outcome),
            v3::Outcome::Skipped => Entry::Check(at, mark.outcome),
            v3::Outcome::Unknown => Entry::Check(at, mark.outcome),
        };
        entries.push(entry);
    }
    for message in state.messages() {
        let trigger = triggers.remove(&message.id);
        entries.push(Entry::Item(
            message.sent_at,
            Box::new(Item::Message(Box::new(message.clone()), trigger)),
        ));
    }
    let mut live = Vec::new();
    for run in state.live_runs() {
        match (run.state.is_finished(), run.started_at) {
            (true, Some(started)) => {
                entries.push(Entry::Item(started, Box::new(Item::Run(run_view(run)))));
            }
            (true, None) => live.push(run),
            (false, _) => live.push(run),
        }
    }
    entries.sort_by_key(Entry::at);
    let timeline = fold_quiet(entries, now);
    let mut items: Vec<Item> = Vec::new();
    let mut day = None;
    let mut divider = state.divider();
    let fresh = fresh_since(state);
    for (at, item) in timeline {
        if let (Some(cursor), Item::Message(message, _trigger)) = (divider, &item)
            && message.id > cursor
            && message.id > MessageId(0)
        {
            divider = None;
            items.push(Item::Unread(fresh));
        }
        let date = local::local(at).date();
        if day != Some(date) {
            day = Some(date);
            items.push(Item::Separator(SharedString::from(day_title(date, today))));
        }
        items.push(item);
    }
    for run in live {
        items.push(Item::Run(run_view(run)));
    }
    items
}

enum Entry {
    Item(OffsetDateTime, Box<Item>),
    Check(OffsetDateTime, v3::Outcome),
}

impl Entry {
    fn at(&self) -> OffsetDateTime {
        match self {
            Entry::Item(at, _item) => *at,
            Entry::Check(at, _outcome) => *at,
        }
    }
}

fn fold_quiet(entries: Vec<Entry>, now: OffsetDateTime) -> Vec<(OffsetDateTime, Item)> {
    let mut timeline = Vec::new();
    let mut since: Option<OffsetDateTime> = None;
    let mut pending: Option<Quiet> = None;
    for entry in entries {
        match entry {
            Entry::Check(at, outcome) => match &mut pending {
                Some(quiet) => quiet.add(at, outcome),
                None => pending = Some(Quiet::new(at, outcome)),
            },
            Entry::Item(at, item) => {
                if let Some(quiet) = pending.take()
                    && at - since.unwrap_or(quiet.first) >= QUIET_GAP
                {
                    timeline.push((quiet.first, Item::Quiet(quiet)));
                }
                since = Some(at);
                timeline.push((at, *item));
            }
        }
    }
    if let Some(quiet) = pending
        && now - since.unwrap_or(quiet.first) >= QUIET_GAP
    {
        timeline.push((quiet.first, Item::Quiet(quiet)));
    }
    timeline
}

fn on_screen(bounds: Bounds<Pixels>, viewport: Bounds<Pixels>) -> bool {
    let inside = bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom();
    let covers = bounds.size.height >= viewport.size.height
        && bounds.top() < viewport.bottom()
        && bounds.bottom() > viewport.top();
    inside || covers
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Fresh {
    replies: usize,
    posts: usize,
}

fn fresh_since(state: &AppState) -> Fresh {
    let mut fresh = Fresh::default();
    let Some(cursor) = state.divider() else {
        return fresh;
    };
    for message in state.messages() {
        if message.id <= cursor {
            continue;
        }
        match message.weight {
            Weight::Reply => fresh.replies += 1,
            Weight::Activity => fresh.posts += 1,
            Weight::Mine => {}
        }
    }
    fresh
}

fn fresh_text(fresh: Fresh) -> String {
    let mut parts = Vec::new();
    match fresh.replies {
        0 => {}
        1 => parts.push("1 reply".to_string()),
        replies => parts.push(format!("{replies} replies")),
    }
    match fresh.posts {
        0 => {}
        1 => parts.push("1 post".to_string()),
        posts => parts.push(format!("{posts} posts")),
    }
    if parts.is_empty() {
        return "New".to_string();
    }
    format!("New · {}", parts.join(", "))
}

fn quote_of(state: &AppState, message: &Message) -> Option<Quote> {
    let target = message.reply_to?;
    let mut previous = None;
    let mut quoted = None;
    for candidate in state.messages() {
        if candidate.id == message.id {
            break;
        }
        previous = Some(candidate.id);
        if candidate.id == target {
            quoted = Some(candidate);
        }
    }
    if previous == Some(target) {
        return None;
    }
    let Some(quoted) = quoted else {
        return Some(Quote {
            target,
            author: SharedString::from("An earlier message"),
            text: String::new(),
        });
    };
    let writer = crate::message::writer(quoted.author, &state.people());
    Some(Quote {
        target,
        author: writer.name,
        text: clip_quote(&plain::plain_text(&crate::message::source(&quoted.body))),
    })
}

fn clip_quote(text: &str) -> String {
    if text.chars().count() <= QUOTE_LIMIT {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(QUOTE_LIMIT).collect();
    clipped.push('…');
    clipped
}

fn stripe_of(state: &AppState, message: &Message) -> Stripe {
    if !state.is_fresh(message) {
        return Stripe::None;
    }
    match message.weight {
        Weight::Reply => Stripe::Reply,
        Weight::Activity => Stripe::Activity,
        Weight::Mine => Stripe::None,
    }
}

fn has_message(state: &AppState, id: MessageId) -> bool {
    let mut found = false;
    for message in state.messages() {
        if message.id == id {
            found = true;
            break;
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Separator(SharedString),
    Unread,
    Message(tuclaw_core::model::MessageId),
    Failed(tuclaw_core::v3::TaskId, i64),
    Quiet(i64),
    Run(Option<tuclaw_core::v3::RunId>),
}

fn key(item: &Item) -> Key {
    match item {
        Item::Separator(title) => Key::Separator(title.clone()),
        Item::Unread(_) => Key::Unread,
        Item::Message(message, _trigger) => Key::Message(message.id),
        Item::Failed(row) => Key::Failed(row.task.clone(), row.at.unix_timestamp()),
        Item::Quiet(quiet) => Key::Quiet(quiet.first.unix_timestamp()),
        Item::Run(run) => Key::Run(run.id.clone()),
    }
}

fn anchor_from(items: &[Item], from: usize) -> Option<(Key, bool)> {
    for (index, item) in items.iter().enumerate().skip(from) {
        match item {
            Item::Message(message, _trigger) => {
                return Some((Key::Message(message.id), index != from));
            }
            Item::Run(run) => return Some((Key::Run(run.id.clone()), index != from)),
            Item::Failed(row) => {
                return Some((
                    Key::Failed(row.task.clone(), row.at.unix_timestamp()),
                    index != from,
                ));
            }
            Item::Quiet(quiet) => {
                return Some((Key::Quiet(quiet.first.unix_timestamp()), index != from));
            }
            Item::Separator(_) => {}
            Item::Unread(_) => {}
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
            Item::Unread(_) => {}
            Item::Message(_, _) => {}
            Item::Failed(_) => {}
            Item::Quiet(_) => {}
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
        replies: _,
        marked: _,
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

type OnPulse = Rc<dyn Fn(&mut Window, &mut gpui::App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pulse {
    automations: usize,
    last: Option<OffsetDateTime>,
    failed: usize,
    open: bool,
}

fn pulse(state: &AppState) -> Option<Pulse> {
    let tasks = state.channel_tasks();
    if tasks.is_empty() {
        return None;
    }
    let mut last = None;
    for mark in state.fires() {
        if let Some(at) = mark.at
            && last.is_none_or(|known| at > known)
        {
            last = Some(at);
        }
    }
    Some(Pulse {
        automations: tasks.len(),
        last,
        failed: state.unseen_failures(),
        open: state.automations_open(),
    })
}

fn pulse_text(pulse: &Pulse) -> String {
    let count = if pulse.automations == 1 {
        "1 automation".to_string()
    } else {
        format!("{} automations", pulse.automations)
    };
    match pulse.last {
        Some(at) => format!("{count} · checked {}", local::clock(at)),
        None => count,
    }
}

fn pulse_element(pulse: Pulse, on_pulse: OnPulse) -> impl IntoElement {
    let dot = if pulse.failed > 0 {
        theme::accent()
    } else {
        theme::status_idle()
    };
    let element = crate::control::button("feed-automations")
        .accessibility_label("Automations of this channel")
        .gap(px(6.))
        .px(px(10.))
        .h(px(26.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.))
        .text_color(theme::text_secondary())
        .hover(|style| style.bg(theme::sunken()))
        .on_click(move |_event, window, cx| on_pulse(window, cx))
        .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
        .child(SharedString::from(pulse_text(&pulse)));
    let element = if pulse.failed > 0 {
        element.child(
            div()
                .px(px(6.))
                .rounded(px(5.))
                .bg(theme::accent())
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::chip_text())
                .child(SharedString::from(format!("{} failed", pulse.failed))),
        )
    } else {
        element
    };
    if pulse.open {
        element.bg(theme::selection())
    } else {
        element
    }
}

fn header_element(header: Header, pulse: Option<Pulse>, on_pulse: OnPulse) -> impl IntoElement {
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
        .children(pulse.map(|pulse| pulse_element(pulse, on_pulse)))
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

fn unread_divider(fresh: Fresh) -> impl IntoElement {
    div()
        .id("feed-unread")
        .debug_selector(|| "feed-unread".to_string())
        .w_full()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(20.))
        .pt(px(10.))
        .pb(px(6.))
        .child(div().flex_1().h(px(1.)).bg(theme::accent().opacity(0.6)))
        .child(
            div()
                .flex_none()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::accent())
                .child(SharedString::from(fresh_text(fresh))),
        )
        .child(div().flex_1().h(px(1.)).bg(theme::accent().opacity(0.6)))
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

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{AgentId, Author, Span};
    use tuclaw_core::v3::MockTransport;

    use tuclaw_core::v3::RunState;

    use super::{Feed, Header, Item, header};
    use crate::live::RunView;
    use crate::runlog::{Row, StepStatus};
    use crate::state::{AppState, Recording};
    use crate::testing::{FakeRecorder, channel_named, loaded, play};

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

    const TURTLE: &[u8] = include_bytes!("../../core/testdata/v3/media/avatar_agent.png");

    fn posted_by_jarvis(
        mock: &MockTransport,
        state: &Entity<AppState>,
        cx: &mut VisualTestContext,
        text: &str,
    ) -> i64 {
        mock.agent_posts(
            tuclaw_core::v3::SurfaceId(1),
            tuclaw_core::v3::AgentId(1),
            text,
        );
        while mock.step() {}
        cx.run_until_parked();
        last_agent_message(state, cx)
    }

    #[gpui::test]
    fn a_linked_picture_loads_and_is_drawn_in_the_message(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        mock.serve_public("https://media.example.test/turtle.png", TURTLE.to_vec());
        let raw = posted_by_jarvis(
            &mock,
            &state,
            cx,
            "Here it is:\n\n![A turtle](https://media.example.test/turtle.png)",
        );
        let selector: &'static str =
            Box::leak(format!("message-{raw}-md-1-picture").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some());
    }

    #[gpui::test]
    fn a_picture_that_is_not_https_is_never_fetched(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        mock.serve_public("http://media.example.test/turtle.png", TURTLE.to_vec());
        let raw = posted_by_jarvis(
            &mock,
            &state,
            cx,
            "![A turtle](http://media.example.test/turtle.png)",
        );
        state.read_with(cx, |state, _cx| assert!(state.pictures().is_empty()));
        let picture: &'static str = Box::leak(format!("message-{raw}-md-picture").into_boxed_str());
        assert!(cx.debug_bounds(picture).is_none());
    }

    fn unread_of(state: &Entity<AppState>, cx: &mut VisualTestContext, name: &str) -> usize {
        state.read_with(cx, |state, _cx| {
            let mut unread = None;
            for channel in state.channels() {
                if channel.name == name {
                    unread = Some(channel.unread);
                }
            }
            unread.expect("the channel exists")
        })
    }

    #[gpui::test]
    fn a_message_elsewhere_counts_until_its_channel_is_opened(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        mock.agent_posts(
            tuclaw_core::v3::SurfaceId(3),
            tuclaw_core::v3::AgentId(2),
            "The lights are on.",
        );
        while mock.step() {}
        cx.run_until_parked();
        assert_eq!(unread_of(&state, cx, "Smart Home"), 1);
        assert!(cx.debug_bounds("feed-unread").is_none());
        let home = channel_named(&state, cx, "Smart Home");
        state.update(cx, |state, cx| state.select(home, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("feed-unread").is_some());
        assert_eq!(
            unread_of(&state, cx, "Smart Home"),
            1,
            "opening is not reading: the message has not been seen yet"
        );
        see_everything_fresh(&state, cx);
        assert_eq!(unread_of(&state, cx, "Smart Home"), 0);
        let client = tuclaw_core::v3::Client::mock(&mock);
        let surfaces = futures::executor::block_on(client.surfaces()).expect("surfaces");
        let mut served = None;
        for surface in surfaces {
            if surface.name == "Smart Home" {
                served = Some(surface.unread);
            }
        }
        assert_eq!(served, Some(0));
    }

    fn marked_of(state: &Entity<AppState>, cx: &mut VisualTestContext, name: &str) -> bool {
        state.read_with(cx, |state, _cx| {
            let mut marked = None;
            for channel in state.channels() {
                if channel.name == name {
                    marked = Some(channel.marked);
                }
            }
            marked.expect("the channel exists")
        })
    }

    fn served_marked(mock: &MockTransport, name: &str) -> bool {
        let client = tuclaw_core::v3::Client::mock(mock);
        let surfaces = futures::executor::block_on(client.surfaces()).expect("surfaces");
        let mut marked = None;
        for surface in surfaces {
            if surface.name == name {
                marked = Some(surface.marked_unread);
            }
        }
        marked.expect("the surface exists")
    }

    #[gpui::test]
    fn a_channel_marked_unread_stays_marked_until_it_is_opened(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let home = channel_named(&state, cx, "Smart Home");
        state.update(cx, |state, cx| state.mark_unread(home, cx));
        cx.run_until_parked();
        assert!(marked_of(&state, cx, "Smart Home"));
        assert!(served_marked(&mock, "Smart Home"));
        state.update(cx, |state, cx| state.select(home, cx));
        cx.run_until_parked();
        assert!(!marked_of(&state, cx, "Smart Home"));
        assert!(!served_marked(&mock, "Smart Home"));
    }

    #[gpui::test]
    fn the_open_channel_marked_unread_stays_marked_until_it_is_opened_again(
        cx: &mut TestAppContext,
    ) {
        let (mock, state, _feed, cx) = feed(cx);
        let general = channel_named(&state, cx, "General");
        state.update(cx, |state, cx| state.mark_unread(general, cx));
        cx.run_until_parked();
        state.update(cx, |state, cx| {
            state.set_window_active(false, cx);
            state.set_window_active(true, cx);
            state.read_to_newest(cx);
        });
        cx.run_until_parked();
        assert!(marked_of(&state, cx, "General"));
        assert!(served_marked(&mock, "General"));
        state.update(cx, |state, cx| state.select(general, cx));
        cx.run_until_parked();
        assert!(!marked_of(&state, cx, "General"));
        assert!(!served_marked(&mock, "General"));
    }

    #[gpui::test]
    fn a_new_answer_releases_the_open_channel_marked_unread(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let general = channel_named(&state, cx, "General");
        state.update(cx, |state, cx| state.mark_unread(general, cx));
        cx.run_until_parked();
        posted_by_jarvis(&mock, &state, cx, "Something new.");
        state.update(cx, |state, cx| state.read_to_newest(cx));
        cx.run_until_parked();
        assert!(!marked_of(&state, cx, "General"));
        assert!(!served_marked(&mock, "General"));
    }

    #[gpui::test]
    fn an_inactive_window_keeps_new_messages_unread_until_it_is_back(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        assert!(cx.debug_bounds("feed-unread").is_none());
        state.update(cx, |state, cx| state.set_window_active(false, cx));
        posted_by_jarvis(&mock, &state, cx, "While you were away.");
        assert_eq!(unread_of(&state, cx, "General"), 1);
        state.update(cx, |state, cx| state.set_window_active(true, cx));
        cx.run_until_parked();
        assert_eq!(unread_of(&state, cx, "General"), 1, "unread until seen");
        assert!(
            cx.debug_bounds("feed-unread").is_some(),
            "the open channel marks where the new messages start"
        );
        see_everything_fresh(&state, cx);
        assert_eq!(unread_of(&state, cx, "General"), 0);
    }

    #[gpui::test]
    fn an_unseen_reply_is_still_unread_after_a_restart(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        state.update(cx, |state, cx| state.set_window_active(false, cx));
        posted_by_jarvis(&mock, &state, cx, "Waiting for you.");
        state.update(cx, |state, cx| state.set_window_active(true, cx));
        cx.run_until_parked();
        let client = tuclaw_core::v3::Client::mock(&mock);
        let surfaces = futures::executor::block_on(client.surfaces()).expect("surfaces");
        let general = surfaces
            .iter()
            .find(|surface| surface.name == "General")
            .expect("General exists");
        assert_eq!(
            (general.unread, general.unread_replies),
            (1, 1),
            "the daemon still counts it, so a restart shows it again"
        );
    }

    fn see_everything_fresh(state: &Entity<AppState>, cx: &mut VisualTestContext) {
        state.update(cx, |state, cx| {
            let mut fresh = Vec::new();
            for message in state.messages() {
                if state.is_fresh(message) {
                    fresh.push(message.id);
                }
            }
            state.mark_seen(fresh, cx);
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn talk_records_and_a_second_press_posts_the_voice(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let recorder = FakeRecorder::default();
        let tape = recorder.0.clone();
        state.update(cx, |state, _cx| state.set_recorder(Box::new(recorder)));
        click(cx, "composer-talk".to_string());
        assert!(tape.borrow().recording);
        assert!(cx.debug_bounds("composer-cancel-recording").is_some());
        click(cx, "composer-talk".to_string());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.recording(), &Recording::Idle);
        });
        let mut steps = 0;
        while mock.step() && steps < 4 {
            steps += 1;
        }
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let Some(last) = state.messages().last() else {
                panic!("the voice message arrived");
            };
            assert_eq!(last.author, Author::User);
            assert!(last.voice.is_some());
        });
    }

    #[gpui::test]
    fn a_cancelled_recording_posts_nothing(cx: &mut TestAppContext) {
        let (_mock, state, _feed, cx) = feed(cx);
        let recorder = FakeRecorder::default();
        let tape = recorder.0.clone();
        state.update(cx, |state, _cx| state.set_recorder(Box::new(recorder)));
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        click(cx, "composer-talk".to_string());
        click(cx, "composer-cancel-recording".to_string());
        assert_eq!(tape.borrow().cancelled, 1);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.recording(), &Recording::Idle);
            assert_eq!(state.messages().len(), before);
        });
    }

    #[gpui::test]
    fn a_recording_the_daemon_refuses_shows_its_reason(cx: &mut TestAppContext) {
        let (_mock, state, _feed, cx) = feed(cx);
        let recorder = FakeRecorder::default();
        recorder.0.borrow_mut().take = Some(vec![0; 21 * 1024 * 1024]);
        state.update(cx, |state, _cx| state.set_recorder(Box::new(recorder)));
        click(cx, "composer-talk".to_string());
        click(cx, "composer-talk".to_string());
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.recording(),
                &Recording::Failed("the recording must be at most 20971520 bytes".into())
            );
        });
        assert!(cx.debug_bounds("composer-voice-error").is_some());
    }

    #[gpui::test]
    fn a_microphone_that_refuses_shows_why(cx: &mut TestAppContext) {
        let (_mock, state, _feed, cx) = feed(cx);
        let recorder = FakeRecorder::default();
        recorder.0.borrow_mut().refuse = Some("the microphone is not available".into());
        state.update(cx, |state, _cx| state.set_recorder(Box::new(recorder)));
        click(cx, "composer-talk".to_string());
        assert!(cx.debug_bounds("composer-voice-error").is_some());
        click(cx, "composer-dismiss-voice-error".to_string());
        assert!(cx.debug_bounds("composer-voice-error").is_none());
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

    fn suggested(
        mock: &MockTransport,
        state: &Entity<AppState>,
        cx: &mut VisualTestContext,
    ) -> i64 {
        mock.agent_suggests(
            tuclaw_core::v3::SurfaceId(1),
            tuclaw_core::v3::AgentId(1),
            "Book the 21:50 show?",
            &["Do it", "Skip"],
        );
        while mock.step() {}
        cx.run_until_parked();
        last_agent_message(state, cx)
    }

    fn choice_of(
        state: &Entity<AppState>,
        cx: &mut VisualTestContext,
        raw: i64,
    ) -> Option<tuclaw_core::model::Choice> {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.id == tuclaw_core::model::MessageId(raw) {
                    found = message.suggestions.as_ref().map(|s| s.choice.clone());
                }
            }
            found
        })
    }

    #[gpui::test]
    fn a_tapped_reply_is_posted_as_a_reply_and_marked_chosen(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let raw = suggested(&mock, &state, cx);
        let second: &'static str = Box::leak(format!("message-{raw}-reply-1").into_boxed_str());
        assert!(cx.debug_bounds(second).is_some());
        click(cx, format!("message-{raw}-reply-0"));
        assert_eq!(
            choice_of(&state, cx, raw),
            Some(tuclaw_core::model::Choice::Chosen("Do it".into()))
        );
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let mut replies = Vec::new();
            for message in state.messages() {
                if message.reply_to == Some(tuclaw_core::model::MessageId(raw)) {
                    replies.push(message.id);
                }
            }
            assert_eq!(replies.len(), 1);
            let tuclaw_core::model::MessageId(id) = replies[0];
            assert!(id > 0, "the local copy was replaced by the posted message");
        });
        assert_eq!(
            choice_of(&state, cx, raw),
            Some(tuclaw_core::model::Choice::Chosen("Do it".into()))
        );
    }

    #[gpui::test]
    fn a_tap_that_loses_the_race_shows_the_winning_option(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let raw = suggested(&mock, &state, cx);
        let other = tuclaw_core::v3::Client::mock(&mock);
        futures::executor::block_on(other.reply(
            tuclaw_core::v3::MessageId(raw),
            &tuclaw_core::v3::ReplyPost {
                option: "Skip".into(),
                client_message_id: tuclaw_core::v3::ClientMessageId(
                    "c0ffee00-0000-4000-8000-0000000000aa".into(),
                ),
            },
        ))
        .expect("the other client taps first");
        click(cx, format!("message-{raw}-reply-0"));
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        assert_eq!(
            choice_of(&state, cx, raw),
            Some(tuclaw_core::model::Choice::Chosen("Skip".into()))
        );
        state.read_with(cx, |state, _cx| {
            for message in state.messages() {
                let tuclaw_core::model::MessageId(id) = message.id;
                assert!(id > 0, "the refused local copy is gone");
            }
        });
    }

    #[gpui::test]
    fn typed_text_closes_open_replies(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let raw = suggested(&mock, &state, cx);
        state.update(cx, |state, cx| {
            state.send("Let me think".to_string(), cx).expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        assert_eq!(
            choice_of(&state, cx, raw),
            Some(tuclaw_core::model::Choice::Closed)
        );
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

    fn check(minute: i64) -> super::Entry {
        super::Entry::Check(
            time::macros::datetime!(2026-10-05 00:00 UTC) + time::Duration::minutes(minute),
            tuclaw_core::v3::Outcome::Skipped,
        )
    }

    fn boundary(minute: i64) -> super::Entry {
        super::Entry::Item(
            time::macros::datetime!(2026-10-05 00:00 UTC) + time::Duration::minutes(minute),
            Box::new(Item::Unread(super::Fresh::default())),
        )
    }

    fn quiet_checks(timeline: &[(time::OffsetDateTime, Item)]) -> Vec<usize> {
        let mut checks = Vec::new();
        for (_at, item) in timeline {
            if let Item::Quiet(quiet) = item {
                checks.push(quiet.checks);
            }
        }
        checks
    }

    #[test]
    fn checks_in_an_hour_long_pause_fold_into_one_quiet_line() {
        let now = time::macros::datetime!(2026-10-05 12:00 UTC);
        let long = super::fold_quiet(
            vec![boundary(0), check(10), check(25), check(70), boundary(180)],
            now,
        );
        assert_eq!(quiet_checks(&long), vec![3]);
        assert_eq!(long.len(), 3);
        let short = super::fold_quiet(vec![boundary(0), check(10), check(20), boundary(40)], now);
        assert!(quiet_checks(&short).is_empty());
        assert_eq!(short.len(), 2);
    }

    #[test]
    fn a_boundary_splits_the_quiet_stretch_and_the_tail_runs_to_now() {
        let now = time::macros::datetime!(2026-10-05 08:00 UTC);
        let timeline = super::fold_quiet(
            vec![
                boundary(0),
                check(30),
                check(90),
                boundary(120),
                check(150),
                check(300),
            ],
            now,
        );
        assert_eq!(quiet_checks(&timeline), vec![2, 2]);
        let recent = time::macros::datetime!(2026-10-05 05:20 UTC);
        let fresh = super::fold_quiet(vec![boundary(300), check(310)], recent);
        assert!(quiet_checks(&fresh).is_empty());
    }

    #[gpui::test]
    fn the_header_counts_the_automations_and_opens_their_panel(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("feed-automations").is_some());
        mock.fire_task(
            &tuclaw_core::v3::TaskId("task-download-done".into()),
            tuclaw_core::v3::Outcome::Failed,
        );
        while mock.step() {}
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| assert_eq!(state.unseen_failures(), 1));
        click(cx, "feed-automations".to_string());
        state.read_with(cx, |state, _cx| {
            assert!(state.automations_open());
            assert_eq!(state.unseen_failures(), 0);
        });
        click(cx, "feed-automations".to_string());
        state.read_with(cx, |state, _cx| assert!(!state.automations_open()));
    }

    #[test]
    fn the_pulse_names_its_count_and_last_check() {
        let at = time::macros::datetime!(2026-10-05 13:15 UTC);
        let pulse = super::Pulse {
            automations: 1,
            last: Some(at),
            failed: 0,
            open: false,
        };
        assert_eq!(
            super::pulse_text(&pulse),
            format!("1 automation · checked {}", crate::local::clock(at))
        );
        let idle = super::Pulse {
            automations: 5,
            last: None,
            ..pulse
        };
        assert_eq!(super::pulse_text(&idle), "5 automations");
    }

    #[gpui::test]
    fn dragging_across_a_message_selects_and_copies_its_text(cx: &mut TestAppContext) {
        let (mock, state) = loaded(cx);
        let built = state.clone();
        let (_root, cx) = cx.add_window_view(move |window, cx| {
            let feed = gpui::AppContext::new(cx, |cx| Feed::new(built, window, cx));
            gpui_kit::base::Root::new(feed, window, cx)
        });
        let raw = posted_by_jarvis(&mock, &state, cx, "Copy this sentence please.");
        let selector: &'static str = Box::leak(format!("message-{raw}-md").into_boxed_str());
        let bounds = cx
            .debug_bounds(selector)
            .expect("the message text is drawn");
        let start = gpui::point(bounds.left() + gpui::px(1.), bounds.center().y);
        let end = gpui::point(bounds.right() - gpui::px(1.), bounds.center().y);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_move(
            end,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-c");
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("Copy this sentence please."));
    }

    #[gpui::test]
    fn a_reply_that_came_while_away_is_striped_until_it_has_been_seen(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        cx.update(|window, _cx| window.activate_window());
        cx.run_until_parked();
        cx.deactivate_window();
        state.read_with(cx, |state, _cx| assert!(!state.is_window_active()));
        let raw = posted_by_jarvis(&mock, &state, cx, "Back with the forecast.");
        let stripe: &'static str = Box::leak(format!("message-{raw}-stripe").into_boxed_str());
        assert!(cx.debug_bounds(stripe).is_some());
        state.read_with(cx, |state, _cx| {
            let channel = state
                .channels()
                .iter()
                .find(|channel| channel.name == "General")
                .expect("General exists");
            assert_eq!((channel.unread, channel.replies), (1, 1));
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(5));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds(stripe).is_some(),
            "an inactive window sees nothing"
        );
        cx.update(|window, _cx| window.activate_window());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds(stripe).is_some(),
            "seen only after a moment on screen"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(3));
        cx.run_until_parked();
        assert!(cx.debug_bounds(stripe).is_none());
        assert!(
            cx.debug_bounds("feed-unread").is_some(),
            "New stays until another channel"
        );
    }

    #[gpui::test]
    fn seeing_the_newest_reply_reads_the_older_ones_too(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        state.update(cx, |state, cx| state.set_window_active(false, cx));
        let first = posted_by_jarvis(&mock, &state, cx, "First, above the view.");
        let last = posted_by_jarvis(&mock, &state, cx, "Second, the newest.");
        state.update(cx, |state, cx| state.set_window_active(true, cx));
        cx.run_until_parked();
        state.update(cx, |state, cx| {
            state.mark_seen(vec![tuclaw_core::model::MessageId(last)], cx)
        });
        cx.run_until_parked();
        let stripe: &'static str = Box::leak(format!("message-{first}-stripe").into_boxed_str());
        assert!(cx.debug_bounds(stripe).is_none());
        assert_eq!(unread_of(&state, cx, "General"), 0);
    }

    #[gpui::test]
    fn a_reply_below_the_view_raises_a_pill_that_scrolls_to_it(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        state.update(cx, |state, _cx| state.set_following(false));
        posted_by_jarvis(&mock, &state, cx, "Done, the file is in place.");
        state.read_with(cx, |state, _cx| {
            let general = state
                .channels()
                .iter()
                .find(|channel| channel.name == "General")
                .expect("General exists");
            assert_eq!(general.replies, 1, "not read while scrolled up");
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("feed-new-pill").is_some());
        click(cx, "feed-new-pill".to_string());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| assert!(state.following()));
        assert!(cx.debug_bounds("feed-new-pill").is_none());
    }

    #[test]
    fn a_message_counts_as_seen_when_it_fits_or_fills_the_view() {
        let view = gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(100.)),
            gpui::size(gpui::px(800.), gpui::px(500.)),
        );
        let at = |top: f32, height: f32| {
            gpui::Bounds::new(
                gpui::point(gpui::px(0.), gpui::px(top)),
                gpui::size(gpui::px(800.), gpui::px(height)),
            )
        };
        assert!(super::on_screen(at(150., 80.), view));
        assert!(
            !super::on_screen(at(560., 80.), view),
            "cut by the bottom edge"
        );
        assert!(!super::on_screen(at(600., 80.), view), "below the view");
        assert!(
            super::on_screen(at(50., 900.), view),
            "taller than the view and filling it"
        );
    }

    #[test]
    fn the_new_divider_counts_replies_and_posts() {
        let fresh = |replies, posts| super::Fresh { replies, posts };
        assert_eq!(super::fresh_text(fresh(0, 0)), "New");
        assert_eq!(super::fresh_text(fresh(1, 0)), "New · 1 reply");
        assert_eq!(super::fresh_text(fresh(3, 2)), "New · 3 replies, 2 posts");
        assert_eq!(super::fresh_text(fresh(0, 1)), "New · 1 post");
    }

    #[gpui::test]
    fn the_copy_button_on_a_quote_copies_only_the_quote(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let raw = posted_by_jarvis(
            &mock,
            &state,
            cx,
            "Steven answered:\n\n> Everything urgent is done.\n> Flag the rest.\n\nThat is all.",
        );
        let copy: &'static str =
            Box::leak(format!("message-{raw}-md-1-quote-copy").into_boxed_str());
        click(cx, copy.to_string());
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(
            copied.as_deref(),
            Some("Everything urgent is done.\nFlag the rest.")
        );
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
            let mut marks = 0;
            let mut triggered = 0;
            for item in feed.items.iter() {
                match item {
                    Item::Failed(row) => {
                        assert_ne!(row.label, "An automation");
                        marks += 1;
                    }
                    Item::Quiet(_) => marks += 1,
                    Item::Message(_, Some(_)) => triggered += 1,
                    Item::Message(_, None) => {}
                    Item::Separator(_) => {}
                    Item::Unread(_) => {}
                    Item::Run(_) => {}
                }
            }
            assert_eq!(triggered, 1);
            assert_eq!(feed.items.len(), 31 + marks);
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
                    Item::Unread(_) => {}
                    Item::Message(_, _) => {}
                    Item::Failed(_) => {}
                    Item::Quiet(_) => {}
                }
            }
            runs
        })
    }

    fn position(feed: &Entity<Feed>, cx: &mut VisualTestContext, wanted: &Wanted) -> usize {
        feed.read_with(cx, |feed, _cx| {
            for (index, item) in feed.items.iter().enumerate() {
                let found = match (item, wanted) {
                    (Item::Run(run), Wanted::Run(id)) => run.id.as_ref() == Some(id),
                    (Item::Message(message, _), Wanted::Text(text)) => {
                        crate::message::source(&message.body) == *text
                    }
                    (Item::Run(_), Wanted::Text(_)) => false,
                    (Item::Message(_, _), Wanted::Run(_)) => false,
                    (Item::Separator(_), _) => false,
                    (Item::Unread(_), _) => false,
                    (Item::Failed(_), _) => false,
                    (Item::Quiet(_), _) => false,
                };
                if found {
                    return index;
                }
            }
            panic!("the feed has no {wanted:?}")
        })
    }

    #[derive(Debug)]
    enum Wanted {
        Run(tuclaw_core::v3::RunId),
        Text(String),
    }

    #[gpui::test]
    fn the_reply_action_quotes_a_message_and_the_reply_links_back(cx: &mut TestAppContext) {
        let (mock, state, _feed, cx) = feed(cx);
        let quoted = state.read_with(cx, |state, _cx| {
            let messages = state.messages();
            let tuclaw_core::model::MessageId(raw) = messages[messages.len() - 3].id;
            raw
        });
        click(cx, format!("message-{quoted}-reply"));
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.replying().map(|message| message.id),
                Some(tuclaw_core::model::MessageId(quoted))
            )
        });
        assert!(cx.debug_bounds("composer-reply").is_some());
        state.update(cx, |state, cx| {
            state
                .send("About the first one".to_string(), cx)
                .expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        assert!(cx.debug_bounds("composer-reply").is_none());
        let reply = state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_to == Some(tuclaw_core::model::MessageId(quoted)) {
                    let tuclaw_core::model::MessageId(raw) = message.id;
                    found = Some(raw);
                }
            }
            found.expect("the reply is in the feed")
        });
        let link: &'static str = Box::leak(format!("message-{reply}-quote-link").into_boxed_str());
        assert!(
            cx.debug_bounds(link).is_some(),
            "the reply shows what it quotes"
        );
    }

    #[gpui::test]
    fn a_stopped_run_stays_where_it_started(cx: &mut TestAppContext) {
        let (mock, state, feed, cx) = feed(cx);
        state.update(cx, |state, cx| {
            state
                .send("Find the release notes".to_string(), cx)
                .expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        for _ in 0..4 {
            mock.step();
        }
        cx.run_until_parked();
        let started = runs(&feed, cx);
        let Some(run) = started[0].id.clone() else {
            panic!("the run started");
        };
        state.update(cx, |state, cx| state.interrupt(&run, cx));
        cx.run_until_parked();
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        state.update(cx, |state, cx| {
            state
                .send("Never mind, what is the weather?".to_string(), cx)
                .expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        while mock.step() {}
        cx.run_until_parked();
        let stopped = runs(&feed, cx);
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].state, RunState::Interrupted);
        let first = position(&feed, cx, &Wanted::Text("Find the release notes".into()));
        let block = position(&feed, cx, &Wanted::Run(run));
        let second = position(
            &feed,
            cx,
            &Wanted::Text("Never mind, what is the weather?".into()),
        );
        assert!(
            first < block,
            "the stopped run follows the message before it"
        );
        assert!(block < second, "and stays above what came after it");
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
            tasks: Vec::new(),
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
