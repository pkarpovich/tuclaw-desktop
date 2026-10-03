use gpui::{AppContext, Entity, TestAppContext};
use tuclaw_core::model::ChannelId;
use tuclaw_core::v3::{Client, MockTransport, Pace, Scenario, Seed};

use crate::link::Source;
use crate::state::AppState;

pub fn mocked(cx: &mut TestAppContext, scenario: Scenario) -> (MockTransport, Entity<AppState>) {
    cx.update(gpui_kit::init);
    let mock = MockTransport::new(scenario, Pace::Stepped);
    let client = Client::mock(&mock);
    let state = cx.new(|_| AppState::new(client, Source::Mock));
    state.update(cx, |state, cx| state.start(cx));
    cx.run_until_parked();
    mock.pump_control();
    cx.run_until_parked();
    (mock, state)
}

pub fn seeded(cx: &mut TestAppContext, seed: Seed) -> (MockTransport, Entity<AppState>) {
    cx.update(gpui_kit::init);
    let mock = MockTransport::seeded(seed, Scenario::default(), Pace::Stepped);
    let client = Client::mock(&mock);
    let state = cx.new(|_| AppState::new(client, Source::Snapshot));
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
