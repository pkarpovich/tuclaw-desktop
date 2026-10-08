use gpui::{
    Anchor, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::Popover;
use gpui_kit::base::input::{Input, InputEvent, InputState};
use tuclaw_core::model::ChannelId;
use tuclaw_core::v3::{self, GroupId};

use crate::control::{button, row_button};
use crate::form::field_frame;
use crate::icon::{Glyph, icon};
use crate::link;
use crate::state::{AppState, Direction};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Channel(ChannelId),
    Group(GroupId),
}

struct Editing {
    target: Target,
    input: Entity<InputState>,
    _subscription: Subscription,
}

pub struct ChannelsView {
    state: Entity<AppState>,
    new_group: Entity<InputState>,
    editing: Option<Editing>,
    deleting: Option<GroupId>,
    _subscriptions: Vec<Subscription>,
}

struct Shelf {
    group: Option<v3::Group>,
    surfaces: Vec<v3::Surface>,
}

impl ChannelsView {
    pub fn new(
        state: Entity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ChannelsView {
        let new_group =
            cx.new(|cx| InputState::new(window, cx).placeholder("New group, e.g. 🎬 Movie nights"));
        let subscriptions = vec![
            cx.observe(&state, |_view, _state, cx| cx.notify()),
            cx.subscribe_in(&new_group, window, Self::on_new_group),
        ];
        ChannelsView {
            state,
            new_group,
            editing: None,
            deleting: None,
            _subscriptions: subscriptions,
        }
    }

    fn on_new_group(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => self.add_group(window, cx),
            InputEvent::Change => {}
            InputEvent::Focus => {}
            InputEvent::Blur => {}
        }
    }

    fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.new_group.read(cx).value().to_string();
        if line.trim().is_empty() {
            return;
        }
        self.state
            .update(cx, |state, cx| state.create_group(line, cx));
        self.new_group
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn edit(&mut self, target: Target, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let subscription = cx.subscribe_in(&input, window, Self::on_edit);
        input.update(cx, |input, cx| input.focus_handle(cx).focus(window, cx));
        self.editing = Some(Editing {
            target,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn on_edit(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => self.save_edit(cx),
            InputEvent::Blur => self.save_edit(cx),
            InputEvent::Change => {}
            InputEvent::Focus => {}
        }
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(Editing {
            target,
            input,
            _subscription: _,
        }) = self.editing.take()
        else {
            return;
        };
        let value = input.read(cx).value().to_string();
        self.state.update(cx, |state, cx| match target {
            Target::Channel(channel) => state.rename_channel(channel, value, cx),
            Target::Group(group) => state.rename_group(group, value, cx),
        });
        cx.notify();
    }

    pub fn text_fields(&self, cx: &App) -> Vec<FocusHandle> {
        let mut fields = vec![self.new_group.read(cx).focus_handle(cx)];
        if let Some(editing) = &self.editing {
            fields.push(editing.input.read(cx).focus_handle(cx));
        }
        fields
    }

    fn editor(&self, target: Target) -> Option<Entity<InputState>> {
        let editing = self.editing.as_ref()?;
        if editing.target != target {
            return None;
        }
        Some(editing.input.clone())
    }

    fn shelves(state: &AppState) -> (Vec<Shelf>, Vec<v3::Surface>) {
        let mut groups = state.groups().to_vec();
        groups.sort_by_key(|group| (group.sort_order, group.id));
        let mut shelves = vec![Shelf {
            group: None,
            surfaces: Vec::new(),
        }];
        for group in groups {
            shelves.push(Shelf {
                group: Some(group),
                surfaces: Vec::new(),
            });
        }
        for surface in link::sidebar_order(state.surfaces(), state.groups()) {
            for shelf in &mut shelves {
                let id = shelf.group.as_ref().map(|group| group.id);
                if id == surface.group_id {
                    shelf.surfaces.push(surface.clone());
                }
            }
        }
        let mut archived = Vec::new();
        for surface in state.surfaces() {
            if surface.archived_at.is_some() {
                archived.push(surface.clone());
            }
        }
        (shelves, archived)
    }

    fn group_header(&self, group: &v3::Group, cx: &mut Context<Self>) -> Div {
        let v3::GroupId(raw) = group.id;
        let id = group.id;
        let title = link::group_title(group);
        let editing = self.editor(Target::Group(id));
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .pt(px(18.))
            .pb(px(6.))
            .px(px(4.));
        row = match editing {
            Some(input) => row.child(
                field_frame(false)
                    .flex_1()
                    .text_size(px(13.))
                    .child(Input::new(&input)),
            ),
            None => row
                .child(
                    div()
                        .flex_1()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_secondary())
                        .child(SharedString::from(title.clone())),
                )
                .child(self.tool(
                    format!("group-rename-{raw}"),
                    Glyph::Rename,
                    cx.listener(move |view, _event, window, cx| {
                        view.edit(Target::Group(id), title.clone(), window, cx)
                    }),
                )),
        };
        row = row
            .child(self.tool(
                format!("group-up-{raw}"),
                Glyph::Up,
                cx.listener(move |view, _event, _window, cx| {
                    view.state
                        .update(cx, |state, cx| state.move_group(id, Direction::Up, cx))
                }),
            ))
            .child(self.tool(
                format!("group-down-{raw}"),
                Glyph::Down,
                cx.listener(move |view, _event, _window, cx| {
                    view.state
                        .update(cx, |state, cx| state.move_group(id, Direction::Down, cx))
                }),
            ));
        if self.deleting == Some(id) {
            row.child(
                button(format!("group-delete-confirm-{raw}"))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(6.))
                    .bg(theme::accent())
                    .text_color(theme::chip_text())
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(cx.listener(move |view, _event, _window, cx| {
                        view.deleting = None;
                        view.state
                            .update(cx, |state, cx| state.delete_group(id, cx));
                    }))
                    .child("Delete group"),
            )
        } else {
            row.child(self.tool(
                format!("group-delete-{raw}"),
                Glyph::Close,
                cx.listener(move |view, _event, _window, cx| {
                    view.deleting = Some(id);
                    cx.notify();
                }),
            ))
        }
    }

