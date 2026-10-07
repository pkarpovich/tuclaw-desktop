use std::rc::Rc;

use gpui::{App, Div, FontWeight, Hsla, SharedString, Window, div, prelude::*, px};
use time::OffsetDateTime;
use tuclaw_core::v3::{FireMark, Outcome, Schedule, ScheduleKind, Task, TaskId};

use crate::control::{button, row_button};
use crate::icon::{Glyph, icon};
use crate::local::clock;
use crate::runlog::MONO;
use crate::theme;

const LABEL_LIMIT: usize = 60;

pub type OnTask = Rc<dyn Fn(&TaskId, &mut Window, &mut App)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRow {
    pub task: TaskId,
    pub label: String,
    pub outcome: Outcome,
    pub at: OffsetDateTime,
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
            at,
            error: error.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quiet {
    pub first: OffsetDateTime,
    pub last: OffsetDateTime,
    pub checks: usize,
    pub ran: usize,
}

impl Quiet {
    pub fn new(at: OffsetDateTime, outcome: Outcome) -> Quiet {
        let mut quiet = Quiet {
            first: at,
            last: at,
            checks: 0,
            ran: 0,
        };
        quiet.add(at, outcome);
        quiet
    }

    pub fn add(&mut self, at: OffsetDateTime, outcome: Outcome) {
        self.first = self.first.min(at);
        self.last = self.last.max(at);
        self.checks += 1;
        let ran = match outcome {
            Outcome::Ran => true,
            Outcome::Silent => true,
            Outcome::Skipped => false,
            Outcome::Failed => false,
            Outcome::Unknown => false,
        };
        if ran {
            self.ran += 1;
        }
    }
}

pub fn quiet_span(quiet: &Quiet) -> String {
    let first = clock(quiet.first);
    let last = clock(quiet.last);
    if first == last {
        return first;
    }
    format!("{first} – {last}")
}

pub fn quiet_text(quiet: &Quiet) -> String {
    let checks = if quiet.checks == 1 {
        "1 check".to_string()
    } else {
        format!("{} checks", quiet.checks)
    };
    if quiet.ran == 0 {
        return format!("quiet · {checks}, nothing to do");
    }
    format!("quiet · {checks} · {} ran", quiet.ran)
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
        ScheduleKind::Cron => cron_text(value).unwrap_or_else(|| format!("cron · {value}")),
        ScheduleKind::Interval => format!("every {value}"),
        ScheduleKind::PollUntil => format!("every {value} until done"),
        ScheduleKind::Event => format!("on {value}"),
        ScheduleKind::Unknown => value.clone(),
    }
}

fn cron_text(value: &str) -> Option<String> {
    let fields: Vec<&str> = value.split_whitespace().collect();
    let [minute, hour, "*", "*", weekdays] = fields.as_slice() else {
        return None;
    };
    let minute: u8 = minute.parse().ok().filter(|minute| *minute < 60)?;
    let hour: u8 = hour.parse().ok().filter(|hour| *hour < 24)?;
    let period = if hour < 12 { "AM" } else { "PM" };
    let shown = match hour % 12 {
        0 => 12,
        other => other,
    };
    let time = format!("{shown}:{minute:02} {period}");
    let days = match *weekdays {
        "*" => "Daily".to_string(),
        "1-5" => "Weekdays".to_string(),
        "0,6" | "6,0" => "Weekends".to_string(),
        list => {
            let mut names = Vec::new();
            for day in list.split(',') {
                names.push(weekday_name(day)?);
            }
            names.join(", ")
        }
    };
    Some(format!("{days} at {time}"))
}

fn weekday_name(day: &str) -> Option<&'static str> {
    let name = match day {
        "0" | "7" => "Sun",
        "1" => "Mon",
        "2" => "Tue",
        "3" => "Wed",
        "4" => "Thu",
        "5" => "Fri",
        "6" => "Sat",
        _ => return None,
    };
    Some(name)
}

