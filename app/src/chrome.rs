use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, DispatchPhase, Div, HitboxBehavior, LongPressEvent, Pixels, Point, Stateful,
    TouchDragEvent, TouchPhase, Window, canvas, prelude::*,
};

use crate::message::OnToggle;

pub type OnTap = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OnHold = Rc<dyn Fn(Hold, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hold {
    Pressed,
    Moved(Point<Pixels>),
    Released,
}

#[derive(Clone)]
pub enum Chrome {
    Desktop,
    Phone(Touch),
}

#[derive(Clone)]
pub struct Touch {
    pub on_press: OnToggle,
    pub on_field: OnTap,
    pub on_drag: OnTap,
    pub on_hold: OnHold,
}

pub fn on_long_press(element: Stateful<Div>, on_press: OnTap) -> Stateful<Div> {
    element.relative().child(
        canvas(
            |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_bounds, hitbox, window, _cx| {
                let on_press = on_press.clone();
                window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                    let started = match event.phase {
                        TouchPhase::Started => true,
                        TouchPhase::Moved => false,
                        TouchPhase::Ended => false,
                        TouchPhase::Cancelled => false,
                    };
                    let bubbling = match phase {
                        DispatchPhase::Bubble => true,
                        DispatchPhase::Capture => false,
                    };
                    if !started || !bubbling || !hitbox.is_hovered(window) {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    on_press(window, cx);
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arming {
    Armed,
    Disarmed,
}

pub fn on_touch_drag(
    element: Stateful<Div>,
    arming: Arming,
    held: Rc<Cell<bool>>,
    on_hold: OnHold,
) -> Stateful<Div> {
    element.child(
        canvas(
            |_bounds, _window, _cx| {},
            move |bounds, _state, window, _cx| {
                let held = held.clone();
                let on_hold = on_hold.clone();
                window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                    match phase {
                        DispatchPhase::Bubble => {}
                        DispatchPhase::Capture => return,
                    }
                    match event.phase {
                        TouchPhase::Started => {
                            match arming {
                                Arming::Armed => {}
                                Arming::Disarmed => return,
                            }
                            if !bounds.contains(&event.start_position) {
                                return;
                            }
                            held.set(true);
                            window.prevent_default();
                            on_hold(Hold::Pressed, window, cx);
                        }
                        TouchPhase::Moved => {
                            if held.get() {
                                on_hold(
                                    Hold::Moved(event.position - event.start_position),
                                    window,
                                    cx,
                                );
                            }
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            if held.replace(false) {
                                on_hold(Hold::Released, window, cx);
                            }
                        }
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}