    fn tool(
        &self,
        selector: String,
        glyph: Glyph,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui_kit::base::Button {
        button(selector)
            .p(px(5.))
            .rounded(px(6.))
            .hover(|style| style.bg(theme::sunken()))
            .on_click(on_click)
            .child(icon(glyph, px(14.), theme::text_secondary()))
    }

    fn channel_row(
        &self,
        surface: &v3::Surface,
        groups: &[v3::Group],
        cx: &mut Context<Self>,
    ) -> Div {
        let channel = link::channel_id(surface.id);
        let v3::SurfaceId(raw) = surface.id;
        let name = surface.name.clone();
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(8.))
            .hover(|style| style.bg(theme::sunken()))
            .child(icon(Glyph::Channel, px(14.), theme::text_muted()));
        row = match self.editor(Target::Channel(channel)) {
            Some(input) => row.child(
                field_frame(false)
                    .flex_1()
                    .text_size(px(13.))
                    .child(Input::new(&input)),
            ),
            None => {
                let mut label = div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
                    .items_baseline()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .child(SharedString::from(name.clone())),
                    );
                if surface.display_name.is_some() {
                    label = label.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme::text_muted())
                            .child(SharedString::from(format!("from {}", surface.topic_name))),
                    );
                }
                row.child(label).child(self.tool(
                    format!("channel-rename-{raw}"),
                    Glyph::Rename,
                    cx.listener(move |view, _event, window, cx| {
                        view.edit(Target::Channel(channel), name.clone(), window, cx)
                    }),
                ))
            }
        };
        row.child(self.group_menu(surface, groups))
            .child(self.tool(
                format!("channel-up-{raw}"),
                Glyph::Up,
                cx.listener(move |view, _event, _window, cx| {
                    view.state.update(cx, |state, cx| {
                        state.move_channel(channel, Direction::Up, cx)
                    })
                }),
            ))
            .child(self.tool(
                format!("channel-down-{raw}"),
                Glyph::Down,
                cx.listener(move |view, _event, _window, cx| {
                    view.state.update(cx, |state, cx| {
                        state.move_channel(channel, Direction::Down, cx)
                    })
                }),
            ))
            .child(
                self.tool(
                    format!("channel-archive-{raw}"),
                    Glyph::Archive,
                    cx.listener(move |view, _event, _window, cx| {
                        view.state
                            .update(cx, |state, cx| state.archive_channel(channel, true, cx))
                    }),
                )
                .accessibility_label("Archive"),
            )
    }

    fn group_menu(&self, surface: &v3::Surface, groups: &[v3::Group]) -> impl IntoElement {
        let channel = link::channel_id(surface.id);
        let v3::SurfaceId(raw) = surface.id;
        let state = self.state.clone();
        let mut current = "No group".to_string();
        for group in groups {
            if Some(group.id) == surface.group_id {
                current = link::group_title(group);
            }
        }
        let mut choices = vec![(None, "No group".to_string())];
        for group in groups {
            choices.push((Some(group.id), link::group_title(group)));
        }
        Popover::new(SharedString::from(format!("channel-group-{raw}")))
            .anchor(Anchor::TopRight)
            .offset(px(4.))
            .trigger(
                button(format!("channel-group-{raw}-trigger"))
                    .gap(px(5.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme::border())
                    .text_size(px(12.))
                    .text_color(theme::text_secondary())
                    .child(icon(Glyph::Folder, px(12.), theme::text_muted()))
                    .child(SharedString::from(current)),
            )
            .content(move |_popover, _window, cx| {
                let popover = cx.entity();
                let mut menu = div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .p(px(6.))
                    .w(px(220.))
                    .rounded(px(10.))
                    .bg(theme::raised())
                    .border_1()
                    .border_color(theme::border());
                for (index, (group, title)) in choices.iter().enumerate() {
                    let state = state.clone();
                    let popover = popover.clone();
                    let group = *group;
                    menu = menu.child(
                        row_button(format!("channel-group-{raw}-{index}"))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .text_size(px(12.5))
                            .hover(|style| style.bg(theme::sunken()))
                            .on_click(move |_event, window, cx| {
                                state
                                    .update(cx, |state, cx| state.file_channel(channel, group, cx));
                                popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                            })
                            .child(SharedString::from(title.clone())),
                    );
                }
                menu
            })
    }

    fn archived_row(&self, surface: &v3::Surface, cx: &mut Context<Self>) -> Div {
        let channel = link::channel_id(surface.id);
        let v3::SurfaceId(raw) = surface.id;
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(8.))
            .text_color(theme::text_muted())
            .child(icon(Glyph::Channel, px(14.), theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.5))
                    .child(SharedString::from(surface.name.clone())),
            )
            .child(
                button(format!("channel-restore-{raw}"))
                    .gap(px(5.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme::border())
                    .text_size(px(12.))
                    .text_color(theme::text_secondary())
                    .on_click(cx.listener(move |view, _event, _window, cx| {
                        view.state
                            .update(cx, |state, cx| state.archive_channel(channel, false, cx))
                    }))
                    .child(icon(Glyph::Restore, px(12.), theme::text_secondary()))
                    .child("Restore"),
            )
    }
}

