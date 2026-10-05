use std::rc::Rc;

use gpui::{App, Div, FontWeight, Hsla, SharedString, Stateful, Window, div, prelude::*, px};
use time::{Duration, OffsetDateTime};
use tuclaw_core::v3::{FireMark, Outcome, Task, TaskId, TaskStatus};

use crate::automation::{OnTask, label_of, outcome_tone, schedule_text};
use crate::control::{button, row_button, switch};
use crate::icon::{Glyph, icon};
use crate::local::clock;
use crate::theme;

pub const WIDTH: f32 = 400.;
pub const WINDOW: Duration = Duration::hours(12);
const STRIP: f32 = WIDTH - 2. * 12. - 2. * 12.;
const TICK: f32 = 4.;

pub type OnClose = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OnToggle = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct PanelInput<'a> {
    pub channel: SharedString,
    pub tasks: Vec<&'a Task>,
    pub fires: &'a [FireMark],
    pub show_skipped: bool,
    pub now: OffsetDateTime,
}

pub struct PanelActions {
    pub on_close: OnClose,
    pub on_task: OnTask,
    pub on_toggle_skipped: OnToggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub checks: usize,
    pub ran: usize,
    pub failed: usize,
    pub last_failure: Option<OffsetDateTime>,
}

pub fn tally(task: &TaskId, fires: &[FireMark], since: OffsetDateTime) -> Tally {
    let mut tally = Tally::default();
    for mark in fires {
        let Some(at) = mark.at.filter(|at| *at >= since) else {
            continue;
        };
        if mark.task_id != *task {
            continue;
        }
        tally.checks += 1;
        match mark.outcome {
            Outcome::Ran => tally.ran += 1,
            Outcome::Silent => tally.ran += 1,
            Outcome::Failed => {
                tally.failed += 1;
                tally.last_failure = Some(tally.last_failure.map_or(at, |last| last.max(at)));
            }
            Outcome::Skipped => {}
            Outcome::Unknown => {}
        }
    }
    tally
}

pub fn tally_text(tally: &Tally, next: Option<OffsetDateTime>) -> String {
    let checks = if tally.checks == 1 {
        "1 check".to_string()
    } else {
        format!("{} checks", tally.checks)
    };
    let mut parts = vec![checks];
    if tally.checks > 0 && tally.ran == 0 && tally.failed == 0 {
        parts.push("all skipped".to_string());
    }
    if tally.ran > 0 {
        parts.push(format!("{} ran", tally.ran));
    }
    if let Some(at) = tally.last_failure {
        parts.push(format!("{} failed, last at {}", tally.failed, clock(at)));
    }
    if let Some(next) = next {
        parts.push(format!("next {}", clock(next)));
    }
    parts.join(" · ")
}

pub fn activity(fires: &[FireMark], since: OffsetDateTime, show_skipped: bool) -> Vec<FireMark> {
    let mut rows = Vec::new();
    for mark in fires {
        if mark.at.is_none_or(|at| at < since) {
            continue;
        }
        let skipped = match mark.outcome {
            Outcome::Skipped => true,
            Outcome::Ran => false,
            Outcome::Silent => false,
            Outcome::Failed => false,
            Outcome::Unknown => false,
        };
        if skipped && !show_skipped {
            continue;
        }
        rows.push(mark.clone());
    }
    rows.sort_by_key(|mark| std::cmp::Reverse(mark.at));
    rows
}

pub fn skipped_count(fires: &[FireMark], since: OffsetDateTime) -> usize {
    let mut skipped = 0;
    for mark in fires {
        if mark.at.is_some_and(|at| at >= since) && mark.outcome == Outcome::Skipped {
            skipped += 1;
        }
    }
    skipped
}

