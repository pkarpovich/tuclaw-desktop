use std::time::Duration;

use gpui::{Point, TestAppContext, TouchEvent, TouchId, TouchPhase, VisualTestContext, px};
use tuclaw_desktop::state::{AppState, Recording};
use tuclaw_desktop::testing::loaded;

use crate::phone::Phone;

fn phone(cx: &mut TestAppContext) -> (gpui::Entity<AppState>, &mut VisualTestContext) {
    let (_mock, state) = loaded(cx);
    let built = state.clone();
    let (_phone, cx) = cx.add_window_view(move |window, cx| Phone::new(built, window, cx));
    cx.run_until_parked();
    (state, cx)
}

fn touch(cx: &mut VisualTestContext, phase: TouchPhase, position: Point<gpui::Pixels>) {
    cx.simulate_event(TouchEvent {
        id: TouchId(1),
        phase,
        position,
        predicted_position: None,
        force: None,
    });
    cx.run_until_parked();
}

fn press(cx: &mut VisualTestContext, selector: &'static str, held: Duration) {
    let center = cx
        .debug_bounds(selector)
        .expect("the target is drawn")
        .center();
    touch(cx, TouchPhase::Started, center);
    cx.executor().advance_clock(held);
    cx.run_until_parked();
    touch(cx, TouchPhase::Ended, center);
}

#[gpui::test]
fn a_long_press_on_a_channel_opens_its_menu_and_the_release_keeps_it(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(700));
    assert!(cx.debug_bounds("sheet-mark-unread").is_some());
}

#[gpui::test]
fn a_tap_on_a_channel_opens_it_and_back_returns_home(cx: &mut TestAppContext) {
    let (state, cx) = phone(cx);
    press(cx, "home-row-Smart Home", Duration::from_millis(50));
    assert!(cx.debug_bounds("conversation-back").is_some());
    state.read_with(cx, |state, _cx| {
        let selected = state.selected().expect("a channel is open");
        let mut name = String::new();
        for channel in state.channels() {
            if channel.id == selected {
                name = channel.name.clone();
            }
        }
        assert_eq!(name, "Smart Home");
    });
    press(cx, "conversation-back", Duration::from_millis(50));
    assert!(cx.debug_bounds("home-row-General").is_some());
    assert!(cx.debug_bounds("conversation-back").is_none());
}

#[gpui::test]
fn the_search_field_filters_channels_by_name(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-search", Duration::from_millis(50));
    cx.simulate_input("smart");
    cx.run_until_parked();
    assert!(cx.debug_bounds("home-row-Smart Home").is_some());
    assert!(cx.debug_bounds("home-row-General").is_none());
}

fn talking(cx: &mut TestAppContext) -> (gpui::Entity<AppState>, &mut VisualTestContext) {
    let (state, cx) = phone(cx);
    state.update(cx, |state, _cx| {
        state.set_recorder(Box::new(tuclaw_desktop::testing::FakeRecorder::default()))
    });
    press(cx, "home-row-General", Duration::from_millis(50));
    (state, cx)
}

fn recording(state: &gpui::Entity<AppState>, cx: &mut VisualTestContext) -> Recording {
    state.read_with(cx, |state, _cx| state.recording().clone())
}

fn live(recording: &Recording) -> bool {
    match recording {
        Recording::Live {
            since: _,
            channel: _,
        } => true,
        Recording::Idle => false,
        Recording::Sending => false,
        Recording::Failed(_) => false,
    }
}

#[gpui::test]
fn holding_the_mic_records_and_releasing_sends(cx: &mut TestAppContext) {
    let (state, cx) = talking(cx);
    let mic = cx.debug_bounds("composer-hold").expect("the mic").center();
    touch(cx, TouchPhase::Started, mic);
    assert!(live(&recording(&state, cx)));
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(cx.debug_bounds("talk-card").is_some());
    touch(cx, TouchPhase::Ended, mic);
    assert!(!live(&recording(&state, cx)));
    assert!(cx.debug_bounds("talk-card").is_none());
}

#[gpui::test]
fn a_quick_tap_on_the_mic_keeps_recording_hands_free(cx: &mut TestAppContext) {
    let (state, cx) = talking(cx);
    let mic = cx.debug_bounds("composer-hold").expect("the mic").center();
    touch(cx, TouchPhase::Started, mic);
    cx.executor().advance_clock(Duration::from_millis(100));
    touch(cx, TouchPhase::Ended, mic);
    assert!(live(&recording(&state, cx)));
    assert!(cx.debug_bounds("talk-card").is_none());
}

#[gpui::test]
fn sliding_left_cancels_and_sliding_up_locks(cx: &mut TestAppContext) {
    let (state, cx) = talking(cx);
    let mic = cx.debug_bounds("composer-hold").expect("the mic").center();
    touch(cx, TouchPhase::Started, mic);
    touch(cx, TouchPhase::Moved, mic - gpui::point(px(150.), px(0.)));
    assert_eq!(recording(&state, cx), Recording::Idle);
    touch(cx, TouchPhase::Ended, mic - gpui::point(px(150.), px(0.)));
    let mic = cx.debug_bounds("composer-hold").expect("the mic").center();
    touch(cx, TouchPhase::Started, mic);
    cx.executor().advance_clock(Duration::from_secs(1));
    touch(cx, TouchPhase::Moved, mic - gpui::point(px(0.), px(100.)));
    touch(cx, TouchPhase::Ended, mic - gpui::point(px(0.), px(100.)));
    assert!(live(&recording(&state, cx)));
}
