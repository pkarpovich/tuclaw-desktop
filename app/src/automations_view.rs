use gpui::{
    Context, Div, Entity, FontWeight, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use time::OffsetDateTime;
use tuclaw_core::v3::{Outcome, Pause, Task, TaskId, TaskRun, TaskStatus};

use crate::automation::{clock, outcome_tone, schedule_text};
use crate::control::{AvatarSize, Face, avatar, button, row_button};
use crate::icon::{Glyph, icon};
use crate::link;
use crate::people::People;
use crate::state::AppState;
use crate::theme;

pub struct AutomationsView {
    state: Entity<AppState>,
    confirming: Option<TaskId>,
    _observation: Subscription,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Active,
    Paused,
    Finished,
}

impl AutomationsView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> AutomationsView {
        let observation = cx.observe(&state, |_view, _state, cx| cx.notify());
        AutomationsView {
            state,
            confirming: None,
            _observation: observation,
        }
    }

    fn row(&self, task: &Task, open: bool, cx: &mut Context<Self>) -> Div {
        let state = self.state.read(cx);
        let people = state.people();
        let topic = task
            .surface_id
            .and_then(|surface| state.surface_name(surface))
            .unwrap_or_else(|| "no topic".to_string());
        let runs = state.task_runs(&task.id).map(<[TaskRun]>::to_vec);
        let id = task.id.clone();
        let selector = format!("automation-{}", task.id.0);
        let opener = self.state.clone();
        let header = row_button(selector)
            .w_full()
            .gap(px(12.))
            .px(px(14.))
            .py(px(12.))
            .on_click(move |_event, _window, cx| {
                let id = id.clone();
                opener.update(cx, |state, cx| state.open_task(id, cx));
            })
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(30.))
                    .rounded(px(8.))
                    .bg(theme::sunken())
                    .child(icon(
                        Glyph::Automation,
                        px(15.),
                        task.last_outcome
                            .map(outcome_tone)
                            .unwrap_or_else(theme::text_muted),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_ellipsis()
                            .child(SharedString::from(first_line(&task.prompt))),
                    )
                    .child(meta_line(task, &people, &topic)),
            )
            .child(last_fire(task))
            .child(self.actions(task, cx));
        let mut card = div()
            .flex()
            .flex_col()
            .rounded(px(12.))
            .bg(theme::raised())
            .border_1()
            .border_color(if open {
                theme::accent()
            } else {
                theme::border()
            })
            .child(header);
        if open {
            card = card.child(detail(task, runs));
        }
        card
    }

    fn actions(&self, task: &Task, cx: &mut Context<Self>) -> Div {
        let mut actions = div().flex().flex_none().items_center().gap(px(6.));
        let toggle = match task.status {
            TaskStatus::Active => Some((Pause::Pause, Glyph::Pause, "Pause")),
            TaskStatus::Paused => Some((Pause::Resume, Glyph::Play, "Resume")),
            TaskStatus::Completed => None,
            TaskStatus::Cancelled => None,
            TaskStatus::Unknown => None,
        };
        let Some((pause, glyph, label)) = toggle else {
            return actions;
        };
        let id = task.id.clone();
        let raw = task.id.0.clone();
        actions = actions.child(
            button(format!("automation-toggle-{raw}"))
                .gap(px(5.))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(7.))
                .border_1()
                .border_color(theme::border())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .hover(|style| style.bg(theme::sunken()))
                .on_click(cx.listener(move |view, _event, _window, cx| {
                    let id = id.clone();
                    view.state
                        .update(cx, |state, cx| state.set_task_paused(id, pause, cx));
                }))
                .child(icon(glyph, px(11.), theme::text_secondary()))
                .child(label),
        );
        let confirming = self.confirming.as_ref() == Some(&task.id);
        let id = task.id.clone();
        actions.child(if confirming {
            button(format!("automation-cancel-{raw}"))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(7.))
                .bg(theme::accent())
                .text_color(theme::chip_text())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(cx.listener(move |view, _event, _window, cx| {
                    view.confirming = None;
                    let id = id.clone();
                    view.state.update(cx, |state, cx| state.cancel_task(id, cx));
                }))
                .child("Cancel it")
        } else {
            button(format!("automation-cancel-{raw}"))
                .accessibility_label("Cancel the automation")
                .p(px(5.))
                .rounded(px(7.))
                .hover(|style| style.bg(theme::sunken()))
                .on_click(cx.listener(move |view, _event, _window, cx| {
                    view.confirming = Some(id.clone());
                    cx.notify();
                }))
                .child(icon(Glyph::Close, px(13.), theme::text_secondary()))
        })
    }
}

