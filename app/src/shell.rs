use gpui::{
    AnyElement, BoxShadow, ClipboardItem, Context, Div, Entity, FocusHandle, Hsla, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Subscription, Window,
    actions, anchored, deferred, div, phi, point, prelude::*, px,
};
use gpui_kit::base::TextSelection;

use crate::agent_settings::Target;
use crate::agents::AgentsView;
use crate::automations_panel;
use crate::automations_view::AutomationsView;
use crate::channels_view::ChannelsView;
use crate::control::{self, button};
use crate::feed::Feed;
use crate::icon::{Glyph, icon};
use crate::inspector::{self, InspectorActions, InspectorInput, OnClose, OnFilter};
use crate::link::Source;
use crate::profile_panel::ProfilePanel;
use crate::runlog::OnDisclose;
use crate::settings_panel::{self, SettingsPanel};

#[derive(Clone)]
enum SlotPanel {
    Agent(Entity<SettingsPanel>),
    Me(Entity<ProfilePanel>),
}
use crate::sidebar::Sidebar;
use crate::state::{AppState, Link, Segment, SidebarVisibility, StateEvent, View};
use crate::theme;
use crate::viewer;

actions!(tuclaw_shell, [ToggleSidebar]);

const TOOLBAR_HEIGHT: f32 = 48.;
const HAIRLINE: f32 = 1.;
const GUTTER: f32 = 10.;
const SIDEBAR_WIDTH: f32 = 250.;
const CONTENT_INSET: f32 = 6.;
const TRAFFIC_LIGHT_SIZE: f32 = 14.;
const TRAFFIC_LIGHTS_WIDTH: f32 = 60.;
const TRAFFIC_LIGHTS_GAP: f32 = 14.;

pub fn traffic_light_position() -> Point<Pixels> {
    point(
        px(GUTTER + CONTENT_INSET),
        px((TOOLBAR_HEIGHT - TRAFFIC_LIGHT_SIZE) / 2.),
    )
}

#[derive(Clone)]
struct CopyMenu {
    position: Point<Pixels>,
    text: String,
}

pub struct Shell {
    state: Entity<AppState>,
    sidebar: Entity<Sidebar>,
    feed: Entity<Feed>,
    agents: Entity<AgentsView>,
    automations: Entity<AutomationsView>,
    channels: Entity<ChannelsView>,
    settings: Option<SlotPanel>,
    copy_menu: Option<CopyMenu>,
    viewer_focus: FocusHandle,
    _observation: Subscription,
    _events: Subscription,
}

impl Shell {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Shell {
        let observation = cx.observe(&state, |_shell, _state, cx| cx.notify());
        let events = cx.subscribe_in(
            &state,
            window,
            |shell, _state, event: &StateEvent, window, cx| match event {
                StateEvent::PictureOpened => {
                    window.focus(&shell.viewer_focus, cx);
                }
                StateEvent::SelectionChanged => {}
                StateEvent::MessagesLoaded => {}
                StateEvent::MessageAppended => {}
                StateEvent::RunsChanged => {}
                StateEvent::FoldToggled => {}
                StateEvent::OlderLoaded => {}
                StateEvent::PicturesLoaded => {}
                StateEvent::SendFailed(_) => {}
                StateEvent::Mention(_) => {}
                StateEvent::TasksLoaded => {}
                StateEvent::ChannelsChanged => {}
            },
        );
        let built = state.clone();
        let sidebar = cx.new(|cx| Sidebar::new(built, cx));
        let built = state.clone();
        let feed = cx.new(|cx| Feed::new(built, window, cx));
        let built = state.clone();
        let agents = cx.new(|cx| AgentsView::new(built, cx));
        let built = state.clone();
        let automations = cx.new(|cx| AutomationsView::new(built, cx));
        let built = state.clone();
        let channels = cx.new(|cx| ChannelsView::new(built, window, cx));
        Shell {
            state,
            sidebar,
            feed,
            agents,
            automations,
            channels,
            settings: None,
            copy_menu: None,
            viewer_focus: cx.focus_handle(),
            _observation: observation,
            _events: events,
        }
    }

