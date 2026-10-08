use gpui::{Context, FocusHandle, Subscription, Window};

pub fn hide_on_blur<V: 'static>(
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Subscription {
    cx.on_blur(focus, window, |_, _, _| hide())
}

pub fn show() {
    #[cfg(target_os = "ios")]
    gpui_mobile::show_keyboard();
}

pub fn hide() {
    #[cfg(target_os = "ios")]
    gpui_mobile::hide_keyboard();
}
