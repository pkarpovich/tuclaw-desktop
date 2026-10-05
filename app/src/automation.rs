use std::rc::Rc;

use gpui::{App, Div, FontWeight, Hsla, SharedString, Window, div, prelude::*, px};
use time::OffsetDateTime;
use tuclaw_core::v3::{FireMark, Outcome, Schedule, ScheduleKind, Task, TaskId};

use crate::control::row_button;
use crate::icon::{Glyph, icon};
use crate::local::clock;
use crate::theme;

const LABEL_LIMIT: usize = 60;

pub type OnTask = Rc<dyn Fn(&TaskId, &mut Window, &mut App)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRow {
    pub task: TaskId,
    pub label: String,
    pub outcome: Outcome,
    pub first: OffsetDateTime,
    pub last: OffsetDateTime,
    pub count: usize,
    pub error: Option<String>,
}

impl FireRow {
    pub fn new(mark: &FireMark, at: OffsetDateTime, tasks: &[Task]) -> FireRow {
        let FireMark {
            task_id,
            at: _,
            outcome,
            run_id: _,
            message_id: _,
            error,
        } = mark;
        FireRow {
            task: task_id.clone(),
            label: label_of(task_id, tasks),
            outcome: *outcome,
            first: at,
            last: at,
            count: 1,
            error: error.clone(),
        }
    }

    pub fn absorbs(&self, next: &FireRow) -> bool {
        skipped(self.outcome) && skipped(next.outcome) && self.task == next.task
    }

    pub fn absorb(&mut self, next: FireRow) {
        self.last = next.last;
        self.count += next.count;
    }
}

fn skipped(outcome: Outcome) -> bool {
    match outcome {
        Outcome::Skipped => true,
        Outcome::Ran => false,
        Outcome::Silent => false,
        Outcome::Failed => false,
        Outcome::Unknown => false,
    }
}

pub fn label_of(task: &TaskId, tasks: &[Task]) -> String {
    let mut prompt = None;
    for candidate in tasks {
        if candidate.id == *task {
            prompt = Some(candidate.prompt.clone());
        }
    }
    let Some(prompt) = prompt else {
        return "An automation".to_string();
    };
    let line = prompt.lines().next().unwrap_or_default().trim().to_string();
    if line.chars().count() <= LABEL_LIMIT {
        return line;
    }
    let mut clipped: String = line.chars().take(LABEL_LIMIT).collect();
    clipped.push('…');
    clipped
}

pub fn outcome_text(row: &FireRow) -> String {
    match row.outcome {
        Outcome::Ran => "ran".to_string(),
        Outcome::Silent => "ran, nothing to say".to_string(),
        Outcome::Skipped if row.count > 1 => {
            format!("skipped {}× since {}", row.count, clock(row.first))
        }
        Outcome::Skipped => "skipped, nothing to do".to_string(),
        Outcome::Failed => match &row.error {
            Some(error) => format!("failed: {error}"),
            None => "failed".to_string(),
        },
        Outcome::Unknown => "fired".to_string(),
    }
}

pub fn outcome_tone(outcome: Outcome) -> Hsla {
    match outcome {
        Outcome::Ran => theme::status_idle(),
        Outcome::Silent => theme::text_muted(),
        Outcome::Skipped => theme::text_muted(),
        Outcome::Failed => theme::accent(),
        Outcome::Unknown => theme::text_muted(),
    }
}

pub fn schedule_text(schedule: &Schedule) -> String {
    let Schedule { kind, value } = schedule;
    match kind {
        ScheduleKind::Once => format!("once · {value}"),
        ScheduleKind::Cron => format!("cron · {value}"),
        ScheduleKind::Interval => format!("every {value}"),
        ScheduleKind::PollUntil => format!("every {value} until done"),
        ScheduleKind::Event => format!("on {value}"),
        ScheduleKind::Unknown => value.clone(),
    }
}

pub fn fire_row(row: &FireRow, on_open: OnTask) -> Div {
    let selector = format!("fire-{}-{}", row.task.0, row.last.unix_timestamp());
    let task = row.task.clone();
    div().px(px(20.)).py(px(3.)).child(
        row_button(selector)
            .gap(px(8.))
            .px(px(10.))
            .py(px(4.))
            .rounded(px(7.))
            .hover(|style| style.bg(theme::sunken()))
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .on_click(move |_event, window, cx| on_open(&task, window, cx))
            .child(icon(Glyph::Automation, px(12.), outcome_tone(row.outcome)))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_secondary())
                    .child(SharedString::from(row.label.clone())),
            )
            .child(div().child("·"))
            .child(
                div()
                    .text_color(outcome_tone(row.outcome))
                    .child(SharedString::from(outcome_text(row))),
            )
            .child(div().child(SharedString::from(clock(row.last)))),
    )
}

pub fn trigger_tag(row: &FireRow, on_open: OnTask) -> gpui_kit::base::Button {
    let selector = format!("trigger-{}-{}", row.task.0, row.last.unix_timestamp());
    let task = row.task.clone();
    row_button(selector)
        .accessibility_label(format!("Open the automation {}", row.label))
        .gap(px(4.))
        .px(px(6.))
        .py(px(1.))
        .rounded(px(6.))
        .bg(theme::sunken())
        .hover(|style| style.text_color(theme::text_secondary()))
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .on_click(move |_event, window, cx| on_open(&task, window, cx))
        .child(icon(Glyph::Automation, px(11.), outcome_tone(row.outcome)))
        .child(SharedString::from(row.label.clone()))
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;
    use tuclaw_core::v3::{FireMark, Outcome, TaskId};

    use super::{FireRow, label_of, outcome_text};
    use crate::local::clock;

    fn row(task: &str, outcome: Outcome, minute: u8) -> FireRow {
        let at = datetime!(2026-10-04 09:00 UTC) + time::Duration::minutes(i64::from(minute));
        FireRow::new(
            &FireMark {
                task_id: TaskId(task.into()),
                at: Some(at),
                outcome,
                run_id: None,
                message_id: None,
                error: None,
            },
            at,
            &[],
        )
    }

    #[test]
    fn consecutive_skips_of_one_task_collapse() {
        let mut first = row("a", Outcome::Skipped, 0);
        let second = row("a", Outcome::Skipped, 15);
        assert!(first.absorbs(&second));
        first.absorb(second);
        assert_eq!(first.count, 2);
        assert_eq!(
            outcome_text(&first),
            format!("skipped 2× since {}", clock(first.first))
        );
        assert!(!first.absorbs(&row("b", Outcome::Skipped, 30)));
        assert!(!first.absorbs(&row("a", Outcome::Ran, 30)));
        assert!(!row("a", Outcome::Ran, 0).absorbs(&row("a", Outcome::Ran, 1)));
    }

    #[test]
    fn an_unknown_task_still_gets_a_label() {
        assert_eq!(label_of(&TaskId("gone".into()), &[]), "An automation");
    }
}
