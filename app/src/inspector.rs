use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Div, FontWeight, SharedString, Stateful, Window, div, phi, prelude::*, px, relative,
};
use gpui_kit::base::ToggleGroup;
use tuclaw_core::model::{Agent, Message, RunOutcome, RunRef};
use tuclaw_core::v3::RunDetail;

use crate::control::{self, button};
use crate::icon::{Glyph, icon};
use crate::message::{avatar, writer};
use crate::rich;
use crate::runlog::{
    self, Disclosure, Footer, OnDisclose, Owner, Row, RunLog, StepStatus, ToolCall, duration_label,
};
use crate::state::{Filter, Inspector};
use crate::theme;

pub const WIDTH: f32 = 400.;

pub type OnClose = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OnFilter = Rc<dyn Fn(Filter, &mut Window, &mut App)>;

pub struct InspectorInput<'a> {
    pub inspector: Inspector,
    pub message: &'a Message,
    pub log: Option<&'a RunLog>,
    pub agents: &'a [Agent],
    pub is_open: &'a dyn Fn(Disclosure, bool) -> bool,
}

pub struct InspectorActions {
    pub on_disclose: OnDisclose,
    pub on_close: OnClose,
    pub on_filter: OnFilter,
}

pub fn filtered(rows: Vec<Row>, filter: Filter) -> Vec<Row> {
    let mut kept = Vec::new();
    for row in rows {
        match (filter, row) {
            (Filter::All, row) => kept.push(row),
            (Filter::Tools, row @ Row::Tool(_)) => kept.push(row),
            (Filter::Tools, row @ Row::Group { .. }) => kept.push(row),
            (Filter::Thoughts, row @ Row::Thought { .. }) => kept.push(row),
            (Filter::Errors, Row::Tool(call)) if call.status == StepStatus::Error => {
                kept.push(Row::Tool(call))
            }
            (
                Filter::Errors,
                Row::Group {
                    seq: _,
                    name: _,
                    description: _,
                    status: _,
                    duration: _,
                    calls,
                },
            ) => {
                for call in calls {
                    if call.status == StepStatus::Error {
                        kept.push(Row::Tool(call));
                    }
                }
            }
            (Filter::Tools, _other) => {}
            (Filter::Thoughts, _other) => {}
            (Filter::Errors, _other) => {}
        }
    }
    kept
}

pub fn offsets(detail: &RunDetail) -> HashMap<i64, Duration> {
    let mut offsets = HashMap::new();
    let start = detail.run.started_at;
    for step in &detail.steps {
        if let Ok(offset) = Duration::try_from(step.started_at - start) {
            offsets.insert(step.seq, offset);
        }
    }
    offsets
}

pub fn offset_label(offset: Duration) -> String {
    let seconds = offset.as_secs();
    format!("+{}:{:02}", seconds / 60, seconds % 60)
}

fn errors(rows: &[Row]) -> usize {
    let mut count = 0;
    for row in rows {
        match row {
            Row::Tool(ToolCall { status, .. }) if *status == StepStatus::Error => count += 1,
            Row::Group { calls, .. } => {
                for call in calls {
                    if call.status == StepStatus::Error {
                        count += 1;
                    }
                }
            }
            Row::Tool(_) => {}
            Row::Thought { .. } => {}
            Row::Task { .. } => {}
            Row::Status { .. } => {}
        }
    }
    count
}

pub fn render(input: InspectorInput, actions: InspectorActions) -> Stateful<Div> {
    let InspectorInput {
        inspector,
        message,
        log,
        agents,
        is_open,
    } = input;
    let InspectorActions {
        on_disclose,
        on_close,
        on_filter,
    } = actions;
    let Inspector {
        message: id,
        filter,
    } = inspector;
    let writer = writer(message.author, agents);
    let close = on_close.clone();
    let header = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(16.))
        .py(px(12.))
        .border_b_1()
        .border_color(theme::hairline())
        .child(avatar(&writer))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Run"),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme::text_muted())
                        .child(writer.name.clone()),
                ),
        )
        .child(
            button("inspector-close")
                .accessibility_label("Close the run")
                .p(px(4.))
                .rounded(px(6.))
                .hover(|style| style.bg(theme::sunken()))
                .on_click(move |_event, window, cx| close(window, cx))
                .child(icon(Glyph::Close, px(14.), theme::text_secondary())),
        );
    let mut panel = div()
        .id("inspector")
        .debug_selector(|| "inspector".to_string())
        .flex()
        .flex_col()
        .size_full()
        .child(header);
    if let Some(run) = &message.run {
        panel = panel.child(status_block(run, log));
    }
    panel = panel.child(filters(filter, on_filter));
    let detail = match log {
        Some(RunLog::Loaded(detail)) => detail,
        Some(RunLog::Loading) => return panel.child(note("Loading the run…")),
        Some(RunLog::Failed) => return panel.child(note("Couldn't load the run.")),
        None => return panel.child(note("Loading the run…")),
    };
    let answer = rich::split_thinking(&crate::message::source(&message.body)).answer;
    let rows = filtered(runlog::rows(detail, &answer), filter);
    if rows.is_empty() {
        return panel.child(note("Nothing here."));
    }
    let offsets = offsets(detail);
    let owner = Owner::Message(id);
    let views = runlog::views(owner.clone(), rows, is_open);
    let mut list = div()
        .id("inspector-steps")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .px(px(12.))
        .py(px(8.))
        .gap(px(2.));
    for (index, view) in views.into_iter().enumerate() {
        let offset = offsets.get(&view.seq()).copied();
        list = list.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .w(px(42.))
                        .pt(px(4.))
                        .font_family("Menlo")
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(SharedString::from(
                            offset.map(offset_label).unwrap_or_default(),
                        )),
                )
                .child(div().flex_1().min_w(px(0.)).child(runlog::row_element(
                    &owner,
                    index,
                    view,
                    on_disclose.clone(),
                ))),
        );
    }
    panel.child(list)
}

