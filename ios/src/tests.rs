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
    press(cx, "sheet-cancel", Duration::from_millis(50));
    assert!(cx.debug_bounds("sheet-mark-unread").is_none());
    assert!(cx.debug_bounds("conversation-back").is_none());
    assert!(cx.debug_bounds("home-search").is_some());
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

#[gpui::test]
fn an_agent_card_opens_its_settings_as_a_sheet(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "tab-agents", Duration::from_millis(50));
    press(cx, "agents-tab-1", Duration::from_millis(50));
    assert!(cx.debug_bounds("settings-close").is_some());
    press(cx, "settings-close", Duration::from_millis(50));
    assert!(cx.debug_bounds("settings-close").is_none());
    assert!(cx.debug_bounds("agents-tab-1").is_some());
}

#[gpui::test]
fn the_pencil_opens_channel_management_and_back_returns(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-channels", Duration::from_millis(50));
    assert!(cx.debug_bounds("channels-back").is_some());
    press(cx, "channels-back", Duration::from_millis(50));
    assert!(cx.debug_bounds("home-search").is_some());
}

#[gpui::test]
fn opening_a_channel_reads_what_arrived_while_on_home(cx: &mut TestAppContext) {
    let (mock, state) = loaded(cx);
    let built = state.clone();
    let (_phone, cx) = cx.add_window_view(move |window, cx| Phone::new(built, window, cx));
    cx.run_until_parked();
    mock.agent_posts(
        tuclaw_core::v3::SurfaceId(1),
        tuclaw_core::v3::AgentId(1),
        "While you were on the list.",
    );
    while mock.step() {}
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(unread(&state, cx, "General"), 1);
    press(cx, "home-row-General", Duration::from_millis(50));
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(unread(&state, cx, "General"), 0);
}

#[gpui::test]
fn the_inspector_keeps_taps_from_the_conversation_beneath(cx: &mut TestAppContext) {
    let (state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(50));
    state.update(cx, |state, cx| {
        let mut ran = None;
        for message in state.messages() {
            if message.run.is_some() {
                ran = Some(message.id);
            }
        }
        let ran = ran.expect("an answer with a run");
        state.toggle(tuclaw_desktop::runlog::Disclosure::Inspect(ran), cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("inspector").is_some());
    press(cx, "conversation-back", Duration::from_millis(50));
    assert!(cx.debug_bounds("inspector").is_some());
    assert!(cx.debug_bounds("conversation-back").is_some());
}

#[gpui::test]
fn the_settings_sheet_keeps_taps_from_the_cards_beneath(cx: &mut TestAppContext) {
    let (state, cx) = phone(cx);
    press(cx, "tab-agents", Duration::from_millis(50));
    let second = cx
        .debug_bounds("agents-tab-2")
        .expect("a second card")
        .center();
    press(cx, "agents-tab-1", Duration::from_millis(50));
    touch(cx, TouchPhase::Started, second);
    touch(cx, TouchPhase::Ended, second);
    let target = state.read_with(cx, |state, _cx| {
        state.settings().map(|settings| settings.target)
    });
    assert_eq!(
        target,
        Some(tuclaw_desktop::agent_settings::Target::Agent(
            tuclaw_core::model::AgentId(1)
        ))
    );
}

fn unread(state: &gpui::Entity<AppState>, cx: &mut VisualTestContext, name: &str) -> usize {
    state.read_with(cx, |state, _cx| {
        let mut unread = 0;
        for channel in state.channels() {
            if channel.name == name {
                unread = channel.unread;
            }
        }
        unread
    })
}

#[gpui::test]
fn the_automations_button_opens_the_channels_panel(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(50));
    press(cx, "conversation-automations", Duration::from_millis(50));
    assert!(cx.debug_bounds("automations-panel").is_some());
}

#[gpui::test]
fn opening_a_task_shows_it_on_the_automations_tab(cx: &mut TestAppContext) {
    let (state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(50));
    state.update(cx, |state, cx| {
        let task = state.tasks().first().map(|task| task.id.clone());
        state.open_task(task.expect("a task"), cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("conversation-back").is_none());
    assert!(cx.debug_bounds("automations-list").is_some());
}

#[gpui::test]
fn viewing_a_run_opens_its_channel(cx: &mut TestAppContext) {
    let (state, cx) = phone(cx);
    press(cx, "tab-agents", Duration::from_millis(50));
    state.update(cx, |state, cx| {
        state.view_run(tuclaw_core::model::AgentId(3), cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("conversation-back").is_some());
    let name = state.read_with(cx, |state, _cx| {
        let selected = state.selected();
        let mut name = String::new();
        for channel in state.channels() {
            if Some(channel.id) == selected {
                name = channel.name.clone();
            }
        }
        name
    });
    assert_eq!(name, "Magnet Feed");
}

#[gpui::test]
fn a_swipe_from_the_left_edge_goes_back(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(50));
    let edge = cx
        .debug_bounds("conversation-edge")
        .expect("the edge")
        .center();
    touch(cx, TouchPhase::Started, edge);
    touch(cx, TouchPhase::Moved, edge + gpui::point(px(60.), px(0.)));
    touch(cx, TouchPhase::Moved, edge + gpui::point(px(140.), px(0.)));
    touch(cx, TouchPhase::Ended, edge + gpui::point(px(140.), px(0.)));
    assert!(cx.debug_bounds("conversation-back").is_none());
    assert!(cx.debug_bounds("home-search").is_some());
}

#[gpui::test]
fn a_short_edge_swipe_stays(cx: &mut TestAppContext) {
    let (_state, cx) = phone(cx);
    press(cx, "home-row-General", Duration::from_millis(50));
    let edge = cx
        .debug_bounds("conversation-edge")
        .expect("the edge")
        .center();
    touch(cx, TouchPhase::Started, edge);
    touch(cx, TouchPhase::Moved, edge + gpui::point(px(30.), px(0.)));
    touch(cx, TouchPhase::Ended, edge + gpui::point(px(30.), px(0.)));
    assert!(cx.debug_bounds("conversation-back").is_some());
}

#[gpui::test]
fn a_sheet_over_the_mic_keeps_its_taps(cx: &mut TestAppContext) {
    let (state, cx) = talking(cx);
    let mic = cx.debug_bounds("composer-hold").expect("the mic").center();
    let ids = state.read_with(cx, |state, _cx| {
        let mut ids = Vec::new();
        for message in state.messages() {
            ids.push(message.id);
        }
        ids
    });
    let mut drawn = None;
    for id in ids.into_iter().rev() {
        let tuclaw_core::model::MessageId(raw) = id;
        let selector: &'static str = Box::leak(format!("message-{raw}").into_boxed_str());
        if let Some(bounds) = cx.debug_bounds(selector)
            && bounds.center().y > px(150.)
            && bounds.center().y < mic.y - px(100.)
        {
            drawn = Some(bounds);
            break;
        }
    }
    let message = drawn.expect("a message is drawn on screen");
    let held = gpui::point(message.origin.x + px(30.), message.center().y);
    touch(cx, TouchPhase::Started, held);
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
    touch(cx, TouchPhase::Ended, held);
    let cancel = cx.debug_bounds("sheet-cancel").expect("the menu is open");
    let over_mic = gpui::point(mic.x, cancel.center().y);
    touch(cx, TouchPhase::Started, over_mic);
    touch(cx, TouchPhase::Ended, over_mic);
    assert_eq!(recording(&state, cx), Recording::Idle);
    assert!(cx.debug_bounds("sheet-cancel").is_none());
}