pub fn render(input: PanelInput, actions: PanelActions) -> Stateful<Div> {
    let PanelInput {
        channel,
        tasks,
        fires,
        show_skipped,
        now,
    } = input;
    let PanelActions {
        on_close,
        on_task,
        on_toggle_skipped,
    } = actions;
    let since = now - WINDOW;
    let mut all_tasks = Vec::new();
    for task in &tasks {
        all_tasks.push((*task).clone());
    }
    let mut body = div()
        .id("automations-panel-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(12.))
        .py(px(10.))
        .gap(px(8.));
    if tasks.is_empty() {
        body = body.child(note("No automations report to this channel."));
    }
    for task in &tasks {
        body = body.child(task_card(task, fires, since, now, on_task.clone()));
    }
    let skipped = skipped_count(fires, since);
    let toggle = on_toggle_skipped.clone();
    body = body.child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(10.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Activity"),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme::text_muted())
                    .child(SharedString::from(format!("Show skipped · {skipped}"))),
            )
            .child(
                switch("automations-show-skipped", show_skipped)
                    .accessibility_label("Show skipped checks")
                    .on_change(move |_checked, _event, window, cx| toggle(window, cx)),
            ),
    );
    let rows = activity(fires, since, show_skipped);
    if rows.is_empty() {
        body = body.child(note("Nothing but skipped checks in the last 12 hours."));
    }
    for mark in rows {
        body = body.child(activity_row(&mark, &all_tasks, on_task.clone()));
    }
    let close = on_close.clone();
    div()
        .id("automations-panel")
        .debug_selector(|| "automations-panel".to_string())
        .flex()
        .flex_col()
        .size_full()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(12.))
                .border_b_1()
                .border_color(theme::hairline())
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Automations"),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme::text_muted())
                                .child(SharedString::from(format!("#{channel} · last 12 h"))),
                        ),
                )
                .child(
                    button("automations-close")
                        .accessibility_label("Close automations")
                        .p(px(4.))
                        .rounded(px(6.))
                        .hover(|style| style.bg(theme::sunken()))
                        .on_click(move |_event, window, cx| close(window, cx))
                        .child(icon(Glyph::Close, px(14.), theme::text_secondary())),
                ),
        )
        .child(body)
}

fn task_card(
    task: &Task,
    fires: &[FireMark],
    since: OffsetDateTime,
    now: OffsetDateTime,
    on_task: OnTask,
) -> impl IntoElement {
    let tally = tally(&task.id, fires, since);
    let dot = match task.status {
        TaskStatus::Paused => theme::text_muted(),
        TaskStatus::Active if tally.failed > 0 => theme::accent(),
        TaskStatus::Active => theme::status_idle(),
        TaskStatus::Completed => theme::text_muted(),
        TaskStatus::Cancelled => theme::text_muted(),
        TaskStatus::Unknown => theme::text_muted(),
    };
    let label = label_of(&task.id, std::slice::from_ref(task));
    let schedule = match task.condition {
        Some(_) => format!("{} · condition", schedule_text(&task.schedule)),
        None => schedule_text(&task.schedule),
    };
    let next = match task.status {
        TaskStatus::Active => task.next_run_at,
        TaskStatus::Paused => None,
        TaskStatus::Completed => None,
        TaskStatus::Cancelled => None,
        TaskStatus::Unknown => None,
    };
    let footer_tone = if tally.failed > 0 {
        theme::accent()
    } else {
        theme::text_muted()
    };
    let id = task.id.clone();
    row_button(format!("automations-task-{}", task.id.0))
        .flex_col()
        .items_start()
        .gap(px(4.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(9.))
        .bg(theme::sunken())
        .on_click(move |_event, window, cx| on_task(&id, window, cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w_full()
                .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(label)),
                ),
        )
        .child(
            div()
                .pl(px(15.))
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child(SharedString::from(schedule)),
        )
        .child(strip(&task.id, fires, since, now))
        .child(
            div()
                .text_size(px(11.5))
                .text_color(footer_tone)
                .child(SharedString::from(tally_text(&tally, next))),
        )
}

fn strip(task: &TaskId, fires: &[FireMark], since: OffsetDateTime, now: OffsetDateTime) -> Div {
    let span = (now - since).as_seconds_f32();
    let mut strip = div()
        .relative()
        .w(px(STRIP))
        .h(px(14.))
        .my(px(2.))
        .rounded(px(3.))
        .bg(theme::field());
    for mark in fires {
        if mark.task_id != *task {
            continue;
        }
        let Some(at) = mark.at.filter(|at| *at >= since && *at <= now) else {
            continue;
        };
        let offset = (at - since).as_seconds_f32() / span * (STRIP - TICK);
        strip = strip.child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(offset))
                .w(px(TICK))
                .h(px(10.))
                .rounded(px(1.))
                .bg(tick_tone(mark.outcome)),
        );
    }
    strip
}