impl Render for AutomationsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let tasks = state.tasks().to_vec();
        let selected = state.selected_task().cloned();
        let mut active = 0;
        let mut paused = 0;
        for task in &tasks {
            match group_of(task.status) {
                Group::Active => active += 1,
                Group::Paused => paused += 1,
                Group::Finished => {}
            }
        }
        let mut list = div()
            .id("automations-list")
            .debug_selector(|| "automations-list".to_string())
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(8.))
            .overflow_y_scroll()
            .px(px(14.))
            .py(px(12.));
        if tasks.is_empty() {
            list = list.child(empty());
        }
        for (group, title) in [
            (Group::Active, "Active"),
            (Group::Paused, "Paused"),
            (Group::Finished, "Recently finished"),
        ] {
            let mut members = Vec::new();
            for task in &tasks {
                if group_of(task.status) == group {
                    members.push(task.clone());
                }
            }
            if members.is_empty() {
                continue;
            }
            list = list.child(
                div()
                    .pt(px(6.))
                    .px(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_label())
                    .child(title),
            );
            for task in members {
                let open = selected.as_ref() == Some(&task.id);
                list = list.child(self.row(&task, open, cx));
            }
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.))
            .child(header(active, paused))
            .child(list)
    }
}

fn group_of(status: TaskStatus) -> Group {
    match status {
        TaskStatus::Active => Group::Active,
        TaskStatus::Paused => Group::Paused,
        TaskStatus::Completed => Group::Finished,
        TaskStatus::Cancelled => Group::Finished,
        TaskStatus::Unknown => Group::Finished,
    }
}

fn header(active: usize, paused: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(px(52.))
        .px(px(20.))
        .border_b_1()
        .border_color(theme::hairline())
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Automations"),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::text_muted())
                .child(format!("{active} active · {paused} paused")),
        )
}

fn empty() -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .py(px(40.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_secondary())
                .child("No automations yet"),
        )
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::text_muted())
                .child("Ask an agent to remind you, watch something or run on a schedule."),
        )
}

fn meta_line(task: &Task, people: &People, topic: &str) -> Div {
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(icon(Glyph::Schedule, px(11.), theme::text_muted()))
        .child(SharedString::from(schedule_text(&task.schedule)));
    if let Some(agent) = task.agent_id.map(link::agent_id)
        && let Some(known) = people.agent(agent)
    {
        line = line
            .child("·")
            .child(avatar(
                Face {
                    initials: SharedString::from(known.initials.clone()),
                    color: theme::agent_chip(known.sort_index as usize),
                    picture: people.picture(known.picture.as_ref()),
                },
                AvatarSize::Row,
            ))
            .child(SharedString::from(known.name.clone()));
    }
    line = line
        .child("·")
        .child(SharedString::from(format!("#{topic}")));
    if task.recurring {
        line = line.child("· every match");
    }
    if task.condition.is_some() {
        line = line.child("· with a check");
    }
    line
}

