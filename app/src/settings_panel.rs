use gpui::{
    Anchor, AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    PathPromptOptions, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use gpui_kit::base::Popover;
use gpui_kit::base::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use tuclaw_core::model::{AgentId, AgentStatus};
use tuclaw_core::v3::{self, Role};

use crate::agent_settings::{Field, Joinable, Saving, Settings, Toast, TopicRow};
use crate::control::{self, AvatarSize, Face, avatar, button, row_button};
use crate::form::{error_line, field_frame, label, saving_label, upload_failure};
use crate::icon::{Glyph, icon};
use crate::people::People;
use crate::state::AppState;
use crate::theme;

pub const WIDTH: f32 = 384.;
const DESCRIPTION_LIMIT: usize = 140;

pub struct SettingsPanel {
    state: Entity<AppState>,
    agent: AgentId,
    description: Entity<TextareaState>,
    model: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPanel {
    pub fn new(
        state: Entity<AppState>,
        agent: AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SettingsPanel {
        let (description, model) = match state.read(cx).directory_agent(agent) {
            Some(known) => (known.description.clone(), known.model.clone()),
            None => (String::new(), String::new()),
        };
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .default_value(description)
                .placeholder("What this agent does")
        });
        let model = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(model)
                .placeholder("default")
        });
        let subscriptions = vec![
            cx.subscribe_in(&description, window, Self::on_description),
            cx.subscribe_in(&model, window, Self::on_model),
            cx.observe(&state, |_panel, _state, cx| cx.notify()),
            cx.observe(&description, |_panel, _input, cx| cx.notify()),
        ];
        SettingsPanel {
            state,
            agent,
            description,
            model,
            _subscriptions: subscriptions,
        }
    }

    pub fn agent(&self) -> AgentId {
        self.agent
    }

    pub fn text_fields(&self, cx: &App) -> Vec<FocusHandle> {
        vec![
            self.description.read(cx).focus_handle(cx),
            self.model.read(cx).focus_handle(cx),
        ]
    }

    fn on_description(
        &mut self,
        _input: &Entity<TextareaState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Blur => self.save_description(cx),
            InputEvent::Change => {}
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => {}
            InputEvent::Focus => {}
        }
    }

    fn on_model(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Blur => self.save_model(cx),
            InputEvent::PressEnter {
                secondary: _,
                shift: _,
            } => self.save_model(cx),
            InputEvent::Change => {}
            InputEvent::Focus => {}
        }
    }

    fn save_description(&mut self, cx: &mut Context<Self>) {
        let text = self.description.read(cx).value().to_string();
        let agent = self.agent;
        self.state
            .update(cx, |state, cx| state.save_description(agent, text, cx));
    }

    fn save_model(&mut self, cx: &mut Context<Self>) {
        let spec = self.model.read(cx).value().to_string();
        let agent = self.agent;
        self.state
            .update(cx, |state, cx| state.save_model(agent, spec, cx));
    }

    fn reset_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model
            .update(cx, |input, cx| input.set_value("", window, cx));
        let agent = self.agent;
        self.state
            .update(cx, |state, cx| state.save_model(agent, String::new(), cx));
    }

    fn upload(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Upload".into()),
        });
        let state = self.state.clone();
        let agent = self.agent;
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
            state.update(cx, |state, cx| state.upload_avatar(agent, bytes, cx));
        })
        .detach();
    }

    fn header(
        &self,
        back: bool,
        name: SharedString,
        saving: &Saving,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut row = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(theme::hairline());
        if back {
            row = row.child(
                button("settings-back")
                    .accessibility_label("Back to the run")
                    .gap(px(2.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(theme::border())
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(cx.listener(|panel, _event, _window, cx| {
                        panel.state.update(cx, |state, cx| state.back_to_run(cx));
                    }))
                    .child(icon(Glyph::Back, px(12.), theme::text_secondary()))
                    .child("Run"),
            );
        }
        row.child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Agent settings"),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme::text_muted())
                        .child(name),
                ),
        )
        .child(div().flex_1())
        .child(saving_label(saving))
        .child(
            button("settings-close")
                .accessibility_label("Close the settings")
                .p(px(4.))
                .rounded(px(6.))
                .hover(|style| style.bg(theme::sunken()))
                .on_click(cx.listener(|panel, _event, _window, cx| {
                    panel
                        .state
                        .update(cx, |state, cx| state.close_inspector(cx));
                }))
                .child(icon(Glyph::Close, px(14.), theme::text_secondary())),
        )
    }

    fn identity(&self, face: Face, known: &v3::Agent, cx: &mut Context<Self>) -> Div {
        let has_picture = face.picture.is_some();
        let state = self.state.read(cx);
        let failure = upload_failure(state.saving(), state.field_error().is_some());
        let bot = known
            .bot_username
            .clone()
            .map(|bot| format!("@{bot}"))
            .unwrap_or_default();
        let mut actions = div().flex().items_center().gap(px(10.)).child(
            button("settings-upload")
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
            let agent = self.agent;
            actions = actions.child(
                button("settings-remove-avatar")
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .on_click(cx.listener(move |panel, _event, _window, cx| {
                        panel
                            .state
                            .update(cx, |state, cx| state.clear_avatar(agent, cx));
                    }))
                    .child("Remove"),
            );
        }
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
                    .min_w(px(0.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(known.name.clone()))
                                    .child(icon(Glyph::Lock, px(11.), theme::text_muted())),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_family(crate::runlog::MONO)
                                    .text_color(theme::text_muted())
                                    .child(SharedString::from(bot)),
                            ),
                    )
                    .child(actions)
                    .children(failure),
            )
    }

    fn description_field(&self, error: Option<String>, cx: &App) -> Div {
        let length = self.description.read(cx).value().chars().count();
        let counter = if length > DESCRIPTION_LIMIT {
            theme::accent()
        } else {
            theme::text_muted()
        };
        let mut field = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("Description"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(counter)
                            .child(format!("{length} / {DESCRIPTION_LIMIT}")),
                    ),
            )
            .child(
                field_frame(error.is_some())
                    .id("settings-description")
                    .debug_selector(|| "settings-description".to_string())
                    .text_size(px(13.))
                    .child(Textarea::new(&self.description)),
            );
        if let Some(error) = error {
            field = field.child(error_line(error));
        }
        field
    }

    fn model_field(&self, error: Option<String>, cx: &mut Context<Self>) -> Div {
        let mut field = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label("Model"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        field_frame(error.is_some())
                            .id("settings-model")
                            .debug_selector(|| "settings-model".to_string())
                            .flex_1()
                            .text_size(px(12.5))
                            .font_family(crate::runlog::MONO)
                            .child(Input::new(&self.model)),
                    )
                    .child(
                        button("settings-model-default")
                            .gap(px(4.))
                            .px(px(9.))
                            .py(px(6.))
                            .rounded(px(7.))
                            .bg(theme::sunken())
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .on_click(cx.listener(|panel, _event, window, cx| {
                                panel.reset_model(window, cx)
                            }))
                            .child(icon(Glyph::Reset, px(12.), theme::text_secondary()))
                            .child("Default"),
                    ),
            );
        field = match error {
            Some(error) => field.child(error_line(error)),
            None => field.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("model[context]:reasoning · leave empty to use the default"),
            ),
        };
        field
    }

    fn topics(&self, rows: Vec<TopicRow>, options: Vec<Joinable>) -> Div {
        let mut leads = Vec::new();
        let mut mentions = Vec::new();
        for row in rows {
            match row.role {
                Role::Lead => leads.push(row),
                Role::Mention => mentions.push(row),
                Role::Unknown => mentions.push(row),
            }
        }
        let count = leads.len() + mentions.len();
        let mut section = div().flex().flex_col().gap(px(8.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Topics"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(count.to_string()),
                )
                .child(div().flex_1())
                .child(self.add_to_topic(options)),
        );
        if !leads.is_empty() {
            let mut group = div().flex().flex_col().gap(px(4.)).child(group_label(
                "Lead",
                "answers everything · one per topic",
                false,
            ));
            for row in leads {
                group = group.child(lead_row(row));
            }
            section = section.child(group);
        }
        if !mentions.is_empty() {
            let mut group = div().flex().flex_col().gap(px(4.)).child(group_label(
                "On mention",
                "answers when @mentioned",
                true,
            ));
            for row in mentions {
                group = group.child(self.mention_row(row));
            }
            section = section.child(group).child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("Hears the rest: reads the whole conversation silently, even when another agent replies."),
            );
        }
        section
    }

    fn add_to_topic(&self, options: Vec<Joinable>) -> AnyElement {
        if options.is_empty() {
            return div().into_any_element();
        }
        let state = self.state.clone();
        let agent = self.agent;
        Popover::new("settings-add-topic")
            .anchor(Anchor::TopRight)
            .offset(px(6.))
            .trigger(
                button("settings-add-topic-trigger")
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(4.))
                    .rounded_full()
                    .bg(theme::text_primary())
                    .text_color(theme::card())
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(icon(Glyph::Add, px(11.), theme::card()))
                    .child("Add to topic"),
            )
            .content(move |_popover, _window, cx| {
                let popover = cx.entity();
                let mut menu = menu_frame().w(px(260.));
                for option in options {
                    let state = state.clone();
                    let popover = popover.clone();
                    let v3::SurfaceId(raw) = option.surface;
                    let hint = match &option.lead {
                        Some(lead) => format!("Lead: {lead}"),
                        None => "No Lead yet".to_string(),
                    };
                    let chosen = option.clone();
                    menu = menu.child(
                        row_button(format!("settings-join-{raw}"))
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .hover(|style| style.bg(theme::sunken()))
                            .on_click(move |_event, window, cx| {
                                state.update(cx, |state, cx| state.join_topic(agent, &chosen, cx));
                                popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                            })
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("# {}", option.name)),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(match option.lead {
                                        Some(_) => theme::text_muted(),
                                        None => theme::accent(),
                                    })
                                    .child(hint),
                            ),
                    );
                }
                menu.child(
                    div()
                        .px(px(10.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(theme::hairline())
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child("Joins as On mention. If the topic has no Lead, it joins as Lead."),
                )
            })
            .into_any_element()
    }

    fn mention_row(&self, row: TopicRow) -> Div {
        let v3::SurfaceId(raw) = row.surface;
        let surface = row.surface;
        let agent = self.agent;
        let toggler = self.state.clone();
        let state = self.state.clone();
        let name = row.name.clone();
        topic_frame(false)
            .child(topic_name(&row))
            .child(div().flex_1())
            .child(
                control::switch(format!("settings-hears-{raw}"), row.listens)
                    .accessibility_label(format!("Hears the rest in #{}", row.name))
                    .on_change(move |_checked, _event, _window, cx| {
                        toggler.update(cx, |state, cx| state.toggle_hears(agent, surface, cx));
                    }),
            )
            .child(
                Popover::new(SharedString::from(format!("settings-topic-menu-{raw}")))
                    .anchor(Anchor::TopRight)
                    .offset(px(4.))
                    .trigger(
                        button(format!("settings-topic-more-{raw}"))
                            .accessibility_label(format!("More for #{name}"))
                            .p(px(3.))
                            .rounded(px(5.))
                            .hover(|style| style.bg(theme::sunken()))
                            .child(icon(Glyph::More, px(14.), theme::text_secondary())),
                    )
                    .content(move |_popover, _window, cx| {
                        let popover = cx.entity();
                        let leader = state.clone();
                        let leaver = state.clone();
                        let first = popover.clone();
                        menu_frame()
                            .w(px(230.))
                            .child(
                                row_button(format!("settings-make-lead-{raw}"))
                                    .flex_col()
                                    .items_start()
                                    .px(px(10.))
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .bg(theme::accent())
                                    .text_color(theme::chip_text())
                                    .on_click(move |_event, window, cx| {
                                        leader.update(cx, |state, cx| {
                                            state.make_lead(agent, surface, cx)
                                        });
                                        first.update(cx, |popover, cx| popover.dismiss(window, cx));
                                    })
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(format!("Make Lead in #{name}")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .child("the current Lead becomes On mention"),
                                    ),
                            )
                            .child(
                                row_button(format!("settings-leave-{raw}"))
                                    .px(px(10.))
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .text_size(px(12.5))
                                    .hover(|style| style.bg(theme::sunken()))
                                    .on_click(move |_event, window, cx| {
                                        leaver.update(cx, |state, cx| {
                                            state.leave_topic(agent, surface, cx)
                                        });
                                        popover
                                            .update(cx, |popover, cx| popover.dismiss(window, cx));
                                    })
                                    .child(format!("Remove from #{name}")),
                            )
                    }),
            )
    }

    fn toast(&self, toast: &Toast, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .left(px(12.))
            .right(px(12.))
            .bottom(px(44.))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(9.))
            .rounded(px(9.))
            .bg(theme::text_primary())
            .text_color(theme::card())
            .text_size(px(12.5))
            .child(div().flex_1().child(SharedString::from(toast.text.clone())))
            .child(
                button("settings-undo")
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::status_busy())
                    .on_click(cx.listener(|panel, _event, _window, cx| {
                        panel.state.update(cx, |state, cx| state.undo(cx));
                    }))
                    .child("Undo"),
            )
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let agent = self.agent;
        let Some(known) = state.directory_agent(agent).cloned() else {
            return div().size_full().into_any_element();
        };
        let back = match state.settings() {
            Some(Settings { target: _, back }) => back.is_some(),
            None => false,
        };
        let saving = state.saving().clone();
        let error = state.field_error().cloned();
        let toast = state.toast().cloned();
        let rows = state.agent_topics(agent);
        let options = state.joinable_topics(agent);
        let people = state.people();
        let face = face_of(&people, agent);
        let status = status_of(&people, agent);
        let home = known
            .home_surface_id
            .and_then(|surface| state.surface_name(surface));
        let busy = known.live_run.is_some();
        let (description_error, model_error) = match error {
            Some(error) => match error.field {
                Field::Description => (Some(error.message), None),
                Field::Model => (None, Some(error.message)),
                Field::Name => (None, None),
            },
            None => (None, None),
        };
        let header = self.header(back, SharedString::from(known.name.clone()), &saving, cx);
        let identity = self.identity(face, &known, cx);
        let description = self.description_field(description_error, cx);
        let model = self.model_field(model_error, cx);
        let topics = self.topics(rows, options);
        let mut panel = div()
            .id("agent-settings")
            .debug_selector(|| "agent-settings".to_string())
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("settings-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .gap(px(18.))
                    .px(px(16.))
                    .py(px(16.))
                    .child(identity)
                    .child(self.status_block(status, home, busy, cx))
                    .child(description)
                    .child(model)
                    .child(topics),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(icon(Glyph::Lock, px(11.), theme::text_muted()))
                    .child("Name, bot and home topic are set in Telegram"),
            );
        if let Some(toast) = toast {
            panel = panel.child(self.toast(&toast, cx));
        }
        panel.into_any_element()
    }
}

