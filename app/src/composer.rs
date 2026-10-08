use anyhow::Result;
use gpui::{
    App, BoxShadow, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyBinding, Render, SharedString, Subscription, Window, actions, div, prelude::*, px,
};
use gpui_kit::base::input::{
    Enter, Escape, IndentInline, MoveDown, MoveUp, Textarea, TextareaState,
};

use crate::chrome::Chrome;
use crate::control::{button, row_button};
use crate::icon::{Glyph, icon, spinner};
use crate::message;
use crate::plain;
use crate::state::{AppState, Draft, Mention, Recording};
use crate::theme;

actions!(composer, [ToggleTalk]);

const REASON_LIMIT: usize = 60;

pub type OnSubmit = Box<dyn Fn(Draft, &mut App) -> Result<()>>;

const MAX_ROWS: usize = 10;
const QUOTE_LIMIT: usize = 90;

struct MentionMenu {
    at: usize,
    highlighted: usize,
    candidates: Vec<Mention>,
}

enum Talk {
    Start,
    Send,
}

enum Sendable {
    Blank,
    Ready,
}

pub struct Composer {
    input: Entity<TextareaState>,
    on_submit: OnSubmit,
    state: Option<Entity<AppState>>,
    mentions: Option<MentionMenu>,
    picked: Vec<Mention>,
    chrome: Chrome,
    _observation: Subscription,
    _state_observation: Option<Subscription>,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("alt-space", ToggleTalk, None)]);
}