    fn sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self
            .state
            .read(cx)
            .settings()
            .map(|settings| settings.target);
        let current = match &self.settings {
            Some(SlotPanel::Agent(panel)) => Some(Target::Agent(panel.read(cx).agent())),
            Some(SlotPanel::Me(_)) => Some(Target::Me),
            None => None,
        };
        if wanted == current {
            return;
        }
        let state = self.state.clone();
        self.settings = match wanted {
            Some(Target::Agent(agent)) => Some(SlotPanel::Agent(
                cx.new(|cx| SettingsPanel::new(state, agent, window, cx)),
            )),
            Some(Target::Me) => Some(SlotPanel::Me(
                cx.new(|cx| ProfilePanel::new(state, window, cx)),
            )),
            None => None,
        };
    }

    fn settings_card(&self) -> Option<AnyElement> {
        let panel = match self.settings.clone()? {
            SlotPanel::Agent(panel) => panel.into_any_element(),
            SlotPanel::Me(panel) => panel.into_any_element(),
        };
        Some(
            card()
                .id("settings-card")
                .flex_none()
                .w(px(settings_panel::WIDTH))
                .overflow_hidden()
                .child(panel)
                .into_any_element(),
        )
    }

    fn body(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let state = self.state.read(cx);
        let view = state.view();
        match view {
            View::Channels => vec![
                card()
                    .id("content-card")
                    .debug_selector(|| "content-card".to_string())
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(self.channels.clone())
                    .into_any_element(),
            ],
            View::Automations => {
                let mut cards = vec![
                    card()
                        .id("content-card")
                        .debug_selector(|| "content-card".to_string())
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(self.automations.clone())
                        .into_any_element(),
                ];
                if let Some(panel) = self.settings_card() {
                    cards.push(panel);
                }
                cards
            }
            View::Agents => {
                let mut cards = vec![
                    card()
                        .id("content-card")
                        .debug_selector(|| "content-card".to_string())
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(self.agents.clone())
                        .into_any_element(),
                ];
                if let Some(panel) = self.settings_card() {
                    cards.push(panel);
                }
                cards
            }
            View::Conversation => {
                let mut cards = vec![
                    card()
                        .id("content-card")
                        .debug_selector(|| "content-card".to_string())
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(self.feed.clone())
                        .into_any_element(),
                ];
                if let Some(panel) = self.settings_card() {
                    cards.push(panel);
                } else if let Some(panel) = self.automations_card(cx) {
                    cards.push(panel);
                } else if let Some(panel) = self.inspector_card(cx) {
                    cards.push(panel);
                }
                cards
            }
        }
    }

    fn automations_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state.read(cx);
        if !state.automations_open() {
            return None;
        }
        let selected = state.selected()?;
        let mut channel = SharedString::default();
        for candidate in state.channels() {
            if candidate.id == selected {
                channel = SharedString::from(candidate.name.clone());
            }
        }
        let closer = self.state.clone();
        let on_close: automations_panel::OnClose = std::rc::Rc::new(move |_window, cx| {
            closer.update(cx, |state, cx| state.close_automations(cx));
        });
        let opener = self.state.clone();
        let on_task: crate::automation::OnTask = std::rc::Rc::new(move |task, _window, cx| {
            let task = task.clone();
            opener.update(cx, |state, cx| state.open_task(task, cx));
        });
        let toggler = self.state.clone();
        let on_toggle_skipped: automations_panel::OnToggle =
            std::rc::Rc::new(move |_window, cx| {
                toggler.update(cx, |state, cx| state.toggle_skipped(cx));
            });
        let panel = automations_panel::render(
            automations_panel::PanelInput {
                channel,
                tasks: state.channel_tasks(),
                fires: state.fires(),
                show_skipped: state.show_skipped(),
                now: time::OffsetDateTime::now_utc(),
            },
            automations_panel::PanelActions {
                on_close,
                on_task,
                on_toggle_skipped,
            },
        );
        Some(
            card()
                .id("automations-card")
                .flex_none()
                .w(px(automations_panel::WIDTH))
                .overflow_hidden()
                .child(panel)
                .into_any_element(),
        )
    }

    fn inspector_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let inspector = state.inspector()?;
        let mut found = None;
        for message in state.messages() {
            if message.id == inspector.message {
                found = Some(message);
            }
        }
        let message = found?;
        let log = match &message.run {
            Some(run) => state.run_log(&run.id),
            None => None,
        };
        let discloser = self.state.clone();
        let on_disclose: OnDisclose = std::rc::Rc::new(move |disclosure, _window, cx| {
            discloser.update(cx, |state, cx| state.toggle(disclosure, cx));
        });
        let closer = self.state.clone();
        let on_close: OnClose = std::rc::Rc::new(move |_window, cx| {
            closer.update(cx, |state, cx| state.close_inspector(cx));
        });
        let filterer = self.state.clone();
        let on_filter: OnFilter = std::rc::Rc::new(move |filter, _window, cx| {
            filterer.update(cx, |state, cx| state.set_filter(filter, cx));
        });
        let panel = inspector::render(
            InspectorInput {
                inspector,
                message,
                log,
                people: state.people(),
                is_open: &|disclosure, by_default| state.is_open(disclosure, by_default),
            },
            InspectorActions {
                on_disclose,
                on_close,
                on_filter,
            },
        );
        Some(
            card()
                .id("inspector-card")
                .flex_none()
                .w(px(inspector::WIDTH))
                .overflow_hidden()
                .child(panel)
                .into_any_element(),
        )
    }

    fn segment(
        &self,
        segment: Segment,
        active: Segment,
        label: &'static str,
        selector: &'static str,
    ) -> impl IntoElement {
        let state = self.state.clone();
        control::segment(selector, segment == active)
            .px(px(12.))
            .py(px(4.))
            .rounded(px(7.))
            .text_size(px(12.5))
            .line_height(phi())
            .styles(|styles| {
                styles.pressed(|style| {
                    style.shadow(vec![
                        BoxShadow::new(px(0.), px(1.), theme::shadow()).blur_radius(px(2.)),
                    ])
                })
            })
            .on_change(move |_pressed, _event, _window, cx| {
                state.update(cx, |state, cx| state.activate_segment(segment, cx));
            })
            .child(label)
    }

    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        button("sidebar-toggle")
            .accessibility_label("Toggle the sidebar")
            .p(px(5.))
            .rounded(px(7.))
            .hover(|style| style.bg(theme::sunken()))
            .on_click(cx.listener(|shell, _event, _window, cx| {
                shell.state.update(cx, |state, cx| state.toggle_sidebar(cx));
            }))
            .child(icon(Glyph::Sidebar, px(16.), theme::text_secondary()))
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.read(cx).active_segment();
        let sidebar = self.state.read(cx).sidebar();
        let lights_end = GUTTER + CONTENT_INSET + TRAFFIC_LIGHTS_WIDTH + TRAFFIC_LIGHTS_GAP;
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(TOOLBAR_HEIGHT + HAIRLINE))
            .pr(px(GUTTER + CONTENT_INSET))
            .border_b_1()
            .border_color(theme::hairline())
            .child({
                let controls = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .pl(px(lights_end))
                    .text_color(theme::text_secondary())
                    .child(self.sidebar_toggle(cx));
                match sidebar {
                    SidebarVisibility::Shown => controls.w(px(GUTTER + SIDEBAR_WIDTH + GUTTER)),
                    SidebarVisibility::Hidden => controls.pr(px(TRAFFIC_LIGHTS_GAP)),
                }
            })
            .child(
                control::segments("segments")
                    .gap(px(3.))
                    .p(px(3.))
                    .rounded(px(9.))
                    .bg(theme::sunken())
                    .child(self.segment(Segment::Channel, active, "Channel", "segment-channel"))
                    .child(self.segment(Segment::Agents, active, "Agents", "segment-agents"))
                    .child(self.segment(
                        Segment::Automations,
                        active,
                        "Automations",
                        "segment-automations",
                    )),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("link-status")
                            .debug_selector(|| "link-status".to_string())
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(theme::text_muted())
                            .child(
                                div()
                                    .w(px(6.))
                                    .h(px(6.))
                                    .rounded_full()
                                    .bg(link_tone(self.state.read(cx).link())),
                            )
                            .child(link_label(self.state.read(cx))),
                    )
                    .child(settings_affordance()),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_settings(window, cx);
        let body = self.body(cx);
        let mut columns = div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .gap(px(GUTTER))
            .p(px(GUTTER));
        match self.state.read(cx).sidebar() {
            SidebarVisibility::Shown => {
                columns = columns.child(
                    div()
                        .w(px(SIDEBAR_WIDTH))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .min_h(px(0.))
                        .child(self.sidebar.clone()),
                );
            }
            SidebarVisibility::Hidden => {}
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::window())
            .text_color(theme::text_primary())
            .text_size(px(13.5))
            .on_action(cx.listener(|shell, _: &ToggleSidebar, _window, cx| {
                shell.state.update(cx, |state, cx| state.toggle_sidebar(cx));
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|shell, event: &MouseDownEvent, window, cx| {
                    let text = TextSelection::selected_text(window, cx);
                    let text = text.trim();
                    if text.is_empty() {
                        return;
                    }
                    shell.copy_menu = Some(CopyMenu {
                        position: event.position,
                        text: text.to_string(),
                    });
                    cx.notify();
                }),
            )
            .child(self.top_bar(cx))
            .child(columns.children(body))
            .children(viewer::viewer(&self.state, &self.viewer_focus, window, cx))
            .children(self.copy_menu(cx))
    }
}

