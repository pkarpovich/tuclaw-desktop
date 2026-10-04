use std::rc::Rc;

use gpui::{App, Div, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px};
use serde_json::Value;
use tuclaw_core::model::{Agent, Author};
use tuclaw_core::v3::{Run, RunId, RunState};

use crate::link;
use crate::message::{agent_badge, avatar, writer};
use crate::rich::{self, Ink};
use crate::runlog::{self, OnDisclose, Owner, Row, RowView};
use crate::theme;

const DETAIL: usize = 90;
const CURSOR: &str = " ▌";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub id: Option<RunId>,
    pub author: Author,
    pub state: RunState,
    pub steps: Vec<Row>,
    pub segment: String,
}

pub fn run_view(run: &Run) -> RunView {
    RunView {
        id: run.id.clone(),
        author: Author::Agent(link::agent_id(run.agent_id)),
        state: run.state,
        steps: runlog::live_rows(run),
        segment: run.segment.clone(),
    }
}

pub fn owner(view: &RunView) -> Owner {
    match &view.id {
        Some(RunId(id)) => Owner::Run(id.clone()),
        None => Owner::Run("queued".to_string()),
    }
}

pub struct LiveLook {
    pub rows: Vec<RowView>,
    pub on_stop: OnStop,
    pub on_disclose: OnDisclose,
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

pub type OnStop = Rc<dyn Fn(&RunId, &mut Window, &mut App)>;

pub fn run_card(view: &RunView, agents: &[Agent], look: LiveLook) -> impl IntoElement {
    let LiveLook {
        rows,
        on_stop,
        on_disclose,
    } = look;
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
                .child(state_chip(view.state))
                .child(div().flex_1())
                .children(stop_button(view, on_stop)),
        );
    let key = match &view.id {
        Some(RunId(id)) => id.clone(),
        None => "queued".to_string(),
    };
    if !rows.is_empty() {
        column = column.child(runlog::live_steps(owner(view), rows, on_disclose));
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
        column = column.child(div().text_size(px(14.5)).child(rich::markdown(
            SharedString::from(format!("run-{key}-segment")),
            format!("{}{cursor}", view.segment),
            Ink::Body,
        )));
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

fn stop_button(view: &RunView, on_stop: OnStop) -> Option<impl IntoElement> {
    let run = view.id.clone()?;
    match view.state {
        RunState::Running => {}
        RunState::Queued => return None,
        RunState::Stopping => return None,
        RunState::Ok => return None,
        RunState::Error => return None,
        RunState::Interrupted => return None,
    }
    let RunId(raw) = &run;
    let selector = format!("run-stop-{raw}");
    Some(
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .flex_none()
            .px(px(8.))
            .py(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme::border())
            .text_size(px(11.5))
            .text_color(theme::text_secondary())
            .cursor_pointer()
            .hover(|style| style.bg(theme::sunken()))
            .on_click(move |_event, window, cx| on_stop(&run, window, cx))
            .child("Stop"),
    )
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
