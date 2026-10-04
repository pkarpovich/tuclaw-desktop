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

pub fn text_label() -> Hsla {
    rgb(0x8d887f).into()
}

pub fn voice_card() -> Hsla {
    rgb(0xf7f5f2).into()
}

pub fn ink_soft() -> Hsla {
    rgb(0x57534e).into()
}

pub fn wave_rest() -> Hsla {
    rgb(0xc4bfb8).into()
}

pub fn field() -> Hsla {
    rgba(0xffffff99).into()
}

pub fn selection() -> Hsla {
    rgba(0x00000012).into()
}

pub fn badge() -> Hsla {
    rgb(0x26241f).into()
}

pub fn chip_text() -> Hsla {
    rgb(0xffffff).into()
}

pub fn accent() -> Hsla {
    rgb(0xb45c3c).into()
}

pub fn status_idle() -> Hsla {
    rgb(0x4ba36a).into()
}

pub fn status_busy() -> Hsla {
    rgb(0xe0a33a).into()
}

pub fn agent_chip(index: usize) -> Hsla {
    let tones = [rgb(0x6b7663), rgb(0x76605b), rgb(0x5c6975), rgb(0x3e4954)];
    tones[index % tones.len()].into()
}