impl SettingsPanel {
    fn status_block(
        &self,
        status: (SharedString, gpui::Hsla),
        home: Option<String>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (text, tone) = status;
        let mut status_cell = div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .flex_1()
            .child(cell_label("Status"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().size(px(6.)).rounded_full().bg(tone))
                    .child(text),
            );
        if busy {
            let agent = self.agent;
            status_cell = status_cell.child(
                button("settings-view-run")
                    .self_start()
                    .gap(px(2.))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::accent())
                    .on_click(cx.listener(move |panel, _event, _window, cx| {
                        panel
                            .state
                            .update(cx, |state, cx| state.view_run(agent, cx));
                    }))
                    .child("View run")
                    .child(icon(Glyph::Closed, px(11.), theme::accent())),
            );
        }
        div()
            .flex()
            .gap(px(1.))
            .rounded(px(9.))
            .bg(theme::sunken())
            .child(status_cell.px(px(10.)).py(px(8.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .flex_1()
                    .px(px(10.))
                    .py(px(8.))
                    .child(cell_label("Home topic"))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(match home {
                                Some(name) => format!("# {name}"),
                                None => "none".to_string(),
                            }),
                    ),
            )
    }
}

fn face_of(people: &People, agent: AgentId) -> Face {
    let mut found = None;
    for candidate in people.agents {
        if candidate.id == agent {
            found = Some(candidate);
        }
    }
    match found {
        Some(known) => Face {
            initials: SharedString::from(known.initials.clone()),
            color: theme::agent_chip(known.sort_index as usize),
            picture: people.picture(known.picture.as_ref()),
        },
        None => Face {
            initials: SharedString::new_static("··"),
            color: theme::agent_chip(0),
            picture: None,
        },
    }
}

