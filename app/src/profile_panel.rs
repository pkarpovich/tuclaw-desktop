use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, PathPromptOptions,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::input::{Input, InputEvent, InputState, Textarea, TextareaState};

use crate::agent_settings::Field;
use crate::control::{AvatarSize, Face, avatar, button};
use crate::form::{error_line, field_frame, label, saving_label, upload_failure};
use crate::icon::{Glyph, icon};
use crate::link;
use crate::state::AppState;
use crate::theme;

const DESCRIPTION_LIMIT: usize = 280;

pub struct ProfilePanel {
    state: Entity<AppState>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    closing: Closing,
    _subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    Closable,
    Fixed,
}

impl ProfilePanel {
    pub fn new(
        state: Entity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ProfilePanel {
        let me = state.read(cx).people().me.clone();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(me.name.clone())
                .placeholder("Your name")
        });
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .default_value(me.description.clone())
                .placeholder("A few words about you")
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, Self::on_name),
            cx.subscribe_in(&description, window, Self::on_description),
            cx.observe(&state, |_panel, _state, cx| cx.notify()),
            cx.observe(&description, |_panel, _input, cx| cx.notify()),
        ];
        ProfilePanel {
            state,
            name,
            description,
            closing: Closing::Closable,
            _subscriptions: subscriptions,
        }
    }

    pub fn with_closing(mut self, closing: Closing) -> ProfilePanel {
        self.closing = closing;
        self
    }

    pub fn text_fields(&self, cx: &App) -> Vec<FocusHandle> {
        vec![
            self.name.read(cx).focus_handle(cx),
            self.description.read(cx).focus_handle(cx),
        ]
    }

    fn on_name(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Blur => self.save_name(cx),
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => self.save_name(cx),
            InputEvent::Change => {}
            InputEvent::Focus => {}
        }
    }

    fn on_description(
        &mut self,
        _input: &Entity<TextareaState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Blur => {
                let text = self.description.read(cx).value().to_string();
                self.state
                    .update(cx, |state, cx| state.save_my_description(text, cx));
            }
            InputEvent::Change => {}
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => {}
            InputEvent::Focus => {}
        }
    }

    fn save_name(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        self.state
            .update(cx, |state, cx| state.save_my_name(name, cx));
    }

    fn upload(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Upload".into()),
        });
        let state = self.state.clone();
        cx.spawn(async move |_panel, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                return;
            };
            state.update(cx, |state, cx| state.upload_my_avatar(bytes, cx));
        })
        .detach();
    }
}

