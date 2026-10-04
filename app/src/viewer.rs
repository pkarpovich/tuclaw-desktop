use gpui::{
    AnyElement, App, Entity, FocusHandle, FontWeight, ImageSource, ObjectFit, SharedString, Window,
    div, hsla, img, prelude::*, px,
};
use gpui_kit::base::Dialog;

use crate::control::button;
use crate::icon::{Glyph, icon};
use crate::pictures::{Remote, Viewed};
use crate::state::AppState;

const MARGIN: f32 = 48.;
const FOOTER: f32 = 56.;

pub fn viewer(
    state: &Entity<AppState>,
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let Viewed { url, caption } = state.read(cx).viewer()?.clone();
    let Some(Remote::Ready(shown)) = state.read(cx).pictures().get(&url).cloned() else {
        return None;
    };
    let viewport = window.viewport_size();
    let room_width = f32::from(viewport.width) - MARGIN * 2.;
    let room_height = f32::from(viewport.height) - MARGIN * 2. - FOOTER;
    let natural_width = shown.width as f32;
    let natural_height = shown.height as f32;
    let scale = (room_width / natural_width)
        .min(room_height / natural_height)
        .clamp(0.01, 1.);
    let width = (natural_width * scale).round();
    let height = (natural_height * scale).round();
    let closer = state.clone();
    let dismisser = state.clone();
    let opened = url.as_str().to_string();
    let mut footer = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .w(px(width))
        .pt(px(14.))
        .text_size(px(13.))
        .text_color(hsla(0., 0., 1., 0.78));
    footer = footer.child(
        div()
            .flex_1()
            .min_w(px(0.))
            .text_ellipsis()
            .child(SharedString::from(caption.clone())),
    );
    footer = footer
        .child(
            button("viewer-open")
                .px(px(10.))
                .py(px(5.))
                .rounded(px(7.))
                .bg(hsla(0., 0., 1., 0.12))
                .hover(|style| style.bg(hsla(0., 0., 1., 0.2)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(0., 0., 1., 0.92))
                .on_click(move |_event, _window, cx| cx.open_url(&opened))
                .child("Open in browser"),
        )
        .child(
            button("viewer-close")
                .accessibility_label("Close the picture")
                .p(px(6.))
                .rounded(px(7.))
                .hover(|style| style.bg(hsla(0., 0., 1., 0.12)))
                .on_click(move |_event, _window, cx| {
                    closer.update(cx, |state, cx| state.close_picture(cx));
                })
                .child(icon(Glyph::Close, px(16.), hsla(0., 0., 1., 0.85))),
        );
    let picture = div()
        .id("picture-viewer")
        .debug_selector(|| "picture-viewer".to_string())
        .flex()
        .flex_col()
        .items_center()
        .child(
            img(ImageSource::Image(shown.image))
                .w(px(width))
                .h(px(height))
                .rounded(px(8.))
                .object_fit(ObjectFit::Contain),
        )
        .child(footer);
    let dialog = Dialog::new(cx)
        .focus_handle(focus.clone())
        .on_open_change(move |open, _reason, _window, cx| {
            if !open {
                dismisser.update(cx, |state, cx| state.close_picture(cx));
            }
        })
        .backdrop(div().size_full().bg(hsla(0., 0., 0., 0.86)))
        .popup(picture);
    Some(dialog.into_any_element())
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{Author, MessageId};
    use tuclaw_core::v3::{AgentId, SurfaceId};

    use crate::shell::Shell;
    use crate::testing::loaded;

    const TURTLE: &[u8] = include_bytes!("../../core/testdata/v3/media/avatar_agent.png");

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_picture_opens_full_window_and_escape_or_close_dismisses_it(cx: &mut TestAppContext) {
        let (mock, state) = loaded(cx);
        mock.serve_public("https://media.example.test/turtle.png", TURTLE.to_vec());
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        cx.run_until_parked();
        mock.agent_posts(
            SurfaceId(1),
            AgentId(1),
            "![A turtle](https://media.example.test/turtle.png)",
        );
        while mock.step() {}
        cx.run_until_parked();
        let raw = state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if let Author::Agent(_) = message.author {
                    let MessageId(raw) = message.id;
                    found = Some(raw);
                }
            }
            found.expect("the post arrived")
        });
        let picture: &'static str = Box::leak(format!("message-{raw}-md-picture").into_boxed_str());
        click(cx, picture);
        assert!(cx.debug_bounds("picture-viewer").is_some());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.debug_bounds("picture-viewer").is_none());
        state.read_with(cx, |state, _cx| assert!(state.viewer().is_none()));
        click(cx, picture);
        click(cx, "viewer-close");
        assert!(cx.debug_bounds("picture-viewer").is_none());
    }
}