fn status_of(people: &People, agent: AgentId) -> (SharedString, gpui::Hsla) {
    let mut status = None;
    for candidate in people.agents {
        if candidate.id == agent {
            status = Some(candidate.status.clone());
        }
    }
    match status {
        Some(AgentStatus::Busy(task)) => (
            SharedString::from(format!("Busy · {task}")),
            theme::status_busy(),
        ),
        Some(AgentStatus::Idle) => (SharedString::new_static("Idle"), theme::status_idle()),
        None => (SharedString::new_static("Unknown"), theme::text_muted()),
    }
}

fn cell_label(text: &'static str) -> Div {
    div()
        .text_size(px(10.5))
        .text_color(theme::text_muted())
        .child(text)
}

fn group_label(title: &'static str, hint: &'static str, hears: bool) -> Div {
    let line = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pt(px(4.))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::accent())
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(hint),
        );
    if !hears {
        return line;
    }
    line.child(div().flex_1()).child(
        div()
            .pr(px(30.))
            .text_size(px(10.))
            .text_color(theme::text_muted())
            .child("HEARS"),
    )
}

fn topic_frame(lead: bool) -> Div {
    let frame = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme::border());
    if lead {
        frame.bg(theme::failure_tint())
    } else {
        frame.bg(theme::raised())
    }
}

