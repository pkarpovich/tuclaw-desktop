use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Div, FontWeight, Hsla, SharedString, Window, div, prelude::*, px, relative};
use gpui_kit::base::Button;
use serde_json::Value;
use time::OffsetDateTime;
use tuclaw_core::model::{MessageId, RunOutcome, RunRef};
use tuclaw_core::v3::{
    ContextWindow, RowKind, Run, RunDetail, Step, StepKind, StepRow, ToolStatus, Usage,
};

use crate::control::{button, row_button};
use crate::icon::{Glyph, icon, spinner};
use crate::live::tool_detail;
use crate::rich::{self, Ink};
use crate::theme;

#[cfg(not(target_os = "ios"))]
pub const MONO: &str = "Menlo";
#[cfg(target_os = "ios")]
pub const MONO: &str = ".AppleSystemUIFontMonospaced";

pub type OnDisclose = Rc<dyn Fn(Disclosure, &mut Window, &mut App)>;

pub const RESULT_LINES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Owner {
    Message(MessageId),
    Run(String),
}

impl Owner {
    fn key(&self) -> String {
        match self {
            Owner::Message(MessageId(id)) => format!("m{id}"),
            Owner::Run(id) => format!("r{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Disclosure {
    Log(MessageId),
    Inspect(MessageId),
    Group(Owner, i64),
    Step(Owner, i64),
    FullResult(Owner, i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunLog {
    Loading,
    Loaded(Box<RunDetail>),
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Running,
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub seq: i64,
    pub name: String,
    pub full_name: String,
    pub arg: String,
    pub status: StepStatus,
    pub duration: Option<Duration>,
    pub input: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Thought {
        seq: i64,
        text: String,
    },
    Tool(ToolCall),
    Group {
        seq: i64,
        name: String,
        description: String,
        status: StepStatus,
        duration: Duration,
        calls: Vec<ToolCall>,
    },
    Task {
        seq: i64,
        task_id: String,
        kind: String,
        description: String,
        state: TaskState,
    },
    Status {
        seq: i64,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    pub context: Option<(u64, u64)>,
    pub tokens: Option<(u64, u64)>,
}

pub fn summary(run: &RunRef) -> Option<Summary> {
    let RunRef {
        id: _,
        outcome,
        steps,
        tools,
        duration,
    } = run;
    let took = duration_label(*duration);
    match outcome {
        RunOutcome::Ok => {
            if *tools == 0 {
                return None;
            }
            Some(Summary {
                text: format!("{} · {took}", count(*tools, "tool", "tools")),
                tone: Tone::Plain,
            })
        }
        RunOutcome::Error => Some(Summary {
            text: format!(
                "Failed · {} · {} · {took}",
                count(*steps, "step", "steps"),
                count(*tools, "tool", "tools")
            ),
            tone: Tone::Failed,
        }),
        RunOutcome::Interrupted => Some(Summary {
            text: format!(
                "Stopped by you · {} · {took}",
                count(*tools, "tool", "tools")
            ),
            tone: Tone::Stopped,
        }),
        RunOutcome::Running => None,
        RunOutcome::Unknown => None,
    }
}

pub fn quick_duration(run: &RunRef) -> Option<String> {
    let RunRef {
        id: _,
        outcome,
        steps: _,
        tools,
        duration,
    } = run;
    match outcome {
        RunOutcome::Ok if *tools == 0 && !duration.is_zero() => Some(duration_label(*duration)),
        RunOutcome::Ok => None,
        RunOutcome::Error => None,
        RunOutcome::Interrupted => None,
        RunOutcome::Running => None,
        RunOutcome::Unknown => None,
    }
}

pub fn opens_by_default(run: &RunRef) -> bool {
    match run.outcome {
        RunOutcome::Error => true,
        RunOutcome::Ok => false,
        RunOutcome::Interrupted => false,
        RunOutcome::Running => false,
        RunOutcome::Unknown => false,
    }
}

pub fn count(value: u32, one: &str, many: &str) -> String {
    if value == 1 {
        format!("1 {one}")
    } else {
        format!("{value} {many}")
    }
}

pub fn duration_label(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{:.1} s", duration.as_secs_f64());
    }
    let seconds = duration.as_secs();
    if seconds < 60 {
        return format!("{seconds} s");
    }
    format!("{} m {} s", seconds / 60, seconds % 60)
}

pub fn short_name(name: &str) -> String {
    match name.rsplit_once("__") {
        Some((_, last)) if !last.is_empty() => last.to_string(),
        Some(_) => name.to_string(),
        None => name.to_string(),
    }
}

pub fn rows(detail: &RunDetail, answer: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for step in &detail.steps {
        let row = match step.kind {
            RowKind::Text => Row::Thought {
                seq: step.seq,
                text: step.output.clone().unwrap_or_default(),
            },
            RowKind::Tool => Row::Tool(tool_call(step)),
            RowKind::Task => task_row(step),
            RowKind::Status => Row::Status {
                seq: step.seq,
                text: status_text(step),
            },
            RowKind::Unknown => continue,
        };
        push_grouped(&mut rows, row);
    }
    if let Some(Row::Thought { seq: _, text }) = rows.last()
        && same_words(text, answer)
    {
        rows.pop();
    }
    rows
}

fn same_words(left: &str, right: &str) -> bool {
    let mut left_words = Vec::new();
    for word in left.split_whitespace() {
        left_words.push(word);
    }
    let mut right_words = Vec::new();
    for word in right.split_whitespace() {
        right_words.push(word);
    }
    !left_words.is_empty() && left_words == right_words
}

fn push_grouped(rows: &mut Vec<Row>, row: Row) {
    let row = match row {
        Row::Task {
            seq,
            task_id,
            kind,
            description,
            state,
        } => {
            merge_task(
                rows,
                Row::Task {
                    seq,
                    task_id,
                    kind,
                    description,
                    state,
                },
            );
            return;
        }
        other => other,
    };
    let Row::Tool(call) = row else {
        rows.push(row);
        return;
    };
    match rows.pop() {
        Some(Row::Tool(previous)) if previous.full_name == call.full_name => {
            rows.push(group(vec![previous, call]));
        }
        Some(Row::Group {
            seq: _,
            name: _,
            description: _,
            status: _,
            duration: _,
            mut calls,
        }) if calls
            .first()
            .is_some_and(|first| first.full_name == call.full_name) =>
        {
            calls.push(call);
            rows.push(group(calls));
        }
        Some(other) => {
            rows.push(other);
            rows.push(Row::Tool(call));
        }
        None => rows.push(Row::Tool(call)),
    }
}

fn merge_task(rows: &mut Vec<Row>, task: Row) {
    let Row::Task {
        seq,
        task_id,
        kind,
        description,
        state,
    } = task
    else {
        return;
    };
    for existing in rows.iter_mut() {
        if let Row::Task {
            seq: _,
            task_id: known,
            kind: known_kind,
            description: known_description,
            state: known_state,
        } = existing
            && !task_id.is_empty()
            && *known == task_id
        {
            *known_state = state;
            if known_description.is_empty() {
                *known_description = description;
            }
            if known_kind.is_empty() {
                *known_kind = kind;
            }
            return;
        }
    }
    rows.push(Row::Task {
        seq,
        task_id,
        kind,
        description,
        state,
    });
}

fn task_state(state: &str) -> TaskState {
    match state {
        "completed" | "done" | "success" => TaskState::Done,
        "failed" | "error" | "killed" | "stopped" | "cancelled" => TaskState::Failed,
        _other => TaskState::Running,
    }
}

pub fn live_rows(run: &Run) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (index, step) in run.steps.iter().enumerate() {
        let seq = index as i64;
        let Step { started_at, kind } = step;
        let row = match kind {
            StepKind::Text { text } => Row::Thought {
                seq,
                text: text.clone(),
            },
            StepKind::Tool {
                tool_use_id: _,
                name,
                input,
                output,
                status,
                finished_at,
            } => {
                let full_name = if name.is_empty() {
                    "tool".to_string()
                } else {
                    name.clone()
                };
                Row::Tool(ToolCall {
                    seq,
                    name: short_name(&full_name),
                    full_name,
                    arg: main_argument(input),
                    status: match status {
                        ToolStatus::Running => StepStatus::Running,
                        ToolStatus::Ok => StepStatus::Ok,
                        ToolStatus::Error => StepStatus::Error,
                    },
                    duration: match started_at {
                        Some(started) => elapsed(*started, *finished_at),
                        None => None,
                    },
                    input: pretty(input),
                    result: output.clone().unwrap_or_default(),
                })
            }
            StepKind::Task {
                task_id,
                task_type,
                state,
                description,
                summary,
            } => Row::Task {
                seq,
                task_id: task_id.clone(),
                kind: task_type.clone(),
                description: description
                    .clone()
                    .filter(|text| !text.is_empty())
                    .or_else(|| summary.clone())
                    .unwrap_or_default(),
                state: task_state(state),
            },
            StepKind::Status { status, detail } => Row::Status {
                seq,
                text: if detail.is_empty() {
                    status.clone()
                } else {
                    format!("{status} · {detail}")
                },
            },
            StepKind::Other { name, output: _ } => Row::Status {
                seq,
                text: name.clone().unwrap_or_else(|| "step".to_string()),
            },
        };
        push_grouped(&mut rows, row);
    }
    rows
}

fn group(calls: Vec<ToolCall>) -> Row {
    let mut status = StepStatus::Ok;
    let mut duration = Duration::ZERO;
    for call in &calls {
        match call.status {
            StepStatus::Error => status = StepStatus::Error,
            StepStatus::Running => {
                if status != StepStatus::Error {
                    status = StepStatus::Running;
                }
            }
            StepStatus::Ok => {}
        }
        duration += call.duration.unwrap_or(Duration::ZERO);
    }
    let first = calls.first().cloned();
    let (seq, name, description) = match first {
        Some(first) => (first.seq, first.name, first.arg),
        None => (0, String::new(), String::new()),
    };
    Row::Group {
        seq,
        name,
        description,
        status,
        duration,
        calls,
    }
}

fn tool_call(step: &StepRow) -> ToolCall {
    let full_name = step.name.clone().unwrap_or_else(|| "tool".to_string());
    let input = step.input.clone().unwrap_or(Value::Null);
    let arg = main_argument(&input);
    ToolCall {
        seq: step.seq,
        name: short_name(&full_name),
        full_name,
        arg,
        status: step_status(step.status.as_deref()),
        duration: elapsed(step.started_at, step.finished_at),
        input: pretty(&input),
        result: step.output.clone().unwrap_or_default(),
    }
}

fn main_argument(input: &Value) -> String {
    if let Value::Object(fields) = input
        && let Some(Value::String(description)) = fields.get("description")
        && fields.contains_key("command")
    {
        return description.clone();
    }
    let detail = match input {
        Value::Object(fields) if !has_known_field(fields) => scalar_pairs(fields),
        Value::Object(_) => tool_detail(input),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    match detail.strip_prefix("https://") {
        Some(rest) => rest.to_string(),
        None => match detail.strip_prefix("http://") {
            Some(rest) => rest.to_string(),
            None => detail,
        },
    }
}

const KNOWN: [&str; 7] = [
    "command",
    "description",
    "url",
    "file_path",
    "pattern",
    "query",
    "prompt",
];

fn has_known_field(fields: &serde_json::Map<String, Value>) -> bool {
    for key in KNOWN {
        if let Some(Value::String(_)) = fields.get(key) {
            return true;
        }
    }
    false
}

fn scalar_pairs(fields: &serde_json::Map<String, Value>) -> String {
    let mut pairs = Vec::new();
    for (key, value) in fields {
        let shown = match value {
            Value::String(text) => first_line(text),
            Value::Number(number) => number.to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Null => continue,
            Value::Array(_) => continue,
            Value::Object(_) => continue,
        };
        pairs.push(format!("{key}: {shown}"));
    }
    pairs.join(" · ")
}

fn pretty(input: &Value) -> String {
    match input {
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

fn step_status(status: Option<&str>) -> StepStatus {
    match status {
        Some("error") => StepStatus::Error,
        Some("running") => StepStatus::Running,
        Some("started") => StepStatus::Running,
        Some(_) => StepStatus::Ok,
        None => StepStatus::Ok,
    }
}

fn elapsed(started: OffsetDateTime, finished: Option<OffsetDateTime>) -> Option<Duration> {
    let finished = finished?;
    let span = finished - started;
    Duration::try_from(span).ok()
}

fn task_row(step: &StepRow) -> Row {
    let raw = step.output.clone().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let field = |key: &str| match parsed.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(_) => String::new(),
        None => String::new(),
    };
    let kind = match field("task_type") {
        kind if kind.is_empty() => step.name.clone().unwrap_or_else(|| "task".to_string()),
        kind => kind,
    };
    let mut description = field("description");
    if description.is_empty() {
        description = field("summary");
    }
    if description.is_empty() && parsed.is_null() {
        description = raw;
    }
    Row::Task {
        seq: step.seq,
        task_id: field("task_id"),
        kind,
        description,
        state: task_state(&field("state")),
    }
}

fn status_text(step: &StepRow) -> String {
    let name = step.name.clone().unwrap_or_default();
    let output = step.output.clone().unwrap_or_default();
    match (name.is_empty(), output.is_empty()) {
        (true, true) => "status".to_string(),
        (true, false) => output,
        (false, true) => name,
        (false, false) => format!("{name} · {output}"),
    }
}

pub fn footer(detail: &RunDetail) -> Footer {
    let context = match &detail.run.context {
        Some(ContextWindow {
            tokens,
            max_tokens,
            model: _,
        }) if *max_tokens > 0 => Some((*tokens, *max_tokens)),
        Some(_) => None,
        None => None,
    };
    let tokens = detail.run.usage.as_ref().map(|usage| {
        let Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            num_turns: _,
            duration_api_ms: _,
        } = usage;
        (
            input_tokens + cache_read_tokens + cache_creation_tokens,
            *output_tokens,
        )
    });
    Footer { context, tokens }
}

pub fn compact(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        return format!("{}M", trim(tokens as f64 / 1_000_000.0));
    }
    if tokens >= 1_000 {
        return format!("{}k", trim(tokens as f64 / 1_000.0));
    }
    tokens.to_string()
}

fn trim(value: f64) -> String {
    if value >= 100.0 || (value - value.round()).abs() < 0.05 {
        format!("{}", value.round() as u64)
    } else {
        format!("{value:.1}")
    }
}

pub fn clip(text: &str, lines: usize) -> (String, bool) {
    let mut kept = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == lines {
            return (kept.join("\n"), true);
        }
        kept.push(line);
    }
    (text.to_string(), false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolView {
    pub call: ToolCall,
    pub open: bool,
    pub full: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowView {
    Thought {
        seq: i64,
        text: String,
        first: bool,
    },
    Tool(ToolView),
    Group {
        seq: i64,
        name: String,
        description: String,
        status: StepStatus,
        duration: Duration,
        open: bool,
        calls: Vec<ToolView>,
    },
    Task {
        seq: i64,
        kind: String,
        description: String,
        state: TaskState,
    },
    Status {
        seq: i64,
        text: String,
    },
}

impl RowView {
    pub fn seq(&self) -> i64 {
        match self {
            RowView::Thought { seq, .. } => *seq,
            RowView::Tool(view) => view.call.seq,
            RowView::Group { seq, .. } => *seq,
            RowView::Task { seq, .. } => *seq,
            RowView::Status { seq, .. } => *seq,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Closed,
    Loading,
    Failed,
    Log { rows: Vec<RowView>, footer: Footer },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub summary: Summary,
    pub body: Body,
}

pub struct PaneInput<'a> {
    pub message: MessageId,
    pub run: &'a RunRef,
    pub answer: &'a str,
    pub log: Option<&'a RunLog>,
}

pub fn pane(input: PaneInput, is_open: &dyn Fn(Disclosure, bool) -> bool) -> Option<Pane> {
    let PaneInput {
        message,
        run,
        answer,
        log,
    } = input;
    let summary = summary(run)?;
    if !is_open(Disclosure::Log(message), opens_by_default(run)) {
        return Some(Pane {
            summary,
            body: Body::Closed,
        });
    }
    let body = match log {
        None => Body::Loading,
        Some(RunLog::Loading) => Body::Loading,
        Some(RunLog::Failed) => Body::Failed,
        Some(RunLog::Loaded(detail)) => Body::Log {
            rows: views(Owner::Message(message), rows(detail, answer), is_open),
            footer: footer(detail),
        },
    };
    Some(Pane { summary, body })
}

pub fn views(
    owner: Owner,
    rows: Vec<Row>,
    is_open: &dyn Fn(Disclosure, bool) -> bool,
) -> Vec<RowView> {
    let tool = |call: ToolCall| {
        let errored = call.status == StepStatus::Error;
        ToolView {
            open: is_open(Disclosure::Step(owner.clone(), call.seq), errored),
            full: is_open(Disclosure::FullResult(owner.clone(), call.seq), false),
            call,
        }
    };
    let mut views = Vec::new();
    let mut thought_seen = false;
    for row in rows {
        let view = match row {
            Row::Thought { seq, text } => {
                let first = !thought_seen;
                thought_seen = true;
                RowView::Thought { seq, text, first }
            }
            Row::Tool(call) => RowView::Tool(tool(call)),
            Row::Group {
                seq,
                name,
                description,
                status,
                duration,
                calls,
            } => {
                let mut members = Vec::new();
                for call in calls {
                    members.push(tool(call));
                }
                RowView::Group {
                    seq,
                    name,
                    description,
                    status,
                    duration,
                    open: is_open(Disclosure::Group(owner.clone(), seq), false),
                    calls: members,
                }
            }
            Row::Task {
                seq,
                task_id: _,
                kind,
                description,
                state,
            } => RowView::Task {
                seq,
                kind,
                description,
                state,
            },
            Row::Status { seq, text } => RowView::Status { seq, text },
        };
        views.push(view);
    }
    views
}

pub fn render(message: MessageId, pane: Pane, on_disclose: OnDisclose) -> Div {
    let MessageId(raw) = message;
    let Pane { summary, body } = pane;
    let open = match body {
        Body::Closed => false,
        Body::Loading => true,
        Body::Failed => true,
        Body::Log { rows: _, footer: _ } => true,
    };
    let toggle = on_disclose.clone();
    let selector = format!("runlog-{raw}");
    let Summary { text, tone } = summary;
    let ink = match tone {
        Tone::Plain => theme::text_muted(),
        Tone::Failed => theme::accent(),
        Tone::Stopped => theme::text_muted(),
    };
    let line = row_button(selector)
        .self_start()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(ink)
        .on_click(move |_event, window, cx| toggle(Disclosure::Log(message), window, cx))
        .child(icon(
            if open { Glyph::Open } else { Glyph::Closed },
            px(12.),
            ink,
        ))
        .child(SharedString::from(text));
    let column = div().flex().flex_col().gap(px(4.)).pt(px(2.)).child(line);
    let card = match body {
        Body::Closed => return column,
        Body::Loading => notice("Loading the run log…"),
        Body::Failed => notice("Couldn't load the run log."),
        Body::Log { rows, footer } => {
            let selector = format!("runlog-{raw}-inspect");
            let inspect = on_disclose.clone();
            let link = button(selector)
                .self_end()
                .gap(px(2.))
                .text_size(px(11.5))
                .text_color(theme::text_secondary())
                .on_click(move |_event, window, cx| {
                    inspect(Disclosure::Inspect(message), window, cx)
                })
                .child("Open in panel")
                .child(icon(Glyph::Closed, px(12.), theme::text_secondary()));
            return column
                .child(log_card(
                    Owner::Message(message),
                    rows,
                    Some(footer),
                    on_disclose,
                ))
                .child(link);
        }
    };
    column.child(card)
}

fn notice(text: &'static str) -> Div {
    card()
        .px(px(12.))
        .py(px(8.))
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(text)
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .bg(theme::voice_card())
        .border_1()
        .border_color(theme::hairline())
        .overflow_hidden()
}

pub fn live_steps(owner: Owner, rows: Vec<RowView>, on_disclose: OnDisclose) -> Div {
    log_card(owner, rows, None, on_disclose)
}

fn log_card(
    owner: Owner,
    rows: Vec<RowView>,
    footer: Option<Footer>,
    on_disclose: OnDisclose,
) -> Div {
    let mut list = div().flex().flex_col().gap(px(2.)).px(px(12.)).py(px(8.));
    for (index, row) in rows.into_iter().enumerate() {
        list = list.child(row_element(&owner, index, row, on_disclose.clone()));
    }
    let card = card().child(list);
    match footer {
        None => card,
        Some(Footer {
            context: None,
            tokens: None,
        }) => card,
        Some(footer) => card.child(footer_element(footer)),
    }
}

pub fn row_element(owner: &Owner, index: usize, row: RowView, on_disclose: OnDisclose) -> Div {
    let raw = owner.key();
    match row {
        RowView::Thought {
            seq: _,
            text,
            first,
        } => {
            let mut thought = div().flex().flex_col().gap(px(1.)).py(px(2.));
            if first {
                thought = thought.child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_label())
                        .child("Thinking"),
                );
            }
            thought.child(div().text_size(px(13.)).child(rich::markdown(
                SharedString::from(format!("runlog-{raw}-thought-{index}")),
                text,
                Ink::Muted,
            )))
        }
        RowView::Tool(view) => tool_element(owner, view, on_disclose, px(0.)),
        RowView::Group {
            seq,
            name,
            description,
            status,
            duration,
            open,
            calls,
        } => {
            let selector = format!("runlog-{raw}-group-{seq}");
            let toggle = on_disclose.clone();
            let group_owner = owner.clone();
            let count = calls.len();
            let header = step_line(
                row_button(selector).on_click(move |_event, window, cx| {
                    toggle(Disclosure::Group(group_owner.clone(), seq), window, cx)
                }),
                StepLine {
                    name,
                    badge: Some(format!("×{count}")),
                    arg: description,
                    duration: Some(duration),
                    status: if open { None } else { Some(status) },
                    chevron: Some(open),
                },
            );
            let mut group = div().flex().flex_col().child(header);
            if open {
                for call in calls {
                    group = group.child(tool_element(owner, call, on_disclose.clone(), px(14.)));
                }
            }
            group
        }
        RowView::Task {
            seq: _,
            kind,
            description,
            state,
        } => div()
            .flex()
            .items_center()
            .gap(px(6.))
            .py(px(2.))
            .text_size(px(12.5))
            .child(
                div()
                    .px(px(5.))
                    .rounded(px(4.))
                    .bg(theme::sunken())
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_secondary())
                    .child("BACKGROUND"),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(kind)),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(theme::text_muted())
                    .child(SharedString::from(first_line(&description))),
            )
            .child(task_mark(state)),
        RowView::Status { seq: _, text } => div()
            .py(px(2.))
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(SharedString::from(format!("· {text}"))),
    }
}

struct StepLine {
    name: String,
    badge: Option<String>,
    arg: String,
    duration: Option<Duration>,
    status: Option<StepStatus>,
    chevron: Option<bool>,
}

fn step_line(line: Button, step: StepLine) -> Button {
    let StepLine {
        name,
        badge,
        arg,
        duration,
        status,
        chevron,
    } = step;
    let failed = status == Some(StepStatus::Error);
    let mut line = line
        .flex()
        .items_center()
        .gap(px(7.))
        .py(px(3.))
        .text_size(px(12.5))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if failed {
                    theme::accent()
                } else {
                    theme::text_primary()
                })
                .child(SharedString::from(name)),
        );
    if let Some(badge) = badge {
        line = line.child(
            div()
                .flex_none()
                .px(px(5.))
                .rounded(px(4.))
                .bg(theme::sunken())
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(badge)),
        );
    }
    line = line.child(
        div()
            .flex_1()
            .min_w(px(0.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .font_family(MONO)
            .text_size(px(11.5))
            .text_color(theme::text_muted())
            .child(SharedString::from(first_line(&arg))),
    );
    if let Some(duration) = duration {
        line = line.child(
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(theme::text_muted())
                .child(SharedString::from(duration_label(duration))),
        );
    }
    let mark = match (status, chevron) {
        (_, Some(true)) => Some(Mark::Icon(Glyph::Open, theme::text_muted())),
        (Some(StepStatus::Error), _) => Some(Mark::Icon(Glyph::Close, theme::accent())),
        (Some(StepStatus::Running), _) => Some(Mark::Busy),
        (Some(StepStatus::Ok), Some(false)) => Some(Mark::Icon(Glyph::Closed, theme::text_muted())),
        (Some(StepStatus::Ok), None) => Some(Mark::Icon(Glyph::Done, theme::status_idle())),
        (None, Some(false)) => Some(Mark::Icon(Glyph::Closed, theme::text_muted())),
        (None, None) => None,
    };
    match mark {
        Some(mark) => line.child(mark_element(mark)),
        None => line,
    }
}

fn tool_element(
    owner: &Owner,
    view: ToolView,
    on_disclose: OnDisclose,
    indent: gpui::Pixels,
) -> Div {
    let raw = owner.key();
    let ToolView { call, open, full } = view;
    let ToolCall {
        seq,
        name,
        full_name,
        arg,
        status,
        duration,
        input,
        result,
    } = call;
    let selector = format!("runlog-{raw}-step-{seq}");
    let toggle = on_disclose.clone();
    let step_owner = owner.clone();
    let name_shown = name.clone();
    let header = step_line(
        row_button(selector).on_click(move |_event, window, cx| {
            toggle(Disclosure::Step(step_owner.clone(), seq), window, cx)
        }),
        StepLine {
            name,
            badge: None,
            arg,
            duration,
            status: Some(status),
            chevron: None,
        },
    );
    let mut block = div().flex().flex_col().pl(indent).child(header);
    if !open {
        return block;
    }
    let failed = status == StepStatus::Error;
    let mut detail = div().flex().flex_col().gap(px(4.)).pl(px(2.)).pb(px(6.));
    if !input.is_empty() {
        detail = detail
            .child(label("Input"))
            .child(mono_box(input, MonoTone::Plain));
    }
    if !result.is_empty() {
        let (shown, clipped) = if full {
            (result.clone(), false)
        } else {
            clip(&result, RESULT_LINES)
        };
        detail = detail.child(label("Result")).child(mono_box(
            shown,
            if failed {
                MonoTone::Failed
            } else {
                MonoTone::Plain
            },
        ));
        let mut meta = div()
            .flex()
            .gap(px(10.))
            .text_size(px(11.))
            .text_color(theme::text_muted())
            .child(div().flex_1().font_family(MONO).child(SharedString::from(
                if full_name == name_shown {
                    String::new()
                } else {
                    full_name
                },
            )));
        if clipped || full {
            let selector = format!("runlog-{raw}-full-{seq}");
            let toggle = on_disclose.clone();
            let full_owner = owner.clone();
            meta = meta.child(
                button(selector)
                    .text_color(theme::text_secondary())
                    .on_click(move |_event, window, cx| {
                        toggle(Disclosure::FullResult(full_owner.clone(), seq), window, cx)
                    })
                    .child(if full { "Show less" } else { "Show full" }),
            );
        }
        detail = detail.child(meta);
    }
    block = block.child(detail);
    block
}

enum MonoTone {
    Plain,
    Failed,
}

fn mono_box(text: String, tone: MonoTone) -> Div {
    let (background, ink) = match tone {
        MonoTone::Plain => (theme::sunken(), theme::text_secondary()),
        MonoTone::Failed => (theme::failure_tint(), theme::accent()),
    };
    div()
        .px(px(9.))
        .py(px(6.))
        .rounded(px(6.))
        .bg(background)
        .font_family(MONO)
        .text_size(px(11.5))
        .line_height(relative(1.4))
        .text_color(ink)
        .child(SharedString::from(text))
}

fn label(text: &'static str) -> Div {
    div()
        .text_size(px(11.))
        .text_color(theme::text_muted())
        .child(text)
}

fn footer_element(footer: Footer) -> Div {
    let Footer { context, tokens } = footer;
    let mut line = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(7.))
        .border_t_1()
        .border_color(theme::hairline())
        .text_size(px(11.5))
        .text_color(theme::text_muted());
    if let Some((used, max)) = context {
        let share = (used as f32 / max as f32).clamp(0.0, 1.0);
        line = line
            .child("Context")
            .child(
                div()
                    .w(px(90.))
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
                compact(used),
                compact(max)
            )));
    }
    if let Some((input, output)) = tokens {
        line = line.child(SharedString::from(format!(
            "{} in · {} out",
            compact(input),
            compact(output)
        )));
    }
    line
}

