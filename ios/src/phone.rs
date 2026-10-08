use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, FontWeight, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use tuclaw_core::model::{ChannelId, MessageId};
use tuclaw_desktop::agent_settings::Target;
use tuclaw_desktop::automations_panel;
use tuclaw_desktop::automations_view::{AutomationsView, Width};
use tuclaw_desktop::channels_view::ChannelsView;
use tuclaw_desktop::chrome::OnTap;
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::inspector;
use tuclaw_desktop::message::source;
use tuclaw_desktop::profile_panel::{Closing, ProfilePanel};
use tuclaw_desktop::settings_panel::SettingsPanel;
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::{theme, viewer};

use crate::agents_tab::AgentsTab;
use crate::conversation::Conversation;
use crate::frame;
use crate::home::{Home, TAB_BAR_HEIGHT};
use crate::keyboard;
use crate::navigator::{Menu, Navigator, Screen, Tab};
use crate::talk::Talk;

pub struct Phone {
    state: Entity<AppState>,
    navigator: Entity<Navigator>,
    home: Entity<Home>,
    conversation: Entity<Conversation>,
    agents: Entity<AgentsTab>,
    automations: Entity<AutomationsView>,
    profile: Entity<ProfilePanel>,
    channels: Entity<ChannelsView>,
    settings: Option<Entity<SettingsPanel>>,
    talk: Entity<Talk>,
    viewer_focus: FocusHandle,
    _observation: Subscription,
    _navigation: Subscription,
    _talking: Subscription,
}

type Action = Box<dyn Fn(&mut Window, &mut App)>;

struct Choice {
    selector: &'static str,
    label: SharedString,
    tone: Tone,
    action: Action,
}

enum Tone {
    Plain,
    Muted,
}

impl Phone {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Phone {
        let observation = cx.observe(&state, |_phone, _state, cx| cx.notify());
        let navigator = cx.new(|cx| Navigator::new(state.clone(), cx));
        let navigation = cx.observe(&navigator, |_phone, _navigator, cx| cx.notify());
        let home = cx.new(|cx| Home::new(state.clone(), navigator.clone(), window, cx));
        let agents = cx.new(|cx| AgentsTab::new(state.clone(), cx));
        let automations =
            cx.new(|cx| AutomationsView::new(state.clone(), cx).with_width(Width::Narrow));
        let profile =
            cx.new(|cx| ProfilePanel::new(state.clone(), window, cx).with_closing(Closing::Fixed));
        let channels = cx.new(|cx| ChannelsView::new(state.clone(), window, cx));
        let talk = cx.new(|cx| Talk::new(state.clone(), cx));
        let talking = cx.observe(&talk, |_phone, _talk, cx| cx.notify());
        let conversation = cx.new(|cx| {
            Conversation::new(state.clone(), navigator.clone(), talk.clone(), window, cx)
        });
        Phone {
            state,
            navigator,
            home,
            conversation,
            agents,
            automations,
            profile,
            channels,
            settings: None,
            talk,
            viewer_focus: cx.focus_handle(),
            _observation: observation,
            _navigation: navigation,
            _talking: talking,
        }
    }

    fn tab_bar(&self, tab: Tab) -> impl IntoElement {
        let mut bar = div()
            .flex()
            .items_center()
            .h(px(TAB_BAR_HEIGHT))
            .px(px(6.))
            .rounded(px(26.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::hairline())
            .shadow(vec![
                gpui::BoxShadow::new(px(0.), px(6.), theme::shadow())
                    .blur_radius(px(20.))
                    .spread_radius(px(-8.)),
            ]);
        for (item, glyph, label, selector) in [
            (Tab::Home, Glyph::Home, "Home", "tab-home"),
            (
                Tab::Automations,
                Glyph::Automation,
                "Automations",
                "tab-automations",
            ),
            (Tab::Agents, Glyph::Agents, "Agents", "tab-agents"),
            (Tab::You, Glyph::Person, "You", "tab-you"),
        ] {
            let color = if item == tab {
                theme::accent()
            } else {
                theme::text_label()
            };
            let navigator = self.navigator.clone();
            bar = bar.child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_string())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .gap(px(3.))
                    .on_click(move |_event, _window, cx| {
                        navigator.update(cx, |navigator, cx| navigator.switch(item, cx))
                    })
                    .child(icon(glyph, px(22.), color))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(if item == tab {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(color)
                            .child(label),
                    ),
            );
        }
        div()
            .id("tab-bar")
            .occlude()
            .absolute()
            .left(px(14.))
            .right(px(14.))
            .bottom((frame::insets().bottom - px(8.)).max(px(8.)))
            .child(bar)
    }

