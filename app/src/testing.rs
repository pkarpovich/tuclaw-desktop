use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AppContext, Entity, TestAppContext};
use tuclaw_core::model::ChannelId;
use tuclaw_core::v3::{Client, MockTransport, Pace, Scenario, Seed};

use crate::audio::{Pcm, PeakCache, Speaker};
use crate::link::Source;
use crate::recorder::{Recorder, Take};
use crate::state::AppState;

#[derive(Default)]
pub struct Sound {
    pub started: Vec<Pcm>,
    pub stops: usize,
    pub playing: bool,
    pub position: Duration,
    pub refuse: Option<String>,
}

#[derive(Clone, Default)]
pub struct FakeSpeaker(pub Rc<RefCell<Sound>>);

impl Speaker for FakeSpeaker {
    fn start(&mut self, pcm: Pcm) -> Result<(), String> {
        let mut sound = self.0.borrow_mut();
        if let Some(reason) = sound.refuse.clone() {
            return Err(reason);
        }
        sound.started.push(pcm);
        sound.playing = true;
        sound.position = Duration::ZERO;
        Ok(())
    }

    fn stop(&mut self) {
        let mut sound = self.0.borrow_mut();
        sound.stops += 1;
        sound.playing = false;
    }

    fn position(&self) -> Duration {
        self.0.borrow().position
    }

    fn finished(&self) -> bool {
        !self.0.borrow().playing
    }
}

#[derive(Default)]
pub struct Tape {
    pub recording: bool,
    pub cancelled: usize,
    pub refuse: Option<String>,
    pub take: Option<Vec<u8>>,
}

#[derive(Clone, Default)]
pub struct FakeRecorder(pub Rc<RefCell<Tape>>);

impl Recorder for FakeRecorder {
    fn start(&mut self) -> Result<(), String> {
        let mut tape = self.0.borrow_mut();
        if let Some(reason) = tape.refuse.clone() {
            return Err(reason);
        }
        tape.recording = true;
        Ok(())
    }

    fn finish(&mut self) -> Result<Take, String> {
        let mut tape = self.0.borrow_mut();
        if !tape.recording {
            return Err("nothing is being recorded".to_string());
        }
        tape.recording = false;
        let bytes = match tape.take.take() {
            Some(bytes) => bytes,
            None => b"....ftypM4A recording".to_vec(),
        };
        Ok(Take {
            kind: tuclaw_core::v3::AudioKind::M4a,
            bytes,
        })
    }

    fn cancel(&mut self) {
        let mut tape = self.0.borrow_mut();
        tape.recording = false;
        tape.cancelled += 1;
    }
}

pub fn mocked(cx: &mut TestAppContext, scenario: Scenario) -> (MockTransport, Entity<AppState>) {
    let (mock, state, _speaker) = speaking(cx, scenario);
    (mock, state)
}

pub fn speaking(
    cx: &mut TestAppContext,
    scenario: Scenario,
) -> (MockTransport, Entity<AppState>, FakeSpeaker) {
    speaking_with(cx, scenario, None)
}

pub fn speaking_with(
    cx: &mut TestAppContext,
    scenario: Scenario,
    cache: Option<PeakCache>,
) -> (MockTransport, Entity<AppState>, FakeSpeaker) {
    cx.update(gpui_kit::init);
    let mock = MockTransport::new(scenario, Pace::Stepped);
    let client = Client::mock(&mock);
    let speaker = FakeSpeaker::default();
    let boxed = Box::new(speaker.clone());
    let state = cx.new(|_| {
        let state = AppState::new(client, Source::Mock, boxed);
        match cache {
            Some(cache) => state.with_peak_cache(cache),
            None => state,
        }
    });
    state.update(cx, |state, cx| state.start(cx));
    cx.run_until_parked();
    mock.pump_control();
    cx.run_until_parked();
    (mock, state, speaker)
}

pub fn seeded(cx: &mut TestAppContext, seed: Seed) -> (MockTransport, Entity<AppState>) {
    cx.update(gpui_kit::init);
    let mock = MockTransport::seeded(seed, Scenario::default(), Pace::Stepped);
    let client = Client::mock(&mock);
    let state =
        cx.new(|_| AppState::new(client, Source::Snapshot, Box::new(FakeSpeaker::default())));
    state.update(cx, |state, cx| state.start(cx));
    cx.run_until_parked();
    mock.pump_control();
    cx.run_until_parked();
    (mock, state)
}

pub fn loaded(cx: &mut TestAppContext) -> (MockTransport, Entity<AppState>) {
    mocked(cx, Scenario::default())
}

pub fn play(mock: &MockTransport, cx: &mut TestAppContext) {
    mock.pump_control();
    mock.play_all();
    cx.run_until_parked();
}

pub fn channel_named(state: &Entity<AppState>, cx: &mut TestAppContext, name: &str) -> ChannelId {
    state.read_with(cx, |state, _cx| {
        let mut found = None;
        for channel in state.channels() {
            if channel.name == name {
                found = Some(channel.id);
                break;
            }
        }
        found.expect("the mock world carries that channel")
    })
}

pub fn long_world(count: i64) -> Seed {
    let surfaces = serde_json::from_str(include_str!("../../core/testdata/v3/surfaces.json"))
        .expect("surfaces");
    let agents =
        serde_json::from_str(include_str!("../../core/testdata/v3/agents.json")).expect("agents");
    let start = time::macros::datetime!(2026-10-01 00:00 UTC);
    let mut messages = Vec::new();
    for id in 1..=count {
        let created = start + time::Duration::hours(id);
        let created = created
            .format(&time::format_description::well_known::Rfc3339)
            .expect("formats");
        let message = serde_json::json!({
            "id": id, "surface_id": 1, "kind": "user", "author": {"kind": "user"},
            "text": format!("message {id}"), "created_at": created
        });
        messages.push(serde_json::from_value(message).expect("message"));
    }
    Seed {
        surfaces,
        agents,
        messages,
        runs: Vec::new(),
        media: Vec::new(),
        me: None,
        tasks: Vec::new(),
    }
}