fn tick_tone(outcome: Outcome) -> Hsla {
    match outcome {
        Outcome::Skipped => theme::border(),
        Outcome::Silent => theme::text_label(),
        Outcome::Ran => theme::text_secondary(),
        Outcome::Failed => theme::accent(),
        Outcome::Unknown => theme::border(),
    }
}

fn activity_row(mark: &FireMark, tasks: &[Task], on_task: OnTask) -> impl IntoElement {
    let at = mark.at.map(clock).unwrap_or_default();
    let outcome = match mark.outcome {
        Outcome::Ran => "Ran".to_string(),
        Outcome::Silent => "Ran, nothing to say".to_string(),
        Outcome::Skipped => "Skipped, nothing to do".to_string(),
        Outcome::Failed => match &mark.error {
            Some(error) => format!("Failed · {error}"),
            None => "Failed".to_string(),
        },
        Outcome::Unknown => "Fired".to_string(),
    };
    let stamp = mark.at.map(|at| at.unix_timestamp()).unwrap_or_default();
    let id = mark.task_id.clone();
    row_button(format!("automations-activity-{}-{stamp}", mark.task_id.0))
        .items_start()
        .gap(px(10.))
        .px(px(6.))
        .py(px(5.))
        .rounded(px(7.))
        .hover(|style| style.bg(theme::sunken()))
        .on_click(move |_event, window, cx| on_task(&id, window, cx))
        .child(
            div()
                .flex_none()
                .w(px(62.))
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child(SharedString::from(at)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::text_primary())
                        .child(SharedString::from(label_of(&mark.task_id, tasks))),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(outcome_tone(mark.outcome))
                        .child(SharedString::from(outcome)),
                ),
        )
}

fn note(text: &'static str) -> impl IntoElement {
    div()
        .px(px(6.))
        .py(px(6.))
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(text)
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;
    use tuclaw_core::v3::{FireMark, Outcome, TaskId};

    use super::{Tally, activity, skipped_count, tally, tally_text};
    use crate::local::clock;

    fn mark(task: &str, minute: i64, outcome: Outcome) -> FireMark {
        FireMark {
            task_id: TaskId(task.into()),
            at: Some(datetime!(2026-10-05 00:00 UTC) + time::Duration::minutes(minute)),
            outcome,
            run_id: None,
            message_id: None,
            error: None,
        }
    }

    #[test]
    fn a_tally_counts_one_task_inside_the_window() {
        let since = datetime!(2026-10-05 00:30 UTC);
        let fires = vec![
            mark("a", 10, Outcome::Skipped),
            mark("a", 40, Outcome::Skipped),
            mark("a", 50, Outcome::Failed),
            mark("a", 70, Outcome::Silent),
            mark("b", 60, Outcome::Ran),
        ];
        let counted = tally(&TaskId("a".into()), &fires, since);
        let failed_at = datetime!(2026-10-05 00:50 UTC);
        assert_eq!(
            counted,
            Tally {
                checks: 3,
                ran: 1,
                failed: 1,
                last_failure: Some(failed_at),
            }
        );
        assert_eq!(
            tally_text(&counted, None),
            format!("3 checks · 1 ran · 1 failed, last at {}", clock(failed_at))
        );
        let quiet = tally(&TaskId("c".into()), &fires, since);
        assert_eq!(tally_text(&quiet, None), "0 checks");
        let skipped_only = Tally {
            checks: 141,
            ..Tally::default()
        };
        assert_eq!(tally_text(&skipped_only, None), "141 checks · all skipped");
    }

    #[test]
    fn activity_hides_skipped_until_asked_and_is_newest_first() {
        let since = datetime!(2026-10-05 00:00 UTC);
        let fires = vec![
            mark("a", 10, Outcome::Skipped),
            mark("a", 20, Outcome::Failed),
            mark("b", 30, Outcome::Ran),
        ];
        let quiet = activity(&fires, since, false);
        assert_eq!(quiet.len(), 2);
        assert_eq!(quiet[0].outcome, Outcome::Ran);
        assert_eq!(activity(&fires, since, true).len(), 3);
        assert_eq!(skipped_count(&fires, since), 1);
    }
}
