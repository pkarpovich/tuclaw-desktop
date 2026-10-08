use gpui::{Pixels, px};

pub struct Insets {
    pub top: Pixels,
    pub bottom: Pixels,
}

#[cfg(target_os = "ios")]
pub fn insets() -> Insets {
    let (top, bottom, _left, _right) = gpui_mobile::safe_area_insets();
    Insets {
        top: px(top),
        bottom: px(bottom),
    }
}

#[cfg(not(target_os = "ios"))]
pub fn insets() -> Insets {
    Insets {
        top: px(62.),
        bottom: px(34.),
    }
}

#[cfg(target_os = "ios")]
pub fn keyboard_height() -> Pixels {
    px(gpui_mobile::keyboard_height())
}

#[cfg(not(target_os = "ios"))]
pub fn keyboard_height() -> Pixels {
    px(0.)
}
