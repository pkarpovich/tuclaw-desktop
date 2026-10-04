use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Div, Entity, FontWeight, Hsla, IntoElement, SharedString, Stateful,
    Window, div, prelude::*, px,
};
use gpui_kit::base::Popover;
use tuclaw_core::model::{AgentId, AgentStatus};

use crate::control::{AvatarSize, Face, avatar, button};
use crate::icon::{Glyph, icon};
use crate::people::People;
use crate::state::AppState;
use crate::theme;

pub type OnAgent = Rc<dyn Fn(AgentId, &mut Window, &mut App)>;
type Dismiss = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct CardActions {
    pub on_mention: OnAgent,
    pub on_settings: OnAgent,
    pub on_view_run: OnAgent,
}

#[derive(Clone)]
struct Profile {
    agent: AgentId,
    face: Face,
    name: SharedString,
    bot: Option<SharedString>,
    description: SharedString,
    busy: Option<SharedString>,
}

pub fn actions(state: &Entity<AppState>) -> CardActions {
    let mentioner = state.clone();
    let opener = state.clone();
    let viewer = state.clone();
    CardActions {
        on_mention: Rc::new(move |agent, _window, cx| {
            mentioner.update(cx, |state, cx| state.mention(agent, cx));
        }),
        on_settings: Rc::new(move |agent, _window, cx| {
            opener.update(cx, |state, cx| state.open_settings(agent, cx));
        }),
        on_view_run: Rc::new(move |agent, _window, cx| {
            viewer.update(cx, |state, cx| state.view_run(agent, cx));
        }),
    }
}

pub fn with_card(
    selector: String,
    agent: AgentId,
    trigger: impl IntoElement,
    people: &People,
    actions: &CardActions,
) -> AnyElement {
    let Some(profile) = profile(people, agent) else {
        return trigger.into_any_element();
    };
    let actions = actions.clone();
    Popover::new(SharedString::from(selector.clone()))
        .anchor(Anchor::TopLeft)
        .offset(px(6.))
        .trigger(
            button(format!("{selector}-trigger"))
                .accessibility_label(format!("Show {}", profile.name))
                .flex_none()
                .child(trigger),
        )
        .content(move |_popover, _window, cx| {
            let popover = cx.entity();
            body(&profile, &actions, move |window, cx| {
                popover.update(cx, |popover, cx| popover.dismiss(window, cx));
            })
        })
        .into_any_element()
}

fn profile(people: &People, agent: AgentId) -> Option<Profile> {
    let known = people.agent(agent)?;
    let wire = people.wire(agent);
    let busy = match &known.status {
        AgentStatus::Busy(task) => Some(SharedString::from(format!("Busy {task}"))),
        AgentStatus::Idle => None,
    };
    Some(Profile {
        agent,
        face: Face {
            initials: SharedString::from(known.initials.clone()),
            color: theme::agent_chip(known.sort_index as usize),
            picture: people.picture(known.picture.as_ref()),
        },
        name: SharedString::from(known.name.clone()),
        bot: wire
            .and_then(|wire| wire.bot_username.clone())
            .map(|bot| SharedString::from(format!("@{bot}"))),
        description: SharedString::from(
            wire.map(|wire| wire.description.clone())
                .unwrap_or_default(),
        ),
        busy,
    })
}

fn body(
    profile: &Profile,
    actions: &CardActions,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let Profile {
        agent,
        face,
        name,
        bot,
        description,
        busy,
    } = profile.clone();
    let dismiss: Dismiss = Rc::new(dismiss);
    let mut identity = div().flex().flex_col().gap(px(2.)).min_w(px(0.)).child(
        div()
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(name),
    );
    if let Some(bot) = bot {
        identity = identity.child(
            div()
                .text_size(px(11.5))
                .font_family(crate::runlog::MONO)
                .text_color(theme::text_muted())
                .child(bot),
        );
    }
    if !description.is_empty() {
        identity = identity.child(
            div()
                .pt(px(4.))
                .text_size(px(12.5))
                .text_color(theme::text_secondary())
                .child(description),
        );
    }
    let mut card = div()
        .id("agent-card")
        .debug_selector(|| "agent-card".to_string())
        .w(px(300.))
        .flex()
        .flex_col()
        .rounded(px(12.))
        .bg(theme::raised())
        .border_1()
        .border_color(theme::border())
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(10.), theme::shadow())
                .blur_radius(px(28.))
                .spread_radius(px(-8.)),
        ])
        .child(
            div()
                .flex()
                .gap(px(12.))
                .p(px(14.))
                .child(avatar(face, AvatarSize::Profile))
                .child(identity),
        );
    let running = busy.is_some();
    if let Some(busy) = busy {
        card = card.child(
            div()
                .mx(px(14.))
                .mb(px(12.))
                .px(px(10.))
                .py(px(7.))
                .rounded(px(8.))
                .bg(theme::failure_tint())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::status_busy())
                .child(busy),
        );
    }
    let mention = actions.on_mention.clone();
    let settings = actions.on_settings.clone();
    let view_run = actions.on_view_run.clone();
    let after_mention = dismiss.clone();
    let after_settings = dismiss.clone();
    let after_view_run = dismiss;
    card.child(
        div()
            .flex()
            .gap(px(6.))
            .p(px(10.))
            .border_t_1()
            .border_color(theme::hairline())
            .child(
                action("card-message", Glyph::Message, "Message", Tone::Off)
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("SOON"),
                    )
                    .disabled(true)
                    .cursor_default(),
            )
            .child(
                action("card-mention", Glyph::Mention, "Mention", Tone::Plain).on_click(
                    move |_event, window, cx| {
                        after_mention(window, cx);
                        mention(agent, window, cx);
                    },
                ),
            )
            .child(
                action("card-settings", Glyph::Adjust, "Settings", Tone::Plain).on_click(
                    move |_event, window, cx| {
                        after_settings(window, cx);
                        settings(agent, window, cx);
                    },
                ),
            )
            .child(if running {
                action("card-view-run", Glyph::Play, "View run", Tone::Primary).on_click(
                    move |_event, window, cx| {
                        after_view_run(window, cx);
                        view_run(agent, window, cx);
                    },
                )
            } else {
                action("card-view-run", Glyph::Play, "View run", Tone::Off)
                    .disabled(true)
                    .cursor_default()
            }),
    )
}

enum Tone {
    Plain,
    Primary,
    Off,
}

fn action(
    selector: &'static str,
    glyph: Glyph,
    label: &'static str,
    tone: Tone,
) -> gpui_kit::base::Button {
    let (background, ink): (Hsla, Hsla) = match tone {
        Tone::Plain => (theme::sunken(), theme::text_primary()),
        Tone::Primary => (theme::accent(), theme::chip_text()),
        Tone::Off => (theme::sunken(), theme::text_muted()),
    };
    button(selector)
        .flex_1()
        .flex_col()
        .gap(px(3.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(background)
        .text_color(ink)
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icon(glyph, px(14.), ink))
        .child(label)
}
