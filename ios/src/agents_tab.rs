use gpui::{
    Context, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use tuclaw_core::model::AgentId;
use tuclaw_desktop::agents::{AgentCard, Status, agent_cards};
use tuclaw_desktop::control::{AvatarSize, avatar};
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::state::AppState;
use tuclaw_desktop::theme;

use crate::frame;
use crate::home::TAB_BAR_HEIGHT;

pub struct AgentsTab {
    state: Entity<AppState>,
    _observation: Subscription,
}

impl AgentsTab {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> AgentsTab {
        let observation = cx.observe(&state, |_tab, _state, cx| cx.notify());
        AgentsTab {
            state,
            _observation: observation,
        }
    }
}

impl Render for AgentsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards = agent_cards(&self.state.read(cx).people());
        let mut busy = 0;
        for card in &cards {
            match card.status {
                Status::Busy(_) => busy += 1,
                Status::Idle => {}
            }
        }
        let mut list = div()
            .id("agents-tab-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(9.))
            .overflow_y_scroll()
            .px(px(12.))
            .pb(px(TAB_BAR_HEIGHT + 24.) + frame::insets().bottom);
        for card in cards {
            list = list.child(self.card(card));
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::window())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pt(frame::insets().top)
                    .px(px(18.))
                    .pb(px(12.))
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::BOLD)
                            .child("Agents"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text_label())
                            .child(SharedString::from(summary(
                                cards_total(&self.state, cx),
                                busy,
                            ))),
                    ),
            )
            .child(list)
    }
}

impl AgentsTab {
    fn card(&self, card: AgentCard) -> impl IntoElement + use<> {
        let AgentCard {
            agent,
            name,
            face,
            role,
            status,
        } = card;
        let AgentId(raw) = agent;
        let opener = self.state.clone();
        let (line, color, border) = match status {
            Status::Busy(task) => (task, theme::accent(), theme::accent()),
            Status::Idle => {
                let line = if role.is_empty() {
                    SharedString::new_static("Idle")
                } else {
                    SharedString::from(format!("Idle · {role}"))
                };
                (line, theme::text_label(), theme::hairline())
            }
        };
        div()
            .id(SharedString::from(format!("agents-tab-{raw}")))
            .debug_selector(move || format!("agents-tab-{raw}"))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(13.))
            .py(px(12.))
            .rounded(px(15.))
            .bg(theme::raised())
            .border_1()
            .border_color(border)
            .on_click(move |_event, _window, cx| {
                opener.update(cx, |state, cx| state.open_settings(agent, cx))
            })
            .child(avatar(face, AvatarSize::Card))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(color)
                            .truncate()
                            .child(line),
                    ),
            )
            .child(icon(Glyph::Closed, px(14.), theme::text_muted()))
    }
}

fn cards_total(state: &Entity<AppState>, cx: &Context<AgentsTab>) -> usize {
    state.read(cx).agents().len()
}

fn summary(total: usize, busy: usize) -> String {
    let connected = match total {
        1 => "1 connected".to_string(),
        count => format!("{count} connected"),
    };
    match busy {
        0 => connected,
        count => format!("{connected} · {count} busy"),
    }
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn the_summary_counts_connected_and_busy_agents() {
        assert_eq!(summary(1, 0), "1 connected");
        assert_eq!(summary(4, 2), "4 connected · 2 busy");
    }
}
