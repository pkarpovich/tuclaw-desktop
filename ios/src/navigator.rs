use gpui::{Context, Entity, Subscription};
use tuclaw_core::model::{ChannelId, MessageId};
use tuclaw_desktop::state::{AppState, Presence, StateEvent};

use crate::keyboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Automations,
    Agents,
    You,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Conversation,
    Channels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Channel(ChannelId),
    Message(MessageId),
}

pub struct Navigator {
    state: Entity<AppState>,
    tab: Tab,
    stack: Vec<Screen>,
    menu: Option<Menu>,
    _events: Subscription,
}

impl Navigator {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Navigator {
        state.update(cx, |state, cx| {
            state.set_feed(Presence::Hidden, cx);
            state.keep_previews(cx);
        });
        let events = cx.subscribe(
            &state,
            |navigator, _state, event: &StateEvent, cx| match event {
                StateEvent::TaskOpened => navigator.show_task(cx),
                StateEvent::RunViewed => navigator.show_selected(cx),
                StateEvent::SelectionChanged => {}
                StateEvent::MessagesLoaded => {}
                StateEvent::MessageAppended => {}
                StateEvent::RunsChanged => {}
                StateEvent::FoldToggled => {}
                StateEvent::OlderLoaded => {}
                StateEvent::PicturesLoaded => {}
                StateEvent::PictureOpened => {}
                StateEvent::SendFailed(_) => {}
                StateEvent::Mention(_) => {}
                StateEvent::TasksLoaded => {}
                StateEvent::ChannelsChanged => {}
                StateEvent::Alert(_) => {}
                StateEvent::ReplyStarted => {}
                StateEvent::FeedShown => {}
            },
        );
        Navigator {
            state,
            tab: Tab::Home,
            stack: Vec::new(),
            menu: None,
            _events: events,
        }
    }

    fn show_task(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.stack.clear();
        self.tab = Tab::Automations;
        keyboard::hide();
        self.state.update(cx, |state, cx| {
            state.close_automations(cx);
            state.set_feed(Presence::Hidden, cx);
        });
        cx.notify();
    }

    fn show_selected(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.state
            .update(cx, |state, cx| state.set_feed(Presence::Shown, cx));
        self.stack.retain(|screen| *screen != Screen::Conversation);
        self.stack.push(Screen::Conversation);
        cx.notify();
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn top(&self) -> Option<Screen> {
        self.stack.last().copied()
    }

    pub fn menu(&self) -> Option<Menu> {
        self.menu
    }

    pub fn switch(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.menu = None;
        keyboard::hide();
        match tab {
            Tab::Automations => self.state.update(cx, |state, cx| state.load_tasks(cx)),
            Tab::Home => {}
            Tab::Agents => {}
            Tab::You => {}
        }
        cx.notify();
    }

    pub fn open_channels(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        keyboard::hide();
        self.state.update(cx, |state, cx| state.open_channels(cx));
        self.stack.retain(|screen| *screen != Screen::Channels);
        self.stack.push(Screen::Channels);
        cx.notify();
    }

    pub fn open_channel(&mut self, channel: ChannelId, cx: &mut Context<Self>) {
        self.menu = None;
        self.state.update(cx, |state, cx| {
            state.select(channel, cx);
            state.set_feed(Presence::Shown, cx);
        });
        self.stack.retain(|screen| *screen != Screen::Conversation);
        self.stack.push(Screen::Conversation);
        cx.notify();
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.stack.pop();
        keyboard::hide();
        if !self.stack.contains(&Screen::Conversation) {
            self.state
                .update(cx, |state, cx| state.set_feed(Presence::Hidden, cx));
        }
        cx.notify();
    }

    pub fn open_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        keyboard::hide();
        self.menu = Some(menu);
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }
}
