#![allow(dead_code)]

use anyhow::{Result, bail};
use gpui::{Context, EventEmitter};
use time::OffsetDateTime;
use tuclaw_core::model::{Agent, Channel, ChannelId, ChannelKind, Message, MessageId, Span};
use tuclaw_core::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Conversation,
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Channel,
    Direct,
    Agents,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenThread {
    pub root: Message,
    pub channel: ChannelId,
    pub replies: Vec<Message>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEvent {
    SelectionChanged,
    MessageAppended,
    ReplyAppended,
    ThreadOpened,
    ThreadClosed,
}

pub struct AppState {
    store: Store,
    agents: Vec<Agent>,
    channels: Vec<Channel>,
    selected: ChannelId,
    messages: Vec<Message>,
    thread: Option<OpenThread>,
    view: View,
    last_channel: Option<ChannelId>,
    last_direct: Option<ChannelId>,
}

impl EventEmitter<StateEvent> for AppState {}

impl AppState {
    pub fn new(store: Store) -> Result<AppState> {
        let agents = store.agents()?;
        let channels = store.channels()?;
        let Some(Channel {
            id,
            name: _,
            group: _,
            kind,
            unread: _,
            sort_index: _,
        }) = channels.first()
        else {
            bail!("the workspace carries no channels");
        };
        let selected = *id;
        let (last_channel, last_direct) = match kind {
            ChannelKind::Channel => (Some(selected), None),
            ChannelKind::Direct(_) => (None, Some(selected)),
        };
        let messages = store.messages(selected)?;
        Ok(AppState {
            store,
            agents,
            channels,
            selected,
            messages,
            thread: None,
            view: View::Conversation,
            last_channel,
            last_direct,
        })
    }

    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    pub fn selected(&self) -> ChannelId {
        self.selected
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn thread(&self) -> Option<&OpenThread> {
        self.thread.as_ref()
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn select(&mut self, channel: ChannelId, cx: &mut Context<Self>) {
        let Some(kind) = self.kind_of(channel) else {
            return;
        };
        self.selected = channel;
        self.messages = match self.store.messages(channel) {
            Ok(messages) => messages,
            Err(error) => {
                eprintln!("could not load the messages of the selected channel: {error}");
                Vec::new()
            }
        };
        match kind {
            ChannelKind::Channel => self.last_channel = Some(channel),
            ChannelKind::Direct(_) => self.last_direct = Some(channel),
        }
        self.view = View::Conversation;
        cx.emit(StateEvent::SelectionChanged);
        cx.notify();
    }

    pub fn active_segment(&self) -> Segment {
        match self.view {
            View::Agents => Segment::Agents,
            View::Conversation => {
                let Some(kind) = self.kind_of(self.selected) else {
                    return Segment::Channel;
                };
                match kind {
                    ChannelKind::Channel => Segment::Channel,
                    ChannelKind::Direct(_) => Segment::Direct,
                }
            }
        }
    }

    pub fn activate_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        match segment {
            Segment::Channel => {
                let target = match self.last_channel {
                    Some(channel) => Some(channel),
                    None => self.first_of_kind(Segment::Channel),
                };
                let Some(target) = target else {
                    return;
                };
                self.select(target, cx);
            }
            Segment::Direct => {
                let target = match self.last_direct {
                    Some(channel) => Some(channel),
                    None => self.first_of_kind(Segment::Direct),
                };
                let Some(target) = target else {
                    return;
                };
                self.select(target, cx);
            }
            Segment::Agents => {
                self.view = View::Agents;
                cx.notify();
            }
        }
    }

    pub fn send(&mut self, body: String, cx: &mut Context<Self>) -> Result<()> {
        let body = body.trim();
        if body.is_empty() {
            return Ok(());
        }
        let body = vec![Span::Text(body.to_string())];
        let sent = match self
            .store
            .send(self.selected, &body, OffsetDateTime::now_utc())
        {
            Ok(sent) => sent,
            Err(error) => {
                eprintln!("could not send the message: {error}");
                return Err(error);
            }
        };
        self.messages.push(sent);
        cx.emit(StateEvent::MessageAppended);
        cx.notify();
        Ok(())
    }

    pub fn open_thread(&mut self, root: MessageId, cx: &mut Context<Self>) {
        let root = match self.store.message(root) {
            Ok(root) => root,
            Err(error) => {
                eprintln!("could not load the thread's root message: {error}");
                return;
            }
        };
        let replies = match self.store.thread(root.id) {
            Ok(replies) => replies,
            Err(error) => {
                eprintln!("could not load the thread's replies: {error}");
                return;
            }
        };
        self.thread = Some(OpenThread {
            root,
            channel: self.selected,
            replies,
        });
        cx.emit(StateEvent::ThreadOpened);
        cx.notify();
    }

    pub fn close_thread(&mut self, cx: &mut Context<Self>) {
        self.thread = None;
        cx.emit(StateEvent::ThreadClosed);
        cx.notify();
    }

    pub fn reply_in_thread(&mut self, body: String, cx: &mut Context<Self>) -> Result<()> {
        let body = body.trim();
        if body.is_empty() {
            return Ok(());
        }
        let Some(OpenThread {
            root,
            channel,
            replies: _,
        }) = &self.thread
        else {
            return Ok(());
        };
        let root = root.id;
        let channel = *channel;
        let body = vec![Span::Text(body.to_string())];
        let reply = match self
            .store
            .reply(root, channel, &body, OffsetDateTime::now_utc())
        {
            Ok(reply) => reply,
            Err(error) => {
                eprintln!("could not send the reply: {error}");
                return Err(error);
            }
        };
        let Some(thread) = &mut self.thread else {
            return Ok(());
        };
        thread.replies.push(reply);
        thread.root.reply_count += 1;
        for message in &mut self.messages {
            if message.id == root {
                message.reply_count += 1;
            }
        }
        cx.emit(StateEvent::ReplyAppended);
        cx.notify();
        Ok(())
    }

    fn kind_of(&self, channel: ChannelId) -> Option<ChannelKind> {
        let mut found = None;
        for Channel {
            id,
            name: _,
            group: _,
            kind,
            unread: _,
            sort_index: _,
        } in &self.channels
        {
            if *id == channel {
                found = Some(*kind);
                break;
            }
        }
        found
    }

    fn first_of_kind(&self, segment: Segment) -> Option<ChannelId> {
        let mut found = None;
        for Channel {
            id,
            name: _,
            group: _,
            kind,
            unread: _,
            sort_index: _,
        } in &self.channels
        {
            let matched = match (segment, kind) {
                (Segment::Channel, ChannelKind::Channel) => true,
                (Segment::Direct, ChannelKind::Direct(_)) => true,
                (Segment::Channel, ChannelKind::Direct(_)) => false,
                (Segment::Direct, ChannelKind::Channel) => false,
                (Segment::Agents, ChannelKind::Channel) => false,
                (Segment::Agents, ChannelKind::Direct(_)) => false,
            };
            if matched {
                found = Some(*id);
                break;
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui::{AppContext, Entity, Subscription, TestAppContext};
    use time::OffsetDateTime;
    use time::macros::datetime;
    use tuclaw_core::model::{ChannelId, ChannelKind, MessageId, Span};
    use tuclaw_core::store::Store;

    use super::{AppState, Segment, StateEvent, View};

    fn seeded_store() -> Store {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        store
    }

    fn seeded(cx: &mut TestAppContext) -> Entity<AppState> {
        let state = AppState::new(seeded_store()).expect("the workspace loads");
        cx.new(|_| state)
    }

    fn readonly_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tuclaw-state-{name}-{}.sqlite", std::process::id()))
    }

    fn readonly_store(path: &PathBuf) -> Store {
        let _ = fs::remove_file(path);
        let store = Store::open(path).expect("the database is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        drop(store);
        let mut permissions = fs::metadata(path)
            .expect("the database exists")
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("the database is made read-only");
        Store::open(path).expect("the read-only database opens")
    }

    fn remove_readonly(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(path.with_extension("sqlite-journal"));
    }

    fn events(
        state: &Entity<AppState>,
        cx: &mut TestAppContext,
    ) -> (Rc<RefCell<Vec<StateEvent>>>, Subscription) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(state, move |_state, event: &StateEvent, _cx| {
                sink.borrow_mut().push(*event);
            })
        });
        (events, subscription)
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

    fn thread_root(state: &Entity<AppState>, cx: &mut TestAppContext) -> MessageId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if message.reply_count > 0 {
                    found = Some(message.id);
                    break;
                }
            }
            found.expect("movie-night carries a thread root")
        })
    }

    #[gpui::test]
    fn construction_opens_the_first_channel(cx: &mut TestAppContext) {
        let state = seeded(cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.agents().len(), 4);
            assert_eq!(state.channels().len(), 10);
            assert_eq!(state.channels()[0].id, state.selected());
            assert_eq!(state.messages().len(), 58);
            assert_eq!(state.thread(), None);
            assert_eq!(state.view(), View::Conversation);
        });
    }

    #[gpui::test]
    fn a_workspace_without_channels_does_not_load(_cx: &mut TestAppContext) {
        let store = Store::open_in_memory().expect("the schema is created");
        assert!(AppState::new(store).is_err());
    }

    #[gpui::test]
    fn a_blank_body_sends_nothing_and_emits_nothing(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let (events, _subscription) = events(&state, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        state.update(cx, |state, cx| {
            state
                .send("   \n  ".to_string(), cx)
                .expect("a blank body is not an error")
        });
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before);
        });
        assert!(events.borrow().is_empty());
    }

    #[gpui::test]
    fn sending_appends_the_stored_message_and_emits(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let (events, _subscription) = events(&state, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        state.update(cx, |state, cx| {
            state
                .send("  подтверждаю 🐢  ".to_string(), cx)
                .expect("the message is written")
        });
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before + 1);
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("подтверждаю 🐢".to_string())]);
            assert_eq!(last.reply_count, 0);
        });
        assert_eq!(*events.borrow(), vec![StateEvent::MessageAppended]);
    }

    #[gpui::test]
    fn a_rejected_write_leaves_the_messages_untouched(cx: &mut TestAppContext) {
        let path = readonly_path("send");
        let state = AppState::new(readonly_store(&path)).expect("the workspace loads");
        let state = cx.new(|_| state);
        let (events, _subscription) = events(&state, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        let failed = state.update(cx, |state, cx| state.send("on it".to_string(), cx));
        assert!(failed.is_err());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before);
        });
        assert!(events.borrow().is_empty());
        remove_readonly(&path);
    }

    #[gpui::test]
    fn selecting_leaves_an_open_thread_open(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        let (events, _subscription) = events(&state, cx);
        let personal = channel_named(&state, cx, "personal");
        state.update(cx, |state, cx| state.select(personal, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.selected(), personal);
            assert!(state.messages().is_empty());
            let thread = state.thread().expect("the thread stays open");
            assert_eq!(thread.root.id, root);
            assert_ne!(thread.channel, personal);
        });
        assert_eq!(*events.borrow(), vec![StateEvent::SelectionChanged]);
    }

    #[gpui::test]
    fn selecting_returns_from_the_agents_view(cx: &mut TestAppContext) {
        let state = seeded(cx);
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.view(), View::Agents));
        let personal = channel_named(&state, cx, "personal");
        state.update(cx, |state, cx| state.select(personal, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.view(), View::Conversation);
            assert_eq!(state.active_segment(), Segment::Channel);
        });
    }

    #[gpui::test]
    fn selecting_an_unknown_channel_changes_nothing(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let (events, _subscription) = events(&state, cx);
        let before = state.read_with(cx, |state, _cx| state.selected());
        state.update(cx, |state, cx| state.select(ChannelId(404), cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.selected(), before));
        assert!(events.borrow().is_empty());
    }

    #[gpui::test]
    fn opening_a_thread_caches_its_root_and_channel(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let (events, _subscription) = events(&state, cx);
        let selected = state.read_with(cx, |state, _cx| state.selected());
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.root.id, root);
            assert_eq!(thread.root.reply_count, 4);
            assert_eq!(thread.channel, selected);
            assert_eq!(thread.replies.len(), 4);
        });
        assert_eq!(*events.borrow(), vec![StateEvent::ThreadOpened]);
    }

    #[gpui::test]
    fn closing_a_thread_clears_it_and_emits(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        let (events, _subscription) = events(&state, cx);
        state.update(cx, |state, cx| state.close_thread(cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.thread(), None));
        assert_eq!(*events.borrow(), vec![StateEvent::ThreadClosed]);
    }

    #[gpui::test]
    fn replying_raises_the_root_count_in_both_places(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        let (events, _subscription) = events(&state, cx);
        state.update(cx, |state, cx| {
            state
                .reply_in_thread("  me too  ".to_string(), cx)
                .expect("the reply is written")
        });
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.replies.len(), 5);
            assert_eq!(
                thread.replies[4].body,
                vec![Span::Text("me too".to_string())]
            );
            assert_eq!(thread.root.reply_count, 5);
            let mut in_feed = None;
            for message in state.messages() {
                if message.id == root {
                    in_feed = Some(message.reply_count);
                    break;
                }
            }
            assert_eq!(in_feed, Some(5));
        });
        assert_eq!(*events.borrow(), vec![StateEvent::ReplyAppended]);
    }

    #[gpui::test]
    fn a_blank_reply_writes_nothing(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        let (events, _subscription) = events(&state, cx);
        state.update(cx, |state, cx| {
            state
                .reply_in_thread("  ".to_string(), cx)
                .expect("a blank body is not an error")
        });
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.replies.len(), 4);
            assert_eq!(thread.root.reply_count, 4);
        });
        assert!(events.borrow().is_empty());
    }

    #[gpui::test]
    fn a_rejected_reply_leaves_the_thread_untouched(cx: &mut TestAppContext) {
        let path = readonly_path("reply");
        let state = AppState::new(readonly_store(&path)).expect("the workspace loads");
        let state = cx.new(|_| state);
        let root = thread_root(&state, cx);
        state.update(cx, |state, cx| state.open_thread(root, cx));
        let (events, _subscription) = events(&state, cx);
        let failed = state.update(cx, |state, cx| {
            state.reply_in_thread("me too".to_string(), cx)
        });
        assert!(failed.is_err());
        state.read_with(cx, |state, _cx| {
            let thread = state.thread().expect("the thread is open");
            assert_eq!(thread.replies.len(), 4);
            assert_eq!(thread.root.reply_count, 4);
        });
        assert!(events.borrow().is_empty());
        remove_readonly(&path);
    }

    #[gpui::test]
    fn replying_without_an_open_thread_writes_nothing(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let (events, _subscription) = events(&state, cx);
        state.update(cx, |state, cx| {
            state
                .reply_in_thread("me too".to_string(), cx)
                .expect("a closed thread is not an error")
        });
        assert!(events.borrow().is_empty());
    }

    #[gpui::test]
    fn the_active_segment_follows_the_selection_and_the_view(cx: &mut TestAppContext) {
        let state = seeded(cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Channel)
        });
        let direct = first_direct(&state, cx);
        state.update(cx, |state, cx| state.select(direct, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Direct)
        });
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Agents)
        });
    }

    #[gpui::test]
    fn activating_a_segment_walks_the_remembered_channels(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let first_channel = state.read_with(cx, |state, _cx| state.selected());
        let archive = channel_named(&state, cx, "media-archive");
        state.update(cx, |state, cx| state.select(archive, cx));
        state.update(cx, |state, cx| state.activate_segment(Segment::Direct, cx));
        let direct = first_direct(&state, cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.selected(), direct);
            assert_eq!(state.active_segment(), Segment::Direct);
        });
        state.update(cx, |state, cx| state.activate_segment(Segment::Channel, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.selected(), archive);
            assert_ne!(state.selected(), first_channel);
        });
    }

    #[gpui::test]
    fn activating_a_segment_from_the_agents_view_reaches_each_kind(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let direct = first_direct(&state, cx);
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.update(cx, |state, cx| state.activate_segment(Segment::Direct, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.view(), View::Conversation);
            assert_eq!(state.selected(), direct);
        });
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.update(cx, |state, cx| state.activate_segment(Segment::Channel, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.view(), View::Conversation);
            assert_eq!(state.active_segment(), Segment::Channel);
        });
    }

    struct Watcher {
        notified: usize,
    }

    #[gpui::test]
    fn an_observer_is_notified_when_the_selection_changes(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let watcher = cx.new(|cx| {
            cx.observe(&state, |watcher: &mut Watcher, _state, _cx| {
                watcher.notified += 1;
            })
            .detach();
            Watcher { notified: 0 }
        });
        watcher.read_with(cx, |watcher, _cx| assert_eq!(watcher.notified, 0));
        let personal = channel_named(&state, cx, "personal");
        state.update(cx, |state, cx| state.select(personal, cx));
        watcher.read_with(cx, |watcher, _cx| assert_eq!(watcher.notified, 1));
    }

    #[gpui::test]
    fn a_sent_message_carries_the_time_it_was_written(cx: &mut TestAppContext) {
        let state = seeded(cx);
        let before = OffsetDateTime::now_utc();
        state.update(cx, |state, cx| {
            state.send("on it".to_string(), cx).expect("it is written")
        });
        state.read_with(cx, |state, _cx| {
            let last = state.messages().last().expect("the message was appended");
            assert!(last.sent_at >= before);
        });
    }
}