    fn sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = match self
            .state
            .read(cx)
            .settings()
            .map(|settings| settings.target)
        {
            Some(Target::Agent(agent)) => Some(agent),
            Some(Target::Me) => None,
            None => None,
        };
        let current = self.settings.as_ref().map(|panel| panel.read(cx).agent());
        if wanted == current {
            return;
        }
        let state = self.state.clone();
        self.settings =
            wanted.map(|agent| cx.new(|cx| SettingsPanel::new(state, agent, window, cx)));
        if self.settings.is_none() {
            keyboard::hide();
        }
    }

    fn settings_sheet(&self) -> Option<AnyElement> {
        let panel = self.settings.clone()?;
        let closer = self.state.clone();
        Some(sheet_frame(
            "settings-sheet",
            panel.into_any_element(),
            Rc::new(move |_window, cx| closer.update(cx, |state, cx| state.close_inspector(cx))),
        ))
    }

    fn automations_sheet(&self, cx: &App) -> Option<AnyElement> {
        let panel = automations_panel::from_state(&self.state, cx)?;
        let closer = self.state.clone();
        Some(sheet_frame(
            "automations-sheet",
            panel.into_any_element(),
            Rc::new(move |_window, cx| closer.update(cx, |state, cx| state.close_automations(cx))),
        ))
    }

    fn text_fields(&self, cx: &App) -> Vec<FocusHandle> {
        let mut fields = Vec::new();
        if let Some(panel) = &self.settings {
            fields.extend(panel.read(cx).text_fields(cx));
        }
        let navigator = self.navigator.read(cx);
        match (navigator.top(), navigator.tab()) {
            (Some(Screen::Channels), _) => fields.extend(self.channels.read(cx).text_fields(cx)),
            (Some(Screen::Conversation), _) => {}
            (None, Tab::You) => fields.extend(self.profile.read(cx).text_fields(cx)),
            (None, Tab::Home) => {}
            (None, Tab::Agents) => {}
            (None, Tab::Automations) => {}
        }
        fields
    }

    fn after_tap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = window.focused(cx) else {
            return;
        };
        if self.text_fields(cx).contains(&focused) {
            keyboard::show();
        }
    }

    fn channels_screen(&self) -> AnyElement {
        let navigator = self.navigator.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::card())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .pt(frame::insets().top)
                    .px(px(12.))
                    .pb(px(4.))
                    .child(
                        div()
                            .id("channels-back")
                            .debug_selector(|| "channels-back".to_string())
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .h(px(34.))
                            .text_size(px(17.))
                            .text_color(theme::accent())
                            .on_click(move |_event, _window, cx| {
                                navigator.update(cx, |navigator, cx| navigator.back(cx))
                            })
                            .child(icon(Glyph::Back, px(22.), theme::accent()))
                            .child("Home"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .pb(frame::insets().bottom)
                    .child(self.channels.clone()),
            )
            .into_any_element()
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = inspector::from_state(&self.state, cx)?;
        let insets = frame::insets();
        Some(
            div()
                .id("phone-inspector")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .flex_col()
                .pt(insets.top)
                .pb(insets.bottom)
                .bg(theme::card())
                .child(panel)
                .into_any_element(),
        )
    }

    fn sheet(&self, menu: Menu, cx: &mut Context<Self>) -> AnyElement {
        let choices = match menu {
            Menu::Channel(channel) => self.channel_choices(channel, cx),
            Menu::Message(message) => self.message_choices(message, cx),
        };
        let closer = self.navigator.clone();
        let mut panel = div()
            .flex()
            .flex_col()
            .mx(px(10.))
            .mb(frame::insets().bottom)
            .rounded(px(18.))
            .bg(theme::raised())
            .overflow_hidden();
        let count = choices.len();
        for (index, choice) in choices.into_iter().enumerate() {
            let Choice {
                selector,
                label,
                tone,
                action,
            } = choice;
            let navigator = self.navigator.clone();
            let color = match tone {
                Tone::Plain => theme::text_primary(),
                Tone::Muted => theme::text_label(),
            };
            let mut row = div()
                .id(selector)
                .debug_selector(move || selector.to_string())
                .flex()
                .items_center()
                .justify_center()
                .h(px(54.))
                .text_size(px(17.))
                .text_color(color)
                .on_click(move |_event, window, cx| {
                    navigator.update(cx, |navigator, cx| navigator.close_menu(cx));
                    action(window, cx);
                })
                .child(label);
            if index + 1 < count {
                row = row.border_b_1().border_color(theme::hairline());
            }
            panel = panel.child(row);
        }
        div()
            .id("sheet-scrim")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .bg(theme::scrim())
            .on_click(move |_event, _window, cx| {
                closer.update(cx, |navigator, cx| navigator.close_menu(cx))
            })
            .child(panel)
            .into_any_element()
    }

    fn channel_choices(&self, channel: ChannelId, cx: &mut Context<Self>) -> Vec<Choice> {
        let mut marked = false;
        for candidate in self.state.read(cx).channels() {
            if candidate.id == channel {
                marked = candidate.marked;
            }
        }
        let state = self.state.clone();
        let mark = if marked {
            Choice {
                selector: "sheet-mark-read",
                label: SharedString::new_static("Mark as read"),
                tone: Tone::Plain,
                action: Box::new(move |_window, cx| {
                    state.update(cx, |state, cx| state.clear_unread_mark(channel, cx))
                }),
            }
        } else {
            Choice {
                selector: "sheet-mark-unread",
                label: SharedString::new_static("Mark as unread"),
                tone: Tone::Plain,
                action: Box::new(move |_window, cx| {
                    state.update(cx, |state, cx| state.mark_unread(channel, cx))
                }),
            }
        };
        vec![mark, cancel()]
    }

    fn message_choices(&self, message: MessageId, cx: &mut Context<Self>) -> Vec<Choice> {
        let mut text = None;
        for candidate in self.state.read(cx).messages() {
            if candidate.id == message {
                text = Some(source(&candidate.body));
            }
        }
        let replier = self.state.clone();
        let mut choices = vec![Choice {
            selector: "sheet-reply",
            label: SharedString::new_static("Reply"),
            tone: Tone::Plain,
            action: Box::new(move |_window, cx| {
                replier.update(cx, |state, cx| state.start_reply(message, cx));
                keyboard::show();
            }),
        }];
        if let Some(text) = text {
            choices.push(Choice {
                selector: "sheet-copy",
                label: SharedString::new_static("Copy"),
                tone: Tone::Plain,
                action: Box::new(move |_window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                }),
            });
        }
        choices.push(cancel());
        choices
    }
}

