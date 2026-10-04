use super::frames::{Frame, decode};

pub(crate) fn fixture(name: &str) -> &'static str {
    match name {
        "event_frame" => include_str!("../../testdata/v3/event_frame.json"),
        "hello" => include_str!("../../testdata/v3/frames/hello.json"),
        "gap" => include_str!("../../testdata/v3/frames/gap.json"),
        "run_snapshot" => include_str!("../../testdata/v3/frames/run_snapshot.json"),
        "run_started" => include_str!("../../testdata/v3/frames/run_started.json"),
        "step_text" => include_str!("../../testdata/v3/frames/step_text.json"),
        "step_tool_started" => include_str!("../../testdata/v3/frames/step_tool_started.json"),
        "step_tool_started_truncated" => {
            include_str!("../../testdata/v3/frames/step_tool_started_truncated.json")
        }
        "step_tool_finished" => {
            include_str!("../../testdata/v3/frames/step_tool_finished.json")
        }
        "step_task" => include_str!("../../testdata/v3/frames/step_task.json"),
        "step_status" => include_str!("../../testdata/v3/frames/step_status.json"),
        "run_reset" => include_str!("../../testdata/v3/frames/run_reset.json"),
        "run_finished" => include_str!("../../testdata/v3/frames/run_finished.json"),
        "run_finished_interrupted" => {
            include_str!("../../testdata/v3/frames/run_finished_interrupted.json")
        }
        "message_created" => include_str!("../../testdata/v3/frames/message_created.json"),
        "text_delta" => include_str!("../../testdata/v3/frames/text_delta.json"),
        "input_accepted" => include_str!("../../testdata/v3/frames/input_accepted.json"),
        "unknown" => include_str!("../../testdata/v3/frames/unknown.json"),
        "focus" => include_str!("../../testdata/v3/frames/focus.json"),
        "task_fired" => include_str!("../../testdata/v3/frames/task_fired.json"),
        other => panic!("no fixture {other}"),
    }
}

pub(crate) fn frame(name: &str) -> Frame {
    decode(fixture(name)).unwrap()
}