impl Composer {
    pub fn new(
        placeholder: impl Into<SharedString>,
        on_submit: OnSubmit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Composer {
        let placeholder = placeholder.into();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, MAX_ROWS)
                .submit_on_enter(true)
                .placeholder(placeholder)
        });
        let observation = cx.observe(&input, |composer, _input, cx| {
            composer.refresh_mentions(cx);
            cx.notify()
        });
        Composer {
            input,
            on_submit,
            state: None,
            mentions: None,
            picked: Vec::new(),
            chrome: Chrome::Desktop,
            _observation: observation,
            _state_observation: None,
        }
    }

    pub fn with_state(mut self, state: Entity<AppState>, cx: &mut Context<Self>) -> Composer {
        self._state_observation = Some(cx.observe(&state, |_composer, _state, cx| cx.notify()));
        self.state = Some(state);
        self
    }

    pub fn with_chrome(mut self, chrome: Chrome) -> Composer {
        self.chrome = chrome;
        self
    }

    fn toggle_talk(&mut self, _action: &ToggleTalk, _window: &mut Window, cx: &mut Context<Self>) {
        self.talk(cx);
    }

    fn talk(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        state.update(cx, |state, cx| match state.recording() {
            Recording::Live {
                since: _,
                channel: _,
            } => state.finish_recording(cx),
            Recording::Idle => state.start_recording(cx),
            Recording::Failed(_) => state.start_recording(cx),
            Recording::Sending => {}
        });
    }

    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        state.update(cx, |state, cx| state.cancel_recording(cx));
    }

    fn voice_controls(&self, cx: &mut Context<Self>) -> Div {
        let recording = match &self.state {
            Some(state) => state.read(cx).recording().clone(),
            None => Recording::Idle,
        };
        let row = div().flex().flex_none().items_center().gap(px(8.));
        match recording {
            Recording::Idle => row.child(hint()).child(self.talk_chip(Talk::Start, cx)),
            Recording::Live { since, channel: _ } => {
                let elapsed = cx
                    .background_executor()
                    .now()
                    .saturating_duration_since(since);
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::accent())
                        .child(div().size(px(8.)).rounded_full().bg(theme::accent()))
                        .child(SharedString::from(format!(
                            "Recording {}:{:02}",
                            elapsed.as_secs() / 60,
                            elapsed.as_secs() % 60
                        ))),
                )
                .child(
                    button("composer-cancel-recording")
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(7.))
                        .hover(|style| style.bg(theme::sunken()))
                        .text_size(px(12.5))
                        .text_color(theme::text_secondary())
                        .on_click(cx.listener(|composer, _event, _window, cx| {
                            composer.cancel_recording(cx)
                        }))
                        .child("Cancel"),
                )
                .child(self.talk_chip(Talk::Send, cx))
            }
            Recording::Sending => row.child(
                div()
                    .id("composer-transcribing")
                    .debug_selector(|| "composer-transcribing".to_string())
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(12.))
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(spinner(px(13.), theme::text_muted()))
                    .child("Transcribing…"),
            ),
            Recording::Failed(reason) => {
                let mut reason: String = reason.chars().take(REASON_LIMIT).collect();
                if reason.chars().count() == REASON_LIMIT {
                    reason.push('…');
                }
                row.child(
                    div()
                        .id("composer-voice-error")
                        .debug_selector(|| "composer-voice-error".to_string())
                        .text_size(px(11.5))
                        .text_color(theme::accent())
                        .child(SharedString::from(reason)),
                )
                .child(
                    button("composer-dismiss-voice-error")
                        .accessibility_label("Dismiss")
                        .p(px(4.))
                        .rounded(px(6.))
                        .hover(|style| style.bg(theme::sunken()))
                        .on_click(cx.listener(|composer, _event, _window, cx| {
                            composer.cancel_recording(cx)
                        }))
                        .child(icon(Glyph::Close, px(12.), theme::text_muted())),
                )
                .child(self.talk_chip(Talk::Start, cx))
            }
        }
    }

    fn talk_chip(&self, talk: Talk, cx: &mut Context<Self>) -> gpui_kit::base::Button {
        let chip = button("composer-talk")
            .flex_none()
            .gap(px(7.))
            .px(px(12.))
            .py(px(6.))
            .rounded_full()
            .border_1()
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .on_click(cx.listener(|composer, _event, _window, cx| composer.talk(cx)));
        match talk {
            Talk::Start => chip
                .accessibility_label("Record a voice message")
                .border_color(theme::border())
                .bg(theme::raised())
                .text_color(theme::text_secondary())
                .child(icon(Glyph::Voice, px(13.), theme::accent()))
                .child("Talk"),
            Talk::Send => chip
                .accessibility_label("Send the voice message")
                .border_color(theme::accent())
                .bg(theme::accent())
                .text_color(theme::chip_text())
                .child(icon(Glyph::Send, px(13.), theme::chip_text()))
                .child("Send voice"),
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }

    #[cfg(test)]
    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub fn restore(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
        });
    }

    fn start_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (value, cursor) = {
            let input = self.input.read(cx);
            let value = input.value().to_string();
            let cursor = input.cursor().min(value.len());
            (value, cursor)
        };
        let opens = value
            .get(..cursor)
            .and_then(|before| before.chars().next_back())
            .is_none_or(char::is_whitespace);
        let text = if opens { "@" } else { " @" };
        self.insert(text, window, cx);
    }

    pub fn mention(&mut self, mention: Mention, window: &mut Window, cx: &mut Context<Self>) {
        self.insert(&format!("@{} ", mention.ident), window, cx);
        self.picked.push(mention);
    }

    pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.insert(text.to_string(), window, cx);
            input.focus_handle(cx).focus(window, cx);
        });
    }

    fn enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        let Enter {
            secondary: _,
            shift,
        } = action;
        if *shift {
            cx.propagate();
            return;
        }
        if self.mentions.is_some() {
            self.pick_highlighted(window, cx);
            return;
        }
        match self.chrome {
            Chrome::Desktop => self.submit(window, cx),
            Chrome::Phone(_) => cx.propagate(),
        }
    }

    fn refresh_mentions(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        let value = input.value().to_string();
        let cursor = input.cursor().min(value.len());
        let menu = self
            .state
            .as_ref()
            .and_then(|state| mention_at(&value, cursor).map(|(at, query)| (state, at, query)));
        let Some((state, at, query)) = menu else {
            self.mentions = None;
            return;
        };
        let candidates = state.read(cx).mention_candidates(&query);
        if candidates.is_empty() {
            self.mentions = None;
            return;
        }
        let highlighted = match &self.mentions {
            Some(previous) if previous.at == at => previous.highlighted.min(candidates.len() - 1),
            Some(_) => 0,
            None => 0,
        };
        self.mentions = Some(MentionMenu {
            at,
            highlighted,
            candidates,
        });
    }

    fn pick_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.mentions else {
            return;
        };
        let Some(mention) = menu.candidates.get(menu.highlighted).cloned() else {
            return;
        };
        self.pick(mention, window, cx);
    }

    fn pick(&mut self, mention: Mention, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.mentions.take() else {
            return;
        };
        let at = menu.at;
        self.input.update(cx, |input, cx| {
            let value = input.value().to_string();
            let cursor = input.cursor().min(value.len());
            let Some(before) = value.get(..at) else {
                return;
            };
            let after = value.get(cursor..).unwrap_or_default();
            let inserted = format!("@{} ", mention.ident);
            let end = at + inserted.len();
            input.set_value(format!("{before}{inserted}{after}"), window, cx);
            input.set_selected_range(end..end, cx);
            input.focus_handle(cx).focus(window, cx);
        });
        self.picked.push(mention);
        self.mentions = None;
        cx.notify();
    }

    fn move_highlight(&mut self, step: Step, cx: &mut Context<Self>) -> bool {
        let Some(menu) = &mut self.mentions else {
            return false;
        };
        let count = menu.candidates.len();
        menu.highlighted = match step {
            Step::Up => (menu.highlighted + count - 1) % count,
            Step::Down => (menu.highlighted + 1) % count,
        };
        cx.notify();
        true
    }

    fn up(&mut self, _action: &MoveUp, _window: &mut Window, cx: &mut Context<Self>) {
        if self.move_highlight(Step::Up, cx) {
            cx.stop_propagation();
        }
    }

    fn down(&mut self, _action: &MoveDown, _window: &mut Window, cx: &mut Context<Self>) {
        if self.move_highlight(Step::Down, cx) {
            cx.stop_propagation();
        }
    }

    fn tab(&mut self, _action: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.mentions.is_none() {
            return;
        }
        cx.stop_propagation();
        self.pick_highlighted(window, cx);
    }

    fn escape(&mut self, _action: &Escape, _window: &mut Window, cx: &mut Context<Self>) {
        if self.mentions.take().is_some() {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(state) = self.state.clone() else {
            return;
        };
        let replying = state.read(cx).replying().is_some();
        if replying {
            cx.stop_propagation();
            state.update(cx, |state, cx| state.cancel_reply(cx));
        }
    }

    fn mention_menu(&self, cx: &mut Context<Self>) -> Option<Div> {
        let menu = self.mentions.as_ref()?;
        let mut list = div()
            .id("composer-mentions")
            .debug_selector(|| "composer-mentions".to_string())
            .flex()
            .flex_col()
            .mx(px(8.))
            .mt(px(8.))
            .p(px(4.))
            .rounded(px(10.))
            .bg(theme::card())
            .border_1()
            .border_color(theme::border());
        for (index, mention) in menu.candidates.iter().enumerate() {
            let chosen = mention.clone();
            let row = row_button(format!("composer-mention-{}", mention.ident))
                .gap(px(8.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(7.))
                .text_size(px(13.))
                .hover(|style| style.bg(theme::sunken()))
                .on_click(cx.listener(move |composer, _event, window, cx| {
                    composer.pick(chosen.clone(), window, cx)
                }))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_primary())
                        .child(SharedString::from(format!("@{}", mention.ident))),
                )
                .child(
                    div()
                        .text_color(theme::text_muted())
                        .child(SharedString::from(mention.name.clone())),
                );
            let row = if index == menu.highlighted {
                row.bg(theme::selection())
            } else {
                row
            };
            list = list.child(row);
        }
        Some(div().child(list))
    }

    fn reply_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (name, text) = {
            let state = self.state.as_ref()?.read(cx);
            let quoted = state.replying()?;
            let writer = message::writer(quoted.author, &state.people());
            let text = clip(&plain::plain_text(&message::source(&quoted.body)));
            (writer.name, text)
        };
        Some(
            div().child(
                div()
                    .id("composer-reply")
                    .debug_selector(|| "composer-reply".to_string())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mx(px(15.))
                    .mt(px(10.))
                    .pl(px(10.))
                    .border_l_2()
                    .border_color(theme::accent())
                    .text_size(px(12.5))
                    .child(icon(Glyph::Back, px(12.), theme::accent()))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(SharedString::from(format!("Replying to {name}"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(theme::text_muted())
                            .child(SharedString::from(text)),
                    )
                    .child(
                        button("composer-cancel-reply")
                            .accessibility_label("Cancel the reply")
                            .p(px(4.))
                            .rounded(px(6.))
                            .hover(|style| style.bg(theme::sunken()))
                            .on_click(cx.listener(|composer, _event, _window, cx| {
                                let Some(state) = composer.state.clone() else {
                                    return;
                                };
                                state.update(cx, |state, cx| state.cancel_reply(cx));
                            }))
                            .child(icon(Glyph::Close, px(12.), theme::text_muted())),
                    ),
            ),
        )
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().to_string();
        if body.trim().is_empty() {
            return;
        }
        let draft = Draft {
            addressed: addressee(&body, &self.picked),
            body,
        };
        let Ok(()) = (self.on_submit)(draft, cx) else {
            return;
        };
        self.picked.clear();
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn send_button(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        let size = px(32.);
        let send = button("composer-send-feed")
            .accessibility_label("Send")
            .flex_none()
            .w(size)
            .h(size)
            .ml(px(7.))
            .rounded_full()
            .on_click(cx.listener(|composer, _event, window, cx| composer.submit(window, cx)));
        match sendable {
            Sendable::Blank => send
                .disabled(true)
                .cursor_default()
                .bg(theme::sunken())
                .child(icon(Glyph::Send, px(16.), theme::text_muted())),
            Sendable::Ready => send
                .bg(theme::accent())
                .shadow(vec![
                    BoxShadow::new(px(0.), px(3.), theme::shadow())
                        .blur_radius(px(8.))
                        .spread_radius(px(-3.)),
                ])
                .child(icon(Glyph::Send, px(16.), theme::chip_text())),
        }
    }

    fn feed_shape(&self, sendable: Sendable, cx: &mut Context<Self>) -> impl IntoElement {
        let reply = self.reply_bar(cx);
        let mentions = self.mention_menu(cx);
        div()
            .on_action(cx.listener(Self::toggle_talk))
            .capture_action(cx.listener(Self::up))
            .capture_action(cx.listener(Self::down))
            .capture_action(cx.listener(Self::tab))
            .capture_action(cx.listener(Self::escape))
            .flex()
            .flex_none()
            .flex_col()
            .px(px(14.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(mentions)
                    .children(reply)
                    .rounded(px(14.))
                    .bg(theme::raised())
                    .border_1()
                    .border_color(theme::border())
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(2.), theme::shadow())
                            .blur_radius(px(10.))
                            .spread_radius(px(-4.)),
                    ])
                    .child(
                        div()
                            .flex()
                            .px(px(15.))
                            .pt(px(14.))
                            .pb(px(8.))
                            .min_w(px(0.))
                            .text_size(px(14.5))
                            .line_height(px(21.))
                            .child(
                                div()
                                    .id("input-feed")
                                    .debug_selector(|| "input-feed".to_string())
                                    .flex_1()
                                    .min_w(px(0.))
                                    .on_action(cx.listener(Self::enter))
                                    .child(Textarea::new(&self.input)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .px(px(9.))
                            .pt(px(6.))
                            .pb(px(9.))
                            .child(
                                button("composer-mention")
                                    .accessibility_label("Mention an agent")
                                    .flex_none()
                                    .w(px(30.))
                                    .h(px(30.))
                                    .rounded(px(8.))
                                    .hover(|style| style.bg(theme::sunken()))
                                    .on_click(cx.listener(|composer, _event, window, cx| {
                                        composer.start_mention(window, cx)
                                    }))
                                    .child(icon(Glyph::Mention, px(16.), theme::text_secondary())),
                            )
                            .child(tool(Glyph::Attach))
                            .child(tool(Glyph::Emoji))
                            .child(tool(Glyph::Format))
                            .child(div().flex_1())
                            .child(self.voice_controls(cx))
                            .child(self.send_button(sendable, cx)),
                    ),
            )
    }
}

impl Composer {
    fn phone_shape(
        &self,
        sendable: Sendable,
        on_field: crate::chrome::OnTap,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let reply = self.reply_bar(cx);
        let mentions = self.mention_menu(cx);
        let recording = match &self.state {
            Some(state) => state.read(cx).recording().clone(),
            None => Recording::Idle,
        };
        let row = div().flex().items_end().gap(px(9.)).px(px(12.)).pt(px(8.));
        let row = match recording {
            Recording::Idle => row
                .child(
                    round_button("composer-mention", px(34.))
                        .accessibility_label("Mention an agent")
                        .bg(theme::sunken())
                        .on_click(cx.listener(|composer, _event, window, cx| {
                            composer.start_mention(window, cx)
                        }))
                        .child(icon(Glyph::Mention, px(17.), theme::ink_soft())),
                )
                .child(
                    div()
                        .id("input-feed")
                        .debug_selector(|| "input-feed".to_string())
                        .flex_1()
                        .min_w(px(0.))
                        .min_h(px(36.))
                        .px(px(14.))
                        .py(px(7.))
                        .rounded(px(18.))
                        .bg(theme::raised())
                        .border_1()
                        .border_color(theme::border())
                        .text_size(px(15.))
                        .line_height(px(21.))
                        .on_action(cx.listener(Self::enter))
                        .on_mouse_up(gpui::MouseButton::Left, move |_event, window, cx| {
                            on_field(window, cx)
                        })
                        .child(Textarea::new(&self.input)),
                )
                .child(match sendable {
                    Sendable::Ready => round_button("composer-send-feed", px(38.))
                        .accessibility_label("Send")
                        .bg(theme::accent())
                        .on_click(
                            cx.listener(|composer, _event, window, cx| composer.submit(window, cx)),
                        )
                        .child(icon(Glyph::Send, px(18.), theme::chip_text())),
                    Sendable::Blank => round_button("composer-talk", px(38.))
                        .accessibility_label("Record a voice message")
                        .bg(theme::accent())
                        .on_click(cx.listener(|composer, _event, _window, cx| composer.talk(cx)))
                        .child(icon(Glyph::Voice, px(18.), theme::chip_text())),
                }),
            Recording::Live { .. } | Recording::Sending | Recording::Failed(_) => row
                .justify_end()
                .min_h(px(38.))
                .child(div().flex_1())
                .child(self.voice_controls(cx)),
        };
        div()
            .on_action(cx.listener(Self::toggle_talk))
            .capture_action(cx.listener(Self::up))
            .capture_action(cx.listener(Self::down))
            .capture_action(cx.listener(Self::tab))
            .capture_action(cx.listener(Self::escape))
            .flex()
            .flex_none()
            .flex_col()
            .pb(px(8.))
            .border_t_1()
            .border_color(theme::hairline())
            .bg(theme::card())
            .children(mentions)
            .children(reply)
            .child(row)
    }
}

fn round_button(selector: &'static str, size: gpui::Pixels) -> gpui_kit::base::Button {
    button(selector).flex_none().w(size).h(size).rounded_full()
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sendable = if self.input.read(cx).value().trim().is_empty() {
            Sendable::Blank
        } else {
            Sendable::Ready
        };
        match self.chrome.clone() {
            Chrome::Desktop => self.feed_shape(sendable, cx).into_any_element(),
            Chrome::Phone(touch) => self
                .phone_shape(sendable, touch.on_field, cx)
                .into_any_element(),
        }
    }
}

