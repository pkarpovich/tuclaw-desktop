use gpui::{Context, Div, FocusHandle, MouseButton, Stateful, Subscription, Window, prelude::*};

pub fn follow_focus<V: 'static>(
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut Context<V>,
) -> [Subscription; 2] {
    [
        cx.on_focus(focus, window, |_, _, _| show()),
        cx.on_blur(focus, window, |_, _, _| hide()),
    ]
}

pub fn field(element: Stateful<Div>) -> Stateful<Div> {
    element.on_mouse_up(MouseButton::Left, |_, _, _| show())
}

pub fn show() {
    #[cfg(target_os = "ios")]
    gpui_mobile::show_keyboard();
}

pub fn hide() {
    #[cfg(target_os = "ios")]
    gpui_mobile::hide_keyboard();
}