impl Render for ChannelsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let (shelves, archived) = Self::shelves(state);
        let mut groups = state.groups().to_vec();
        groups.sort_by_key(|group| (group.sort_order, group.id));
        let error = state.channels_error().map(str::to_string);
        let mut visible = 0;
        for shelf in &shelves {
            visible += shelf.surfaces.len();
        }
        let mut header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(24.))
            .py(px(16.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Channels"),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(format!("{visible} shown · {} archived", archived.len())),
            );
        if let Some(error) = error {
            header = header.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::accent())
                    .child(SharedString::from(error)),
            );
        }
        let adder = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pb(px(6.))
            .child(
                field_frame(false)
                    .id("new-group")
                    .debug_selector(|| "new-group".to_string())
                    .flex_1()
                    .text_size(px(13.))
                    .child(Input::new(&self.new_group)),
            )
            .child(
                button("new-group-add")
                    .gap(px(5.))
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(7.))
                    .bg(theme::text_primary())
                    .text_color(theme::card())
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(cx.listener(|view, _event, window, cx| view.add_group(window, cx)))
                    .child(icon(Glyph::Add, px(12.), theme::card()))
                    .child("Add group"),
            );
        let mut body = div()
            .id("channels-body")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(24.))
            .py(px(16.))
            .child(adder);
        for shelf in shelves {
            let Shelf { group, surfaces } = shelf;
            body = match &group {
                Some(group) => body.child(self.group_header(group, cx)),
                None if surfaces.is_empty() => body,
                None => body.child(
                    div()
                        .pt(px(18.))
                        .pb(px(6.))
                        .px(px(4.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text_secondary())
                        .child("No group"),
                ),
            };
            if group.is_some() && surfaces.is_empty() {
                body = body.child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child("Empty. File channels here from their folder menu."),
                );
            }
            for surface in &surfaces {
                body = body.child(self.channel_row(surface, &groups, cx));
            }
        }
        if !archived.is_empty() {
            body = body.child(
                div()
                    .pt(px(24.))
                    .pb(px(6.))
                    .px(px(4.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child("Archived"),
            );
            for surface in &archived {
                body = body.child(self.archived_row(surface, cx));
            }
        }
        div()
            .id("channels")
            .debug_selector(|| "channels".to_string())
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use tuclaw_core::v3::SurfaceId;

    use crate::link;
    use crate::shell::Shell;
    use crate::state::{AppState, View};
    use crate::testing::{channel_named, loaded};

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn group_of(
        state: &Entity<AppState>,
        cx: &mut VisualTestContext,
        name: &str,
    ) -> Option<String> {
        state.read_with(cx, |state, _cx| {
            let mut group = None;
            for channel in state.channels() {
                if channel.name == name {
                    group = Some(channel.group.clone());
                }
            }
            group.expect("the channel is in the sidebar")
        })
    }

    #[gpui::test]
    fn channels_are_grouped_renamed_archived_and_restored(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        cx.run_until_parked();
        click(cx, "sidebar-channels");
        state.read_with(cx, |state, _cx| assert_eq!(state.view(), View::Channels));
        assert!(cx.debug_bounds("channels").is_some());

        state.update(cx, |state, cx| {
            state.create_group("🎬 Movie nights".into(), cx)
        });
        cx.run_until_parked();
        let group = state.read_with(cx, |state, _cx| state.groups()[0].clone());
        assert_eq!(group.emoji.as_deref(), Some("🎬"));
        assert_eq!(group.name, "Movie nights");

        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| {
            state.file_channel(magnet, Some(group.id), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            group_of(&state, cx, "Magnet Feed"),
            Some("🎬  Movie nights".to_string())
        );

        state.update(cx, |state, cx| {
            state.rename_channel(magnet, "Torrents".into(), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            group_of(&state, cx, "Torrents"),
            Some("🎬  Movie nights".to_string())
        );

        state.update(cx, |state, cx| state.archive_channel(magnet, true, cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            for channel in state.channels() {
                assert_ne!(channel.id, magnet);
            }
        });
        let restore: &'static str =
            Box::leak(format!("channel-restore-{}", link::surface_id(magnet).0).into_boxed_str());
        click(cx, restore);
        assert_eq!(
            group_of(&state, cx, "Torrents"),
            Some("🎬  Movie nights".to_string())
        );
        assert_eq!(link::surface_id(magnet), SurfaceId(2));
    }
}