fn tool(glyph: Glyph) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(30.))
        .h(px(30.))
        .rounded(px(8.))
        .child(icon(glyph, px(16.), theme::text_secondary()))
}

enum Step {
    Up,
    Down,
}

fn mention_at(value: &str, cursor: usize) -> Option<(usize, String)> {
    let before = value.get(..cursor)?;
    let at = before.rfind('@')?;
    let opens = before[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    if !opens {
        return None;
    }
    let query = &before[at + 1..];
    for letter in query.chars() {
        if !(letter.is_alphanumeric() || letter == '_' || letter == '-') {
            return None;
        }
    }
    Some((at, query.to_string()))
}

fn addressee(body: &str, picked: &[Mention]) -> Option<tuclaw_core::model::AgentId> {
    let mut first: Option<(usize, tuclaw_core::model::AgentId)> = None;
    for mention in picked {
        let Some(at) = body.find(&format!("@{}", mention.ident)) else {
            continue;
        };
        if first.is_none_or(|(known, _agent)| at < known) {
            first = Some((at, mention.agent));
        }
    }
    first.map(|(_at, agent)| agent)
}

fn clip(text: &str) -> String {
    if text.chars().count() <= QUOTE_LIMIT {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(QUOTE_LIMIT).collect();
    clipped.push('…');
    clipped
}

fn hint() -> impl IntoElement {
    div()
        .flex_none()
        .mr(px(6.))
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .child("⌥Space to talk")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use anyhow::bail;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{AgentId, Span};

    use super::{Composer, OnSubmit};
    use crate::state::{AppState, Draft};
    use crate::testing::{channel_named, loaded};

    fn mount(
        cx: &mut TestAppContext,
        on_submit: OnSubmit,
    ) -> (Entity<Composer>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(move |window, cx| {
            Composer::new("Message #General", on_submit, window, cx)
        })
    }

    fn sending(
        cx: &mut TestAppContext,
    ) -> (Entity<AppState>, Entity<Composer>, &mut VisualTestContext) {
        let (_mock, state) = loaded(cx);
        let sender = state.clone();
        let (composer, cx) = mount(
            cx,
            Box::new(move |draft, cx| sender.update(cx, |state, cx| state.send_draft(draft, cx))),
        );
        (state, composer, cx)
    }

    fn focus(composer: &Entity<Composer>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = composer.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        });
        cx.run_until_parked();
    }

    fn typed(composer: &Entity<Composer>, cx: &mut VisualTestContext) -> String {
        composer.read_with(cx, |composer, cx| composer.text(cx))
    }

    fn drafting(
        cx: &mut TestAppContext,
    ) -> (
        Rc<RefCell<Vec<Draft>>>,
        Entity<Composer>,
        &mut VisualTestContext,
    ) {
        let (_mock, state) = loaded(cx);
        let general = channel_named(&state, cx, "General");
        state.update(cx, |state, cx| state.select(general, cx));
        cx.run_until_parked();
        let drafts = Rc::new(RefCell::new(Vec::new()));
        let sent = drafts.clone();
        let (composer, cx) = cx.add_window_view(move |window, cx| {
            Composer::new(
                "Message #General",
                Box::new(move |draft, _cx| {
                    sent.borrow_mut().push(draft);
                    Ok(())
                }),
                window,
                cx,
            )
            .with_state(state.clone(), cx)
        });
        (drafts, composer, cx)
    }

    #[gpui::test]
    fn an_at_sign_offers_the_wired_agents_and_enter_picks_one(cx: &mut TestAppContext) {
        let (drafts, composer, cx) = drafting(cx);
        focus(&composer, cx);
        cx.simulate_input("ask @");
        assert!(cx.debug_bounds("composer-mentions").is_some());
        assert!(cx.debug_bounds("composer-mention-tuclaw").is_some());
        assert!(cx.debug_bounds("composer-mention-magnet_feed").is_some());
        cx.simulate_input("ma");
        assert!(cx.debug_bounds("composer-mention-tuclaw").is_none());
        cx.simulate_keystrokes("enter");
        assert_eq!(typed(&composer, cx), "ask @magnet_feed ");
        assert!(drafts.borrow().is_empty(), "the pick does not send");
        assert!(cx.debug_bounds("composer-mentions").is_none());
        cx.simulate_input("any news?");
        cx.simulate_keystrokes("enter");
        let sent = drafts.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].body, "ask @magnet_feed any news?");
        assert_eq!(sent[0].addressed, Some(AgentId(3)));
    }

    #[gpui::test]
    fn the_mention_button_types_an_at_sign_and_opens_the_list(cx: &mut TestAppContext) {
        let (drafts, composer, cx) = drafting(cx);
        focus(&composer, cx);
        cx.simulate_input("ask");
        let button = cx
            .debug_bounds("composer-mention")
            .expect("the mention button is drawn");
        cx.simulate_click(button.center(), Modifiers::default());
        assert_eq!(typed(&composer, cx), "ask @");
        assert!(cx.debug_bounds("composer-mentions").is_some());
        cx.simulate_keystrokes("enter");
        assert_eq!(typed(&composer, cx), "ask @tuclaw ");
        assert!(drafts.borrow().is_empty());
    }

    #[gpui::test]
    fn arrows_move_the_choice_and_escape_closes_the_list(cx: &mut TestAppContext) {
        let (drafts, composer, cx) = drafting(cx);
        focus(&composer, cx);
        cx.simulate_input("@");
        cx.simulate_keystrokes("down tab");
        assert_eq!(typed(&composer, cx), "@magnet_feed ");
        cx.simulate_input("and @");
        cx.simulate_keystrokes("escape");
        assert!(cx.debug_bounds("composer-mentions").is_none());
        assert!(drafts.borrow().is_empty());
    }

    #[gpui::test]
    fn a_hand_typed_name_addresses_nobody(cx: &mut TestAppContext) {
        let (drafts, composer, cx) = drafting(cx);
        focus(&composer, cx);
        cx.simulate_input("@magnet_feed hi");
        cx.simulate_keystrokes("escape enter");
        let sent = drafts.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].addressed, None);
    }

    #[gpui::test]
    fn enter_sends_the_body_and_clears_the_input(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before + 1);
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("hi".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }

    #[gpui::test]
    fn enter_on_a_blank_input_sends_nothing(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before);
        });
    }

    #[gpui::test]
    fn shift_enter_sends_a_body_carrying_the_break(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        cx.simulate_input("a");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("b");
        cx.simulate_keystrokes("enter");
        state.read_with(cx, |state, _cx| {
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("a\nb".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }

    #[gpui::test]
    fn a_rejected_submission_keeps_the_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| bail!("the write was rejected")));
        focus(&composer, cx);
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        assert_eq!(typed(&composer, cx), "hi");
    }

    #[gpui::test]
    fn the_send_button_takes_the_same_path(cx: &mut TestAppContext) {
        let (state, composer, cx) = sending(cx);
        focus(&composer, cx);
        let before = state.read_with(cx, |state, _cx| state.messages().len());
        cx.simulate_input("hi");
        let button = cx
            .debug_bounds("composer-send-feed")
            .expect("the send button is drawn");
        cx.simulate_click(button.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.messages().len(), before + 1);
            let last = state.messages().last().expect("the message was appended");
            assert_eq!(last.body, vec![Span::Text("hi".to_string())]);
        });
        assert_eq!(typed(&composer, cx), "");
    }

    #[gpui::test]
    fn restore_puts_a_failed_body_back(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        composer.update_in(cx, |composer, window, cx| {
            composer.restore("Лисички 🍄".to_string(), window, cx)
        });
        assert_eq!(typed(&composer, cx), "Лисички 🍄");
        focus(&composer, cx);
        cx.simulate_input("!");
        assert_eq!(typed(&composer, cx), "Лисички 🍄!");
    }

    #[gpui::test]
    fn pasting_inserts_the_clipboard_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        focus(&composer, cx);
        cx.simulate_input("ask ");
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            "лисички со сливками\nи луком".to_string(),
        ));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(typed(&composer, cx), "ask лисички со сливками\nи луком");
    }

    #[gpui::test]
    fn select_all_then_typing_replaces_the_text(cx: &mut TestAppContext) {
        let (composer, cx) = mount(cx, Box::new(|_body, _cx| Ok(())));
        focus(&composer, cx);
        cx.simulate_input("draft one");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("final");
        assert_eq!(typed(&composer, cx), "final");
        cx.simulate_keystrokes("shift-left shift-left backspace");
        assert_eq!(typed(&composer, cx), "fin");
    }
}
