use gpui::{
    Context, Div, Entity, FontWeight, Hsla, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use tuclaw_core::model::{Agent, AgentStatus};

use crate::control::{AvatarSize, Face, avatar};
use crate::people::People;
use crate::state::AppState;
use crate::theme;

pub struct AgentsView {
    state: Entity<AppState>,
    _observation: Subscription,
}

pub struct AgentCard {
    pub name: SharedString,
    pub face: Face,
    pub role: SharedString,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Busy(SharedString),
}

impl AgentsView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> AgentsView {
        let observation = cx.observe(&state, |_view, _state, cx| cx.notify());
        AgentsView {
            state,
            _observation: observation,
        }
    }
}

impl Render for AgentsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards = agent_cards(&self.state.read(cx).people());
        let total = cards.len();
        let mut busy = 0;
        for AgentCard {
            name: _,
            face: _,
            role: _,
            status,
        } in &cards
        {
            match status {
                Status::Busy(_) => busy += 1,
                Status::Idle => {}
            }
        }
        let mut list = div()
            .id("agents-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(8.))
            .overflow_y_scroll()
            .px(px(14.))
            .py(px(12.));
        for card in cards {
            list = list.child(card_element(card));
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(header(total, busy))
            .child(list)
    }
}

pub fn agent_cards(people: &People) -> Vec<AgentCard> {
    let mut ordered: Vec<&Agent> = Vec::new();
    for agent in people.agents {
        ordered.push(agent);
    }
    ordered.sort_by_key(|agent| (agent.sort_index, agent.id.0));
    let mut cards = Vec::new();
    for Agent {
        id: _,
        name,
        initials,
        role,
        status,
        sort_index,
        picture,
    } in ordered
    {
        let status = match status {
            AgentStatus::Idle => Status::Idle,
            AgentStatus::Busy(task) => Status::Busy(SharedString::from(task.clone())),
        };
        cards.push(AgentCard {
            name: SharedString::from(name.clone()),
            face: Face {
                initials: SharedString::from(initials.clone()),
                color: theme::agent_chip(*sort_index as usize),
                picture: people.picture(picture.as_ref()),
            },
            role: SharedString::from(role.clone()),
            status,
        });
    }
    cards
}

fn header(total: usize, busy: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(px(52.))
        .px(px(14.))
        .border_b_1()
        .border_color(theme::hairline())
        .child(
            div()
                .text_size(px(16.))
                .font_weight(FontWeight::BOLD)
                .child("Agents"),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::text_label())
                .child(format!("{total} connected · {busy} busy")),
        )
}

fn card_element(card: AgentCard) -> impl IntoElement {
    let AgentCard {
        name,
        face,
        role,
        status,
    } = card;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .p(px(12.))
        .rounded(px(12.))
        .bg(theme::raised())
        .border_1()
        .border_color(theme::border())
        .child(avatar(face, AvatarSize::Message))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme::text_label())
                        .child(role),
                ),
        )
        .child(div().flex_1())
        .child(status_element(status))
}

fn status_element(status: Status) -> Div {
    match status {
        Status::Idle => status_frame(theme::text_muted(), theme::text_label(), FontWeight::NORMAL)
            .child(SharedString::new_static("Idle")),
        Status::Busy(task) => {
            status_frame(theme::accent(), theme::accent(), FontWeight::SEMIBOLD).child(task)
        }
    }
}

fn status_frame(dot: Hsla, text: Hsla, weight: FontWeight) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .pl(px(8.))
        .text_size(px(12.5))
        .text_color(text)
        .font_weight(weight)
        .child(div().flex_none().w(px(7.)).h(px(7.)).rounded_full().bg(dot))
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, SharedString, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{Agent, AgentId, AgentStatus};

    use super::{AgentCard, AgentsView, Status, agent_cards};
    use crate::people::{Gallery, Me, People};
    use crate::shell::Shell;
    use crate::state::{Segment, View};
    use crate::testing::loaded;

    fn cards(agents: &[Agent]) -> Vec<AgentCard> {
        let me = Me::default();
        let gallery = Gallery::new();
        agent_cards(&People {
            agents,
            me: &me,
            gallery: &gallery,
        })
    }

    #[gpui::test]
    fn the_cards_follow_the_sort_index(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let cards = state.read_with(cx, |state, _cx| agent_cards(&state.people()));
        let mut names = Vec::new();
        let mut statuses = Vec::new();
        for AgentCard {
            name,
            face: _,
            role: _,
            status,
        } in &cards
        {
            names.push(name.clone());
            statuses.push(status.clone());
        }
        assert_eq!(
            names,
            vec![
                SharedString::new_static("Jarvis"),
                SharedString::new_static("Home"),
                SharedString::new_static("Magnet Feed"),
                SharedString::new_static("Scout"),
            ]
        );
        assert_eq!(
            statuses,
            vec![
                Status::Idle,
                Status::Idle,
                Status::Busy(SharedString::new_static("in #Magnet Feed")),
                Status::Idle,
            ]
        );
    }

    #[test]
    fn shuffled_agents_are_ordered_by_the_sort_index() {
        let mut agents = Vec::new();
        for (id, sort_index) in [(7, 3), (2, 1), (9, 0), (4, 2)] {
            agents.push(Agent {
                id: AgentId(id),
                name: format!("agent {id}"),
                initials: "AG".to_string(),
                role: "role".to_string(),
                status: AgentStatus::Idle,
                sort_index,
                picture: None,
            });
        }
        let cards = cards(&agents);
        let mut names = Vec::new();
        for card in &cards {
            names.push(card.name.clone());
        }
        assert_eq!(
            names,
            vec![
                SharedString::new_static("agent 9"),
                SharedString::new_static("agent 2"),
                SharedString::new_static("agent 4"),
                SharedString::new_static("agent 7"),
            ]
        );
    }

    #[test]
    fn agents_sharing_a_sort_index_are_ordered_by_identifier() {
        let mut agents = Vec::new();
        for id in [5, 1] {
            agents.push(Agent {
                id: AgentId(id),
                name: format!("agent {id}"),
                initials: "AG".to_string(),
                role: "role".to_string(),
                status: AgentStatus::Idle,
                sort_index: 0,
                picture: None,
            });
        }
        let cards = cards(&agents);
        let mut names = Vec::new();
        for card in &cards {
            names.push(card.name.clone());
        }
        assert_eq!(
            names,
            vec![
                SharedString::new_static("agent 1"),
                SharedString::new_static("agent 5"),
            ]
        );
    }

    #[gpui::test]
    fn drawing_the_agents_view_does_not_panic(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_view, cx) = cx.add_window_view(move |_window, cx| AgentsView::new(built, cx));
        state.update(cx, |state, cx| {
            state.activate_segment(Segment::Agents, cx);
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| assert_eq!(state.view(), View::Agents));
    }

    #[gpui::test]
    fn a_channel_row_leaves_the_agents_view(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_shell, cx): (Entity<Shell>, &mut VisualTestContext) =
            cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        let agents = cx
            .debug_bounds("sidebar-agents")
            .expect("the agents row is drawn");
        cx.simulate_click(agents.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Agents)
        });
        let channel = cx
            .debug_bounds("sidebar-row-Smart Home")
            .expect("the Smart Home row is drawn");
        cx.simulate_click(channel.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Channel)
        });
    }
}