enum Mark {
    Icon(Glyph, Hsla),
    Busy,
}

fn mark_element(mark: Mark) -> Div {
    let frame = div().flex_none().flex().justify_center().w(px(12.));
    match mark {
        Mark::Icon(glyph, tone) => frame.child(icon(glyph, px(12.), tone)),
        Mark::Busy => frame.child(spinner(px(12.), theme::text_muted())),
    }
}

fn task_mark(state: TaskState) -> Div {
    mark_element(match state {
        TaskState::Running => Mark::Busy,
        TaskState::Done => Mark::Icon(Glyph::Done, theme::status_idle()),
        TaskState::Failed => Mark::Icon(Glyph::Close, theme::accent()),
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(outcome: RunOutcome, steps: u32, tools: u32, millis: u64) -> RunRef {
        RunRef {
            id: "r".into(),
            outcome,
            steps,
            tools,
            duration: Duration::from_millis(millis),
        }
    }

    #[test]
    fn the_summary_names_tools_failures_and_stops() {
        assert_eq!(summary(&run(RunOutcome::Ok, 2, 0, 3_000)), None);
        assert_eq!(
            quick_duration(&run(RunOutcome::Ok, 2, 0, 3_000)),
            Some("3 s".into())
        );
        assert_eq!(
            summary(&run(RunOutcome::Ok, 6, 3, 13_029)),
            Some(Summary {
                text: "3 tools · 13 s".into(),
                tone: Tone::Plain
            })
        );
        assert_eq!(quick_duration(&run(RunOutcome::Ok, 6, 3, 13_029)), None);
        assert_eq!(
            summary(&run(RunOutcome::Error, 5, 3, 9_000)),
            Some(Summary {
                text: "Failed · 5 steps · 3 tools · 9 s".into(),
                tone: Tone::Failed
            })
        );
        assert_eq!(
            summary(&run(RunOutcome::Interrupted, 2, 1, 8_000)),
            Some(Summary {
                text: "Stopped by you · 1 tool · 8 s".into(),
                tone: Tone::Stopped
            })
        );
        assert!(opens_by_default(&run(RunOutcome::Error, 1, 1, 1)));
        assert!(!opens_by_default(&run(RunOutcome::Ok, 1, 1, 1)));
    }

    #[test]
    fn durations_and_counts_read_naturally() {
        assert_eq!(duration_label(Duration::from_millis(700)), "0.7 s");
        assert_eq!(duration_label(Duration::from_secs(13)), "13 s");
        assert_eq!(duration_label(Duration::from_secs(252)), "4 m 12 s");
        assert_eq!(compact(323_968), "324k");
        assert_eq!(compact(9_800), "9.8k");
        assert_eq!(compact(1_000_000), "1M");
        assert_eq!(compact(140), "140");
    }

    #[test]
    fn tool_names_lose_their_mcp_prefix() {
        assert_eq!(
            short_name("mcp__home-assistant__light__HassLightSet"),
            "HassLightSet"
        );
        assert_eq!(
            short_name("mcp__tuclaw-ipc__memory_update"),
            "memory_update"
        );
        assert_eq!(short_name("Bash"), "Bash");
    }

    fn step(seq: i64, kind: &str, extra: Value) -> StepRow {
        let mut row = json!({"seq": seq, "kind": kind, "started_at": "2026-10-03T15:26:00Z"});
        if let (Value::Object(row), Value::Object(extra)) = (&mut row, extra) {
            row.extend(extra);
        }
        serde_json::from_value(row).expect("a step row")
    }

    fn detail(steps: Vec<StepRow>) -> RunDetail {
        let mut detail: RunDetail =
            serde_json::from_str(include_str!("../../core/testdata/v3/run.json")).expect("run");
        detail.steps = steps;
        detail
    }

    #[test]
    fn repeated_tools_collapse_into_one_group() {
        let bash = |seq: i64, status: &str| {
            step(
                seq,
                "tool",
                json!({"name": "Bash", "status": status,
                    "input": {"command": "curl -s localhost", "description": "Polling qbittorrent"},
                    "finished_at": "2026-10-03T15:26:02Z"}),
            )
        };
        let rows = rows(
            &detail(vec![
                step(1, "text", json!({"output": "Checking the feed."})),
                bash(2, "ok"),
                bash(3, "ok"),
                bash(4, "error"),
                step(
                    5,
                    "tool",
                    json!({"name": "WebFetch", "status": "ok", "input": {"url": "https://rutracker.org/forum"}}),
                ),
            ]),
            "",
        );
        assert_eq!(rows.len(), 3);
        let Row::Group {
            seq,
            name,
            description,
            status,
            duration,
            calls,
        } = &rows[1]
        else {
            panic!("the three Bash calls form a group, got {:?}", rows[1]);
        };
        assert_eq!(*seq, 2);
        assert_eq!(name, "Bash");
        assert_eq!(description, "Polling qbittorrent");
        assert_eq!(*status, StepStatus::Error);
        assert_eq!(*duration, Duration::from_secs(6));
        assert_eq!(calls.len(), 3);
        let Row::Tool(fetch) = &rows[2] else {
            panic!("WebFetch stays a single call");
        };
        assert_eq!(fetch.arg, "rutracker.org/forum");
        assert_eq!(fetch.duration, None);
    }

    #[test]
    fn tasks_and_statuses_read_their_payloads() {
        let rows = rows(
            &detail(vec![
                step(
                    1,
                    "task",
                    json!({"name": "local_bash", "output": "{\"task_type\":\"local_bash\",\"description\":\"Download the image\"}"}),
                ),
                step(
                    2,
                    "status",
                    json!({"name": "compacting", "output": "context 91%"}),
                ),
                step(3, "mystery", json!({})),
            ]),
            "",
        );
        assert_eq!(
            rows,
            vec![
                Row::Task {
                    seq: 1,
                    task_id: String::new(),
                    kind: "local_bash".into(),
                    description: "Download the image".into(),
                    state: TaskState::Running,
                },
                Row::Status {
                    seq: 2,
                    text: "compacting · context 91%".into()
                },
            ]
        );
    }

    #[test]
    fn the_answer_is_not_repeated_as_the_last_thought() {
        let steps = vec![
            step(1, "text", json!({"output": "Посмотрю заметки."})),
            step(
                2,
                "tool",
                json!({"name": "mcp__telegram-user__get_messages", "status": "ok",
                    "input": {"dialog_id": 606980954, "dialog_type": "user", "limit": 15}}),
            ),
            step(3, "text", json!({"output": "Готово,  сэр.\n"})),
        ];
        let rows = rows(&detail(steps), "Готово, сэр.");
        assert_eq!(rows.len(), 2);
        let Row::Tool(call) = &rows[1] else {
            panic!("the tool stays");
        };
        assert_eq!(call.name, "get_messages");
        assert_eq!(
            call.arg,
            "dialog_id: 606980954 · dialog_type: user · limit: 15"
        );
    }

    #[test]
    fn a_background_task_is_one_row_that_follows_its_state() {
        let task = |seq: i64, payload: &str| {
            step(
                seq,
                "task",
                json!({"name": "local_bash", "output": payload}),
            )
        };
        let rows = rows(
            &detail(vec![
                task(
                    1,
                    "{\"task_id\":\"bb45\",\"task_type\":\"local_bash\",\"state\":\"started\",\"description\":\"Collect hall list\"}",
                ),
                step(2, "text", json!({"output": "Meanwhile, past showings."})),
                task(
                    3,
                    "{\"task_id\":\"bb45\",\"task_type\":\"local_bash\",\"state\":\"completed\",\"summary\":\"15 halls\"}",
                ),
                task(
                    4,
                    "{\"task_id\":\"cc01\",\"task_type\":\"local_agent\",\"state\":\"failed\",\"summary\":\"timed out\"}",
                ),
            ]),
            "",
        );
        assert_eq!(
            rows,
            vec![
                Row::Task {
                    seq: 1,
                    task_id: "bb45".into(),
                    kind: "local_bash".into(),
                    description: "Collect hall list".into(),
                    state: TaskState::Done,
                },
                Row::Thought {
                    seq: 2,
                    text: "Meanwhile, past showings.".into(),
                },
                Row::Task {
                    seq: 4,
                    task_id: "cc01".into(),
                    kind: "local_agent".into(),
                    description: "timed out".into(),
                    state: TaskState::Failed,
                },
            ]
        );
    }

    #[test]
    fn the_footer_carries_context_and_tokens() {
        let detail = detail(Vec::new());
        assert_eq!(
            footer(&detail),
            Footer {
                context: Some((323_968, 1_000_000)),
                tokens: Some((12 + 321_004 + 2_950, 412)),
            }
        );
    }

    #[test]
    fn long_results_are_clipped_to_a_few_lines() {
        let mut lines = Vec::new();
        for line in 1..=20 {
            lines.push(line.to_string());
        }
        let text = lines.join("\n");
        let (clipped, more) = clip(&text, RESULT_LINES);
        assert!(more);
        assert_eq!(clipped.lines().count(), RESULT_LINES);
        assert_eq!(clip("short", RESULT_LINES), ("short".to_string(), false));
    }
}