fn topic_name(row: &TopicRow) -> Div {
    let name = div().flex().items_center().gap(px(6.)).min_w(px(0.)).child(
        div()
            .text_size(px(12.5))
            .child(SharedString::from(format!("# {}", row.name))),
    );
    if !row.home {
        return name;
    }
    name.child(
        div()
            .px(px(5.))
            .rounded(px(4.))
            .bg(theme::sunken())
            .text_size(px(10.))
            .text_color(theme::text_muted())
            .child("home"),
    )
}

fn lead_row(row: TopicRow) -> Div {
    topic_frame(true)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(topic_name(&row).font_weight(FontWeight::SEMIBOLD))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child("Assign another Lead to remove"),
                ),
        )
        .child(div().flex_1())
        .child(icon(Glyph::Lock, px(12.), theme::text_muted()))
}

fn menu_frame() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(6.))
        .rounded(px(10.))
        .bg(theme::raised())
        .border_1()
        .border_color(theme::border())
        .shadow(vec![
            gpui::BoxShadow::new(px(0.), px(8.), theme::shadow())
                .blur_radius(px(24.))
                .spread_radius(px(-8.)),
        ])
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use tuclaw_core::model::{AgentId, MessageId};
    use tuclaw_core::v3;

    use crate::shell::Shell;
    use crate::state::{AppState, Filter, Inspector};
    use crate::testing::loaded;

    fn opened(
        cx: &mut TestAppContext,
        agent: AgentId,
    ) -> (Entity<AppState>, &mut VisualTestContext) {
        let (_mock, state) = loaded(cx);
        state.update(cx, |state, cx| state.open_settings(agent, cx));
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        cx.run_until_parked();
        (state, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn listens(state: &Entity<AppState>, cx: &mut VisualTestContext) -> bool {
        state.read_with(cx, |state, _cx| {
            let mut listens = false;
            for row in state.agent_topics(AgentId(3)) {
                if row.surface == v3::SurfaceId(1) {
                    listens = row.listens;
                }
            }
            listens
        })
    }

    #[gpui::test]
    fn the_panel_shows_the_agent_and_its_topics(cx: &mut TestAppContext) {
        let (_state, cx) = opened(cx, AgentId(3));
        for selector in [
            "agent-settings",
            "settings-description",
            "settings-model",
            "settings-hears-1",
            "settings-topic-more-1",
            "settings-add-topic-trigger",
        ] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
        }
        assert!(cx.debug_bounds("settings-back").is_none());
    }

    #[gpui::test]
    fn hears_toggles_on_click(cx: &mut TestAppContext) {
        let (state, cx) = opened(cx, AgentId(3));
        assert!(!listens(&state, cx));
        click(cx, "settings-hears-1");
        assert!(listens(&state, cx));
    }

    #[gpui::test]
    fn the_run_comes_back_from_the_header(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let run = Inspector {
            message: MessageId(1),
            filter: Filter::All,
        };
        state.update(cx, |state, cx| {
            state.toggle(crate::runlog::Disclosure::Inspect(MessageId(1)), cx);
            state.open_settings(AgentId(3), cx);
        });
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        cx.run_until_parked();
        click(cx, "settings-back");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.settings(), None);
            assert_eq!(state.inspector(), Some(run));
        });
    }

    #[gpui::test]
    fn the_topic_menu_makes_the_agent_lead_with_an_undo(cx: &mut TestAppContext) {
        let (state, cx) = opened(cx, AgentId(3));
        click(cx, "settings-topic-more-1");
        click(cx, "settings-make-lead-1");
        state.read_with(cx, |state, _cx| {
            let mut role = None;
            for row in state.agent_topics(AgentId(3)) {
                if row.surface == v3::SurfaceId(1) {
                    role = Some(row.role);
                }
            }
            assert_eq!(role, Some(v3::Role::Lead));
        });
        click(cx, "settings-undo");
        state.read_with(cx, |state, _cx| {
            let mut role = None;
            for row in state.agent_topics(AgentId(3)) {
                if row.surface == v3::SurfaceId(1) {
                    role = Some(row.role);
                }
            }
            assert_eq!(role, Some(v3::Role::Mention));
        });
    }

    #[gpui::test]
    fn close_empties_the_slot(cx: &mut TestAppContext) {
        let (state, cx) = opened(cx, AgentId(3));
        click(cx, "settings-close");
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.settings(), None);
            assert_eq!(state.inspector(), None);
        });
        assert!(cx.debug_bounds("agent-settings").is_none());
    }
}
