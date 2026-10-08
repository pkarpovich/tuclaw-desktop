use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, FontWeight, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use tuclaw_core::model::{ChannelId, MessageId};
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::inspector;
use tuclaw_desktop::message::source;
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::{theme, viewer};

use crate::conversation::Conversation;
use crate::frame;
use crate::home::{Home, TAB_BAR_HEIGHT};
use crate::keyboard;
use crate::navigator::{Menu, Navigator, Screen, Tab};

pub struct Phone {
    state: Entity<AppState>,
    navigator: Entity<Navigator>,
    home: Entity<Home>,
    conversation: Entity<Conversation>,
    viewer_focus: FocusHandle,
    _observation: Subscription,
    _navigation: Subscription,
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
        let conversation =
            cx.new(|cx| Conversation::new(state.clone(), navigator.clone(), window, cx));
        Phone {
            state,
            navigator,
            home,
            conversation,
            viewer_focus: cx.focus_handle(),
            _observation: observation,
            _navigation: navigation,
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
            .absolute()
            .left(px(14.))
            .right(px(14.))
            .bottom(frame::insets().bottom - px(8.))
            .child(bar)
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = inspector::from_state(&self.state, cx)?;
        let insets = frame::insets();
        Some(
            div()
                .id("phone-inspector")
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

fn placeholder(title: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(theme::window())
        .pt(frame::insets().top)
        .px(px(18.))
        .child(
            div()
                .text_size(px(28.))
                .font_weight(FontWeight::BOLD)
                .child(title),
        )
        .child(
            div()
                .pt(px(8.))
                .text_size(px(14.))
                .text_color(theme::text_label())
                .child("Coming in a later build."),
        )
        .into_any_element()
}

impl Render for Phone {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let navigator = self.navigator.read(cx);
        let tab = navigator.tab();
        let top = navigator.top();
        let menu = navigator.menu();
        let screen: AnyElement = match top {
            Some(Screen::Conversation) => self.conversation.clone().into_any_element(),
            None => match tab {
                Tab::Home => self.home.clone().into_any_element(),
                Tab::Automations => placeholder("Automations"),
                Tab::Agents => placeholder("Agents"),
                Tab::You => placeholder("You"),
            },
        };
        let bar = match top {
            Some(Screen::Conversation) => None,
            None => Some(self.tab_bar(tab)),
        };
        let inspector = match top {
            Some(Screen::Conversation) => self.inspector(cx),
            None => None,
        };
        let sheet = menu.map(|menu| self.sheet(menu, cx));
        let picture = viewer::viewer(&self.state, &self.viewer_focus, window, cx);
        div()
            .relative()
            .size_full()
            .bg(theme::card())
            .text_color(theme::text_primary())
            .child(screen)
            .children(bar)
            .children(inspector)
            .children(sheet)
            .children(picture)
    }
}
