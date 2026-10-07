use gpui::{Context, FocusHandle, Subscription, Window};

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

fn show() {
    #[cfg(target_os = "ios")]
    gpui_mobile::show_keyboard();
}

fn hide() {
    #[cfg(target_os = "ios")]
    gpui_mobile::hide_keyboard();
}