fn cancel() -> Choice {
    Choice {
        selector: "sheet-cancel",
        label: SharedString::new_static("Cancel"),
        tone: Tone::Muted,
        action: Box::new(|_window, _cx| {}),
    }
}

fn sheet_frame(selector: &'static str, content: AnyElement, on_dismiss: OnTap) -> AnyElement {
    div()
        .id(selector)
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .flex_col()
        .justify_end()
        .bg(theme::scrim())
        .child(
            div()
                .id(SharedString::from(format!("{selector}-scrim")))
                .h(frame::insets().top + px(24.))
                .flex_none()
                .on_click(move |_event, window, cx| on_dismiss(window, cx)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .pb(frame::insets().bottom)
                .rounded_t(px(18.))
                .overflow_hidden()
                .bg(theme::card())
                .child(content),
        )
        .into_any_element()
}

fn tab_frame(content: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme::card())
        .pt(frame::insets().top)
        .pb(px(TAB_BAR_HEIGHT + 16.) + frame::insets().bottom)
        .child(content)
        .into_any_element()
}

impl Render for Phone {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_settings(window, cx);
        let navigator = self.navigator.read(cx);
        let tab = navigator.tab();
        let top = navigator.top();
        let menu = navigator.menu();
        let screen: AnyElement = match top {
            Some(Screen::Conversation) => self.conversation.clone().into_any_element(),
            Some(Screen::Channels) => self.channels_screen(),
            None => match tab {
                Tab::Home => self.home.clone().into_any_element(),
                Tab::Automations => tab_frame(self.automations.clone().into_any_element()),
                Tab::Agents => self.agents.clone().into_any_element(),
                Tab::You => tab_frame(self.profile.clone().into_any_element()),
            },
        };
        let bar = match top {
            Some(Screen::Conversation) => None,
            Some(Screen::Channels) => None,
            None => Some(self.tab_bar(tab)),
        };
        let inspector = match top {
            Some(Screen::Conversation) => self.inspector(cx),
            Some(Screen::Channels) => None,
            None => None,
        };
        let automations = match top {
            Some(Screen::Conversation) => self.automations_sheet(cx),
            Some(Screen::Channels) => None,
            None => None,
        };
        let settings = self.settings_sheet();
        let sheet = menu.map(|menu| self.sheet(menu, cx));
        let held = match top {
            Some(Screen::Conversation) => self.talk.read(cx).overlay(cx),
            Some(Screen::Channels) => None,
            None => None,
        };
        let picture = viewer::viewer(&self.state, &self.viewer_focus, window, cx).map(|picture| {
            div()
                .id("phone-picture")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(picture)
        });
        div()
            .relative()
            .size_full()
            .bg(theme::card())
            .text_color(theme::text_primary())
            .child(screen)
            .children(bar)
            .capture_any_mouse_up(
                cx.listener(|phone, _event, window, cx| phone.after_tap(window, cx)),
            )
            .children(inspector)
            .children(automations)
            .children(held)
            .children(settings)
            .children(sheet)
            .children(picture)
    }
}
