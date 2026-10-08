use std::rc::Rc;

use gpui::{
    App, DispatchPhase, Div, LongPressEvent, Stateful, TouchPhase, Window, canvas, prelude::*,
};

use crate::message::OnToggle;

pub type OnTap = Rc<dyn Fn(&mut Window, &mut App)>;

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
}

pub fn on_long_press(element: Stateful<Div>, on_press: OnTap) -> Stateful<Div> {
    element.relative().child(
        canvas(
            |_bounds, _window, _cx| {},
            move |bounds, _state, window, _cx| {
                let visible = bounds.intersect(&window.content_mask().bounds);
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
                    if !started || !bubbling || !visible.contains(&event.start_position) {
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