impl Render for ProfilePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let people = state.people();
        let me = people.me.clone();
        let picture = people.picture(me.picture.as_ref());
        let has_picture = picture.is_some();
        let saving = state.saving().clone();
        let error = state.field_error().cloned();
        let failure = upload_failure(&saving, error.is_some());
        let (name_error, description_error) = match error {
            Some(error) => match error.field {
                Field::Name => (Some(error.message), None),
                Field::Description => (None, Some(error.message)),
                Field::Model => (None, None),
            },
            None => (None, None),
        };
        let face = Face {
            initials: SharedString::from(link::initials(&me.name)),
            color: theme::accent(),
            picture,
        };
        let length = self.description.read(cx).value().chars().count();
        let mut avatar_actions = div().flex().items_center().gap(px(10.)).child(
            button("profile-upload")
                .gap(px(5.))
                .px(px(9.))
                .py(px(4.))
                .rounded(px(7.))
                .border_1()
                .border_color(theme::border())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(cx.listener(|panel, _event, _window, cx| panel.upload(cx)))
                .child(icon(Glyph::Upload, px(12.), theme::text_secondary()))
                .child("Upload image"),
        );
        if has_picture {
            avatar_actions = avatar_actions.child(
                button("profile-remove-avatar")
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .on_click(cx.listener(|panel, _event, _window, cx| {
                        panel
                            .state
                            .update(cx, |state, cx| state.clear_my_avatar(cx));
                    }))
                    .child("Remove"),
            );
        }
        let mut name_field = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label("Name"))
            .child(
                field_frame(name_error.is_some())
                    .id("profile-name")
                    .debug_selector(|| "profile-name".to_string())
                    .text_size(px(13.))
                    .child(Input::new(&self.name)),
            );
        if let Some(error) = name_error {
            name_field = name_field.child(error_line(error));
        }
        let mut description_field = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("About you"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(if length > DESCRIPTION_LIMIT {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                            .child(format!("{length} / {DESCRIPTION_LIMIT}")),
                    ),
            )
            .child(
                field_frame(description_error.is_some())
                    .id("profile-description")
                    .debug_selector(|| "profile-description".to_string())
                    .text_size(px(13.))
                    .child(Textarea::new(&self.description)),
            );
        if let Some(error) = description_error {
            description_field = description_field.child(error_line(error));
        }
        div()
            .id("profile")
            .debug_selector(|| "profile".to_string())
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(theme::hairline())
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Your profile"),
                    )
                    .child(div().flex_1())
                    .child(saving_label(&saving))
                    .children(
                        match self.closing {
                            Closing::Closable => Some(()),
                            Closing::Fixed => None,
                        }
                        .map(|()| {
                            button("profile-close")
                                .accessibility_label("Close the profile")
                                .p(px(4.))
                                .rounded(px(6.))
                                .hover(|style| style.bg(theme::sunken()))
                                .on_click(cx.listener(|panel, _event, _window, cx| {
                                    panel
                                        .state
                                        .update(cx, |state, cx| state.close_inspector(cx));
                                }))
                                .child(icon(Glyph::Close, px(14.), theme::text_secondary()))
                        }),
                    ),
            )
            .child(
                div()
                    .id("profile-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .gap(px(18.))
                    .px(px(16.))
                    .py(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(avatar(face, AvatarSize::Profile))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(SharedString::from(me.name.clone())),
                                    )
                                    .child(avatar_actions)
                                    .children(failure),
                            ),
                    )
                    .child(name_field)
                    .child(description_field),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("Shown to you in the app; agents do not read it yet."),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext};

    use crate::agent_settings::Target;
    use crate::shell::Shell;
    use crate::testing::loaded;

    const AVIF: &[u8] = include_bytes!("../testdata/avatar.avif");

    #[gpui::test]
    fn an_avif_upload_becomes_the_picture_and_garbage_says_why(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        state.update(cx, |state, cx| state.open_profile(cx));
        cx.run_until_parked();
        state.update(cx, |state, cx| {
            state.upload_my_avatar(b"not an image".to_vec(), cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("upload-failure").is_some());
        let before = state.read_with(cx, |state, _cx| state.people().me.picture.clone());
        state.update(cx, |state, cx| state.upload_my_avatar(AVIF.to_vec(), cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let me = state.people().me;
            assert!(me.picture.is_some());
            assert_ne!(me.picture, before);
            assert_eq!(state.saving(), &crate::agent_settings::Saving::Saved);
        });
        assert!(cx.debug_bounds("upload-failure").is_none());
    }

    #[gpui::test]
    fn the_sidebar_gear_opens_the_profile_and_it_saves(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        cx.run_until_parked();
        let gear = cx
            .debug_bounds("sidebar-profile")
            .expect("the profile gear is drawn");
        cx.simulate_click(gear.center(), Modifiers::default());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.settings().map(|settings| settings.target),
                Some(Target::Me)
            );
        });
        assert!(cx.debug_bounds("profile").is_some());
        assert!(cx.debug_bounds("profile-name").is_some());
        state.update(cx, |state, cx| {
            state.save_my_name("Pavel".into(), cx);
            state.save_my_description("Builds tuclaw".into(), cx);
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let me = state.people().me;
            assert_eq!(me.name, "Pavel");
            assert_eq!(me.description, "Builds tuclaw");
            assert_eq!(state.field_error(), None);
        });
        state.update(cx, |state, cx| state.save_my_name("   ".into(), cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert!(state.field_error().is_some());
            assert_eq!(state.people().me.name, "Pavel");
        });
    }
}