fn last_fire(task: &Task) -> Div {
    let column = div()
        .flex()
        .flex_col()
        .flex_none()
        .items_end()
        .gap(px(2.))
        .text_size(px(11.5));
    let next = match task.next_run_at {
        Some(at) => format!("next {}", when(at)),
        None => match task.status {
            TaskStatus::Active => "waits for its trigger".to_string(),
            TaskStatus::Paused => "paused".to_string(),
            TaskStatus::Completed => "done".to_string(),
            TaskStatus::Cancelled => "cancelled".to_string(),
            TaskStatus::Unknown => String::new(),
        },
    };
    let column = column.child(
        div()
            .text_color(theme::text_secondary())
            .child(SharedString::from(next)),
    );
    match (task.last_outcome, task.last_run_at) {
        (Some(outcome), Some(at)) => column.child(div().text_color(outcome_tone(outcome)).child(
            SharedString::from(format!("{} {}", outcome_word(outcome), when(at))),
        )),
        (Some(_), None) => column,
        (None, _) => column.child(div().text_color(theme::text_muted()).child("never fired")),
    }
}

fn detail(task: &Task, runs: Option<Vec<TaskRun>>) -> Div {
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(14.))
        .pb(px(14.))
        .pt(px(2.))
        .border_t_1()
        .border_color(theme::hairline())
        .child(
            div()
                .pt(px(10.))
                .text_size(px(13.))
                .text_color(theme::text_primary())
                .child(SharedString::from(task.prompt.clone())),
        );
    if let Some(condition) = &task.condition {
        body = body.child(fact("Check", condition));
    }
    if let Some(from) = task.active_from {
        body = body.child(fact("Starts", &when(from)));
    }
    if let Some(until) = task.active_until {
        body = body.child(fact("Ends", &when(until)));
    }
    let mut history = div().flex().flex_col().gap(px(3.)).child(
        div()
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_label())
            .child("Recent fires"),
    );
    match runs {
        None => {
            history = history.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .child("Loading…"),
            );
        }
        Some(runs) if runs.is_empty() => {
            history = history.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .child("It has not fired yet."),
            );
        }
        Some(runs) => {
            for run in runs {
                history = history.child(run_line(&run));
            }
        }
    }
    body.child(history)
}

fn fact(name: &'static str, value: &str) -> Div {
    div()
        .flex()
        .gap(px(8.))
        .text_size(px(12.))
        .child(
            div()
                .w(px(52.))
                .flex_none()
                .text_color(theme::text_muted())
                .child(name),
        )
        .child(
            div()
                .font_family(crate::runlog::MONO)
                .text_color(theme::text_secondary())
                .child(SharedString::from(value.to_string())),
        )
}

fn run_line(run: &TaskRun) -> Div {
    let TaskRun {
        at,
        outcome,
        duration_ms,
        error,
    } = run;
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(12.))
        .child(div().size(px(6.)).rounded_full().bg(outcome_tone(*outcome)))
        .child(
            div()
                .w(px(130.))
                .flex_none()
                .text_color(theme::text_secondary())
                .child(SharedString::from(when(*at))),
        )
        .child(
            div()
                .text_color(outcome_tone(*outcome))
                .child(outcome_word(*outcome)),
        )
        .child(
            div()
                .text_color(theme::text_muted())
                .child(SharedString::from(format!(
                    "{:.1} s",
                    *duration_ms as f64 / 1000.
                ))),
        );
    if let Some(error) = error {
        line = line.child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_ellipsis()
                .text_color(theme::accent())
                .child(SharedString::from(error.clone())),
        );
    }
    line
}

fn outcome_word(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Ran => "ran",
        Outcome::Silent => "ran quietly",
        Outcome::Skipped => "skipped",
        Outcome::Failed => "failed",
        Outcome::Unknown => "fired",
    }
}

fn when(at: OffsetDateTime) -> String {
    let today = OffsetDateTime::now_utc().date();
    if at.date() == today {
        return clock(at);
    }
    let description = time::macros::format_description!("[month repr:short] [day padding:none]");
    format!(
        "{} {}",
        at.format(&description).unwrap_or_default(),
        clock(at)
    )
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}
