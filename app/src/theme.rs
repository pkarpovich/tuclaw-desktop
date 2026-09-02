use gpui::{Hsla, rgb, rgba};

pub fn window() -> Hsla {
    rgb(0xf6f4f1).into()
}

pub fn card() -> Hsla {
    rgb(0xfffefd).into()
}

pub fn raised() -> Hsla {
    rgb(0xffffff).into()
}

pub fn sunken() -> Hsla {
    rgba(0x0000000d).into()
}

pub fn border() -> Hsla {
    rgba(0x0000001a).into()
}

pub fn hairline() -> Hsla {
    rgba(0x00000014).into()
}

pub fn shadow() -> Hsla {
    rgba(0x1e1c1a38).into()
}

pub fn text_primary() -> Hsla {
    rgb(0x1c1b19).into()
}

pub fn text_secondary() -> Hsla {
    rgb(0x6f6a64).into()
}

pub fn text_muted() -> Hsla {
    rgb(0xa5a09a).into()
}