pub fn quiet_divider(quiet: &Quiet) -> Div {
    let selector = format!("quiet-{}", quiet.first.unix_timestamp());
    let rule = || div().flex_1().h(px(1.)).bg(theme::hairline());
    div().px(px(20.)).py(px(8.)).child(
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(11.5))
            .child(rule())
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_label())
                    .child(SharedString::from(quiet_span(quiet))),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(quiet_text(quiet))),
            )
            .child(rule()),
    )
}

pub fn failed_card(row: &FireRow, on_open: OnTask) -> Div {
    let selector = format!("failed-{}-{}", row.task.0, row.at.unix_timestamp());
    let task = row.task.clone();
    let error = row.error.clone().unwrap_or_else(|| "failed".to_string());
    div().px(px(20.)).py(px(4.)).child(
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector({
                let selector = selector.clone();
                move || selector
            })
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(9.))
            .bg(theme::failure_tint())
            .border_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .child(icon(Glyph::Automation, px(13.), theme::accent()))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child("Automation failed"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(theme::text_secondary())
                            .child(SharedString::from(row.label.clone())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(theme::text_muted())
                            .child(SharedString::from(clock(row.at))),
                    ),
            )
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(11.5))
                    .text_color(theme::accent())
                    .child(SharedString::from(error)),
            )
            .child(
                div().flex().child(
                    button(format!("{selector}-open"))
                        .px(px(10.))
                        .py(px(3.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme::border())
                        .bg(theme::card())
                        .text_size(px(12.))
                        .text_color(theme::text_primary())
                        .on_click(move |_event, window, cx| on_open(&task, window, cx))
                        .child("Open automation"),
                ),
            ),
    )
}

pub fn trigger_tag(row: &FireRow, on_open: OnTask) -> gpui_kit::base::Button {
    let selector = format!("trigger-{}-{}", row.task.0, row.at.unix_timestamp());
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
    use tuclaw_core::v3::{Outcome, TaskId};

    use super::{Quiet, cron_text, label_of, quiet_span, quiet_text};
    use crate::local::clock;

    #[test]
    fn a_quiet_stretch_counts_its_checks_and_what_ran() {
        let start = datetime!(2026-10-05 01:00 UTC);
        let mut quiet = Quiet::new(start, Outcome::Skipped);
        assert_eq!(quiet_text(&quiet), "quiet · 1 check, nothing to do");
        assert_eq!(quiet_span(&quiet), clock(start));
        let end = start + time::Duration::hours(2);
        quiet.add(end, Outcome::Skipped);
        quiet.add(start + time::Duration::minutes(30), Outcome::Silent);
        assert_eq!(quiet.checks, 3);
        assert_eq!(quiet_text(&quiet), "quiet · 3 checks · 1 ran");
        assert_eq!(
            quiet_span(&quiet),
            format!("{} – {}", clock(start), clock(end))
        );
    }

    #[test]
    fn a_plain_cron_reads_as_days_and_a_time() {
        assert_eq!(cron_text("13 8 * * *").as_deref(), Some("Daily at 8:13 AM"));
        assert_eq!(
            cron_text("17 14 * * 3,5").as_deref(),
            Some("Wed, Fri at 2:17 PM")
        );
        assert_eq!(cron_text("7 3 * * 1").as_deref(), Some("Mon at 3:07 AM"));
        assert_eq!(
            cron_text("0 0 * * 1-5").as_deref(),
            Some("Weekdays at 12:00 AM")
        );
        assert_eq!(cron_text("*/5 * * * *"), None);
        assert_eq!(cron_text("0 9 1 * *"), None);
        assert_eq!(cron_text("0 9 * * 8"), None);
    }

    #[test]
    fn an_unknown_task_still_gets_a_label() {
        assert_eq!(label_of(&TaskId("gone".into()), &[]), "An automation");
    }
}