impl Shell {
    fn copy_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let CopyMenu { position, text } = self.copy_menu.clone()?;
        let menu = div()
            .id("copy-menu")
            .debug_selector(|| "copy-menu".to_string())
            .p(px(4.))
            .w(px(140.))
            .rounded(px(8.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::border())
            .shadow(vec![BoxShadow {
                color: theme::shadow(),
                offset: point(px(0.), px(4.)),
                blur_radius: px(12.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|shell, _event, _window, cx| {
                shell.copy_menu = None;
                cx.notify();
            }))
            .child(
                control::row_button("copy-menu-copy")
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(5.))
                    .text_size(px(12.5))
                    .hover(|style| style.bg(theme::sunken()))
                    .on_click(cx.listener(move |shell, _event, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        shell.copy_menu = None;
                        cx.notify();
                    }))
                    .child("Copy"),
            );
        Some(deferred(anchored().position(position).snap_to_window().child(menu)).with_priority(2))
    }
}

fn link_tone(link: &Link) -> Hsla {
    match link {
        Link::Connecting => theme::text_muted(),
        Link::Live => theme::status_idle(),
        Link::Reconnecting => theme::status_busy(),
        Link::Failed(_) => theme::accent(),
    }
}

fn link_label(state: &AppState) -> SharedString {
    let source = match state.source() {
        Source::Mock => "mock",
        Source::Snapshot => "snapshot",
        Source::Daemon(_) => "daemon",
    };
    let link = match state.link() {
        Link::Connecting => "connecting".to_string(),
        Link::Live => "live".to_string(),
        Link::Reconnecting => "reconnecting".to_string(),
        Link::Failed(reason) => format!("offline: {reason}"),
    };
    SharedString::from(format!("tuclaw · {source} · {link}"))
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .bg(theme::card())
        .rounded(px(12.))
        .border_1()
        .border_color(theme::border())
        .shadow(vec![
            BoxShadow::new(px(0.), px(8.), theme::shadow())
                .blur_radius(px(24.))
                .spread_radius(px(-10.)),
        ])
}

