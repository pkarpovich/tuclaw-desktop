use gpui::{Div, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::Value;
use tuclaw_core::model::{Agent, Author};
use tuclaw_core::v3::{Run, RunId, RunState, StepKind, ToolStatus};

use crate::link;
use crate::message::{agent_badge, avatar, writer};
use crate::theme;

const DETAIL: usize = 90;
const CURSOR: &str = " ▌";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub id: Option<RunId>,
    pub author: Author,
    pub state: RunState,
    pub steps: Vec<StepView>,
    pub segment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepView {
    Thought(String),
    Tool {
        name: String,
        detail: String,
        status: ToolStatus,
    },
    Task {
        kind: String,
        state: String,
        description: String,
    },
    Status {
        status: String,
        detail: String,
    },
    Other(String),
}

pub fn run_view(run: &Run) -> RunView {
    let mut steps = Vec::new();
    for step in &run.steps {
        let view = match &step.kind {
            StepKind::Text { text } => StepView::Thought(text.clone()),
            StepKind::Tool {
                tool_use_id: _,
                name,
                input,
                output: _,
                status,
                finished_at: _,
            } => StepView::Tool {
                name: if name.is_empty() {
                    "tool".to_string()
                } else {
                    name.clone()
                },
                detail: tool_detail(input),
                status: *status,
            },
            StepKind::Task {
                task_id: _,
                task_type,
                state,
                description,
                summary: _,
            } => StepView::Task {
                kind: task_type.clone(),
                state: state.clone(),
                description: description.clone().unwrap_or_default(),
            },
            StepKind::Status { status, detail } => StepView::Status {
                status: status.clone(),
                detail: detail.clone(),
            },
            StepKind::Other { name, output: _ } => {
                StepView::Other(name.clone().unwrap_or_else(|| "step".to_string()))
            }
        };
        steps.push(view);
    }
    RunView {
        id: run.id.clone(),
        author: Author::Agent(link::agent_id(run.agent_id)),
        state: run.state,
        steps,
        segment: run.segment.clone(),
    }
}

pub fn tool_detail(input: &Value) -> String {
    let Value::Object(fields) = input else {
        return String::new();
    };
    if let Some(Value::String(_)) = fields.get("truncated")
        && fields.len() == 1
    {
        return "(input too large to show)".to_string();
    }
    for key in [
        "command",
        "description",
        "url",
        "file_path",
        "pattern",
        "query",
        "prompt",
    ] {
        if let Some(Value::String(value)) = fields.get(key) {
            return clamp(first_line(value));
        }
    }
    clamp(&input.to_string())
}

fn first_line(text: &str) -> &str {
    let Some(line) = text.lines().next() else {
        return text;
    };
    line.trim_end_matches(['\\', ' '])
}

fn clamp(text: &str) -> String {
    let mut clamped = String::new();
    for (count, letter) in text.chars().enumerate() {
        if count == DETAIL {
            clamped.push('…');
            break;
        }
        clamped.push(letter);
    }
    clamped
}

pub fn state_label(state: RunState) -> &'static str {
    match state {
        RunState::Queued => "queued",
        RunState::Running => "working",
        RunState::Stopping => "stopping",
        RunState::Ok => "done",
        RunState::Error => "failed",
        RunState::Interrupted => "stopped",
    }
}

pub fn run_card(view: &RunView, agents: &[Agent]) -> impl IntoElement {
    let writer = writer(view.author, agents);
    let selector = match &view.id {
        Some(RunId(id)) => format!("run-{id}"),
        None => "run-queued".to_string(),
    };
    let mut column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(writer.name.clone()),
                )
                .child(agent_badge())
                .child(state_chip(view.state)),
        );
    for step in &view.steps {
        column = column.child(step_element(step));
    }
    if !view.segment.is_empty() || view.state == RunState::Running {
        let cursor = match view.state {
            RunState::Queued => "",
            RunState::Running => CURSOR,
            RunState::Stopping => CURSOR,
            RunState::Ok => "",
            RunState::Error => "",
            RunState::Interrupted => "",
        };
        column = column.child(
            div()
                .text_size(px(14.))
                .line_height(px(21.))
                .child(SharedString::from(format!("{}{cursor}", view.segment))),
        );
    }
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .w_full()
        .flex()
        .gap(px(12.))
        .px(px(20.))
        .py(px(8.))
        .child(avatar(&writer))
        .child(column)
}

fn state_chip(state: RunState) -> Div {
    let dot = match state {
        RunState::Queued => theme::status_idle(),
        RunState::Running => theme::status_busy(),
        RunState::Stopping => theme::status_busy(),
        RunState::Ok => theme::status_idle(),
        RunState::Error => theme::accent(),
        RunState::Interrupted => theme::status_idle(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(px(11.5))
        .text_color(theme::text_muted())
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(dot))
        .child(state_label(state))
}

fn step_element(step: &StepView) -> Div {
    let row = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w(px(0.))
        .text_size(px(12.5))
        .text_color(theme::text_secondary());
    match step {
        StepView::Thought(text) => div()
            .text_size(px(13.))
            .line_height(px(19.))
            .text_color(theme::text_muted())
            .child(SharedString::from(text.clone())),
        StepView::Tool {
            name,
            detail,
            status,
        } => row
            .child(div().text_color(theme::text_label()).child("▸"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(name.clone())),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(detail.clone())),
            )
            .child(div().flex_none().child(match status {
                ToolStatus::Running => "…",
                ToolStatus::Ok => "✓",
                ToolStatus::Error => "✗",
            })),
        StepView::Task {
            kind,
            state,
            description,
        } => row
            .child(div().text_color(theme::text_label()).child("◷"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(kind.clone())),
            )
            .child(
                div()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(format!("{state} · {description}"))),
            ),
        StepView::Status { status, detail } => row
            .text_color(theme::text_muted())
            .child(SharedString::from(format!("· {status} - {detail}"))),
        StepView::Other(name) => row
            .text_color(theme::text_muted())
            .child(SharedString::from(format!("· {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tool_details_prefer_the_meaningful_field_and_its_first_line() {
        assert_eq!(
            tool_detail(
                &json!({"command": "rg -i 'лисич' ~/notes \\\n  --glob '*.md'", "description": "search"})
            ),
            "rg -i 'лисич' ~/notes"
        );
        assert_eq!(
            tool_detail(&json!({"url": "https://example.org"})),
            "https://example.org"
        );
        assert_eq!(
            tool_detail(&json!({"truncated": "{\"content\": \"..."})),
            "(input too large to show)"
        );
        assert_eq!(tool_detail(&json!({"x": 1})), r#"{"x":1}"#);
        assert_eq!(tool_detail(&Value::Null), "");
        let long = "a".repeat(200);
        let detail = tool_detail(&json!({ "command": long }));
        assert_eq!(detail.chars().count(), DETAIL + 1);
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn every_state_has_a_label() {
        let mut labels = Vec::new();
        for state in [
            RunState::Queued,
            RunState::Running,
            RunState::Stopping,
            RunState::Ok,
            RunState::Error,
            RunState::Interrupted,
        ] {
            labels.push(state_label(state));
        }
        assert_eq!(
            labels,
            vec!["queued", "working", "stopping", "done", "failed", "stopped"]
        );
    }
}