fn status_block(run: &RunRef, log: Option<&RunLog>) -> Div {
    let RunRef {
        id: _,
        outcome,
        steps,
        tools,
        duration,
    } = run;
    let (label, dot) = match outcome {
        RunOutcome::Ok => ("Completed", theme::status_idle()),
        RunOutcome::Error => ("Failed", theme::accent()),
        RunOutcome::Interrupted => ("Stopped", theme::text_muted()),
        RunOutcome::Running => ("Working", theme::status_busy()),
        RunOutcome::Unknown => ("Finished", theme::text_muted()),
    };
    let mut block = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(16.))
        .py(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(dot))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme::text_muted())
                        .child(SharedString::from(format!(
                            "{steps} steps · {tools} tools · {}",
                            duration_label(*duration)
                        ))),
                ),
        );
    let Some(RunLog::Loaded(detail)) = log else {
        return block;
    };
    let Footer { context, tokens } = runlog::footer(detail);
    if let Some((used, max)) = context {
        let share = (used as f32 / max as f32).clamp(0.0, 1.0);
        block = block.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child("Context")
                .child(
                    div()
                        .flex_1()
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(theme::wave_rest())
                        .child(
                            div()
                                .h_full()
                                .w(relative(share))
                                .rounded(px(2.))
                                .bg(theme::ink_soft()),
                        ),
                )
                .child(SharedString::from(format!(
                    "{} / {}",
                    runlog::compact(used),
                    runlog::compact(max)
                ))),
        );
    }
    let failures = errors(&runlog::rows(detail, ""));
    let mut facts = Vec::new();
    if let Some((input, output)) = tokens {
        facts.push(format!(
            "{} in · {} out",
            runlog::compact(input),
            runlog::compact(output)
        ));
    }
    if failures > 0 {
        facts.push(format!(
            "{failures} failed {}",
            if failures == 1 { "call" } else { "calls" }
        ));
    }
    if facts.is_empty() {
        return block;
    }
    block.child(
        div()
            .text_size(px(11.5))
            .text_color(theme::text_muted())
            .child(SharedString::from(facts.join(" · "))),
    )
}

fn filters(active: Filter, on_filter: OnFilter) -> ToggleGroup {
    let mut group = control::segments("inspector-filters")
        .gap(px(2.))
        .mx(px(16.))
        .mb(px(6.))
        .p(px(2.))
        .rounded(px(8.))
        .bg(theme::sunken());
    for (filter, label, selector) in [
        (Filter::All, "All", "inspector-filter-all"),
        (Filter::Tools, "Tools", "inspector-filter-tools"),
        (Filter::Thoughts, "Thoughts", "inspector-filter-thoughts"),
        (Filter::Errors, "Errors", "inspector-filter-errors"),
    ] {
        let pick = on_filter.clone();
        group = group.child(
            control::segment(selector, filter == active)
                .flex_1()
                .py(px(4.))
                .rounded(px(6.))
                .text_size(px(12.))
                .line_height(phi())
                .on_change(move |_pressed, _event, window, cx| pick(filter, window, cx))
                .child(label),
        );
    }
    group
}

fn note(text: &'static str) -> Div {
    div()
        .px(px(16.))
        .py(px(12.))
        .text_size(px(12.5))
        .text_color(theme::text_muted())
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(seq: i64, name: &str, status: StepStatus) -> ToolCall {
        ToolCall {
            seq,
            name: name.into(),
            full_name: name.into(),
            arg: String::new(),
            status,
            duration: None,
            input: String::new(),
            result: String::new(),
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            Row::Thought {
                seq: 1,
                text: "Checking.".into(),
            },
            Row::Tool(call(2, "Read", StepStatus::Ok)),
            Row::Group {
                seq: 3,
                name: "Bash".into(),
                description: "polling".into(),
                status: StepStatus::Error,
                duration: Duration::from_secs(4),
                calls: vec![
                    call(3, "Bash", StepStatus::Ok),
                    call(4, "Bash", StepStatus::Error),
                ],
            },
            Row::Status {
                seq: 5,
                text: "compacting".into(),
            },
        ]
    }

    #[test]
    fn each_filter_keeps_its_kind_of_step() {
        assert_eq!(filtered(rows(), Filter::All).len(), 4);
        let tools = filtered(rows(), Filter::Tools);
        assert_eq!(tools.len(), 2);
        let thoughts = filtered(rows(), Filter::Thoughts);
        assert_eq!(
            thoughts,
            vec![Row::Thought {
                seq: 1,
                text: "Checking.".into()
            }]
        );
        assert_eq!(
            filtered(rows(), Filter::Errors),
            vec![Row::Tool(call(4, "Bash", StepStatus::Error))]
        );
        assert_eq!(errors(&rows()), 1);
    }

    #[test]
    fn offsets_read_as_minutes_and_seconds() {
        assert_eq!(offset_label(Duration::from_secs(3)), "+0:03");
        assert_eq!(offset_label(Duration::from_secs(177)), "+2:57");
        let detail: RunDetail =
            serde_json::from_str(include_str!("../../core/testdata/v3/run.json")).expect("run");
        let offsets = offsets(&detail);
        assert_eq!(offsets.get(&1), Some(&Duration::from_secs(2)));
        assert_eq!(offsets.get(&2), Some(&Duration::from_secs(3)));
    }
}
