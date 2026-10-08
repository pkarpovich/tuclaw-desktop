use std::time::Duration;

use gpui::{Point, TestAppContext, TouchEvent, TouchId, TouchPhase, VisualTestContext};
use tuclaw_desktop::state::AppState;
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