fn settings_affordance() -> impl IntoElement {
    div()
        .p(px(6.))
        .rounded(px(8.))
        .child(icon(Glyph::Adjust, px(15.), theme::text_secondary()))
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

    use crate::state::SidebarVisibility;

    use super::{
        CONTENT_INSET, GUTTER, HAIRLINE, Shell, TOOLBAR_HEIGHT, TRAFFIC_LIGHT_SIZE, ToggleSidebar,
        link_label, link_tone, traffic_light_position,
    };
    use crate::state::{AppState, Segment};
    use crate::testing::loaded;

    fn shell(cx: &mut TestAppContext) -> (Entity<AppState>, &mut VisualTestContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |window, cx| Shell::new(built, window, cx));
        (state, cx)
    }

    #[gpui::test]
    fn right_clicking_a_selection_offers_copy(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let built = state.clone();
        let (_root, cx) = cx.add_window_view(move |window, cx| {
            let shell = gpui::AppContext::new(cx, |cx| Shell::new(built, window, cx));
            gpui_kit::base::Root::new(shell, window, cx)
        });
        cx.run_until_parked();
        let raw = state.read_with(cx, |state, _cx| {
            let Some(last) = state.messages().last() else {
                panic!("the first channel has history");
            };
            let tuclaw_core::model::MessageId(raw) = last.id;
            raw
        });
        let selector: &'static str = Box::leak(format!("message-{raw}-md").into_boxed_str());
        let text = cx
            .debug_bounds(selector)
            .expect("the last message text is drawn");
        let start = point(text.left() + px(1.), text.top() + px(8.));
        let end = point(text.right() - px(1.), text.top() + px(8.));
        cx.simulate_mouse_down(start, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(start, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("copy-menu").is_none(),
            "no selection, no menu"
        );
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_down(end, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let copy = cx
            .debug_bounds("copy-menu-copy")
            .expect("a selection offers copy");
        cx.simulate_click(copy.center(), Modifiers::default());
        cx.run_until_parked();
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert!(copied.is_some_and(|text| !text.is_empty()));
        assert!(cx.debug_bounds("copy-menu").is_none());
    }

    #[gpui::test]
    fn drawing_the_shell_does_not_panic(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Channel)
        });
    }

    #[gpui::test]
    fn clicking_the_segments_moves_the_active_one(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        let agents = cx
            .debug_bounds("segment-agents")
            .expect("the agents segment is drawn");
        cx.simulate_click(agents.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Agents)
        });
        let channel = cx
            .debug_bounds("segment-channel")
            .expect("the channel segment is drawn");
        cx.simulate_click(channel.center(), Modifiers::default());
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.active_segment(), Segment::Channel)
        });
    }

    #[gpui::test]
    fn the_agents_view_replaces_the_feed(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        assert!(
            cx.debug_bounds("input-feed").is_some(),
            "the conversation view draws its composer"
        );
        let agents = cx
            .debug_bounds("segment-agents")
            .expect("the agents segment is drawn");
        cx.simulate_click(agents.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("input-feed").is_none(),
            "the agents view takes the feed's place"
        );
        let channel = cx
            .debug_bounds("segment-channel")
            .expect("the channel segment is drawn");
        cx.simulate_click(channel.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("input-feed").is_some(),
            "the feed comes back with the conversation view"
        );
    }

    #[gpui::test]
    fn there_is_no_direct_segment(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        assert!(cx.debug_bounds("segment-channel").is_some());
        assert!(cx.debug_bounds("segment-agents").is_some());
        assert!(cx.debug_bounds("segment-direct").is_none());
    }

    #[gpui::test]
    fn the_toolbar_names_the_source_and_the_link(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        assert!(cx.debug_bounds("link-status").is_some());
        let label = state.read_with(cx, |state, _cx| link_label(state));
        assert_eq!(label.as_ref(), "tuclaw · mock · live");
    }

    #[gpui::test]
    fn the_segments_start_at_the_content_card(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        let segments = cx.debug_bounds("segments").expect("the segments are drawn");
        let card = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        assert_eq!(segments.left(), card.left());
    }

    #[gpui::test]
    fn the_segments_sit_in_the_middle_of_the_toolbar(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        let segments = cx.debug_bounds("segments").expect("the segments are drawn");
        assert_eq!(segments.center().y, px(TOOLBAR_HEIGHT / 2.));
    }

    #[gpui::test]
    fn the_content_card_and_the_search_field_share_a_top_edge(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        let card = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        let search = cx
            .debug_bounds("sidebar-search")
            .expect("the search field is drawn");
        assert_eq!(card.top(), search.top());
        assert_eq!(card.top(), px(TOOLBAR_HEIGHT + HAIRLINE + GUTTER));
    }

    #[test]
    fn the_traffic_lights_align_with_the_sidebar_and_the_toolbar() {
        let position = traffic_light_position();
        assert_eq!(position.x, px(GUTTER + CONTENT_INSET));
        assert_eq!(
            position.y + px(TRAFFIC_LIGHT_SIZE / 2.),
            px(TOOLBAR_HEIGHT / 2.)
        );
    }

    #[gpui::test]
    fn the_agents_view_card_starts_where_the_conversation_card_does(cx: &mut TestAppContext) {
        let (_state, cx) = shell(cx);
        let conversation = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        let agents = cx
            .debug_bounds("segment-agents")
            .expect("the agents segment is drawn");
        cx.simulate_click(agents.center(), Modifiers::default());
        cx.run_until_parked();
        let roster = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        assert_eq!(roster.origin, conversation.origin);
    }

    #[gpui::test]
    fn the_sidebar_toggle_hides_and_shows_the_sidebar(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        let toggle = cx
            .debug_bounds("sidebar-toggle")
            .expect("the toggle is drawn");
        cx.simulate_click(toggle.center(), Modifiers::default());
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.sidebar(), SidebarVisibility::Hidden)
        });
        assert!(cx.debug_bounds("sidebar-search").is_none());
        let card = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        assert_eq!(card.left(), px(GUTTER));
        let toggle = cx
            .debug_bounds("sidebar-toggle")
            .expect("the toggle is drawn");
        cx.simulate_click(toggle.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(cx.debug_bounds("sidebar-search").is_some());
        let card = cx
            .debug_bounds("content-card")
            .expect("the content card is drawn");
        let segments = cx.debug_bounds("segments").expect("the segments are drawn");
        assert_eq!(segments.left(), card.left());
    }

    #[gpui::test]
    fn with_the_sidebar_hidden_the_segments_follow_the_toolbar_controls(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        state.update(cx, |state, cx| state.toggle_sidebar(cx));
        cx.run_until_parked();
        let toggle = cx
            .debug_bounds("sidebar-toggle")
            .expect("the toggle is drawn");
        let segments = cx.debug_bounds("segments").expect("the segments are drawn");
        assert!(toggle.left() >= px(GUTTER + CONTENT_INSET + 60.));
        assert!(segments.left() > toggle.right());
    }

    #[gpui::test]
    fn the_toggle_sidebar_action_flips_the_sidebar(cx: &mut TestAppContext) {
        let (state, cx) = shell(cx);
        cx.dispatch_action(ToggleSidebar);
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.sidebar(), SidebarVisibility::Hidden)
        });
        cx.dispatch_action(ToggleSidebar);
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.sidebar(), SidebarVisibility::Shown)
        });
    }

    #[test]
    fn every_link_state_has_its_own_tone() {
        use crate::state::Link;
        use crate::theme;
        assert_eq!(link_tone(&Link::Live), theme::status_idle());
        assert_eq!(link_tone(&Link::Reconnecting), theme::status_busy());
        assert_eq!(link_tone(&Link::Failed("x".into())), theme::accent());
        assert_eq!(link_tone(&Link::Connecting), theme::text_muted());
    }
}
