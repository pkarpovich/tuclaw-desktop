use gpui::{
    BoxShadow, Context, Div, Entity, FontWeight, IntoElement, Pixels, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use crate::feed::Feed;
use crate::sidebar::Sidebar;
use crate::state::{AppState, Segment};
use crate::theme;
use crate::thread::ThreadPanel;

pub struct Shell {
    state: Entity<AppState>,
    sidebar: Entity<Sidebar>,
    feed: Entity<Feed>,
    thread: Entity<ThreadPanel>,
    _observation: Subscription,
}

impl Shell {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Shell {
        let observation = cx.observe(&state, |_shell, _state, cx| cx.notify());
        let built = state.clone();
        let sidebar = cx.new(|cx| Sidebar::new(built, cx));
        let built = state.clone();
        let feed = cx.new(|cx| Feed::new(built, cx));
        let built = state.clone();
        let thread = cx.new(|cx| ThreadPanel::new(built, cx));
        Shell {
            state,
            sidebar,
            feed,
            thread,
            _observation: observation,
        }
    }

    fn segment(
        &self,
        segment: Segment,
        active: Segment,
        label: &'static str,
        selector: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chip = div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .flex()
            .items_center()
            .px(px(12.))
            .py(px(4.))
            .rounded(px(7.))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .on_click(cx.listener(move |shell, _event, _window, cx| {
                shell
                    .state
                    .update(cx, |state, cx| state.activate_segment(segment, cx));
            }))
            .child(label);
        if segment == active {
            chip.bg(theme::raised())
                .text_color(theme::text_primary())
                .shadow(vec![
                    BoxShadow::new(px(0.), px(1.), theme::shadow()).blur_radius(px(2.)),
                ])
        } else {
            chip.text_color(theme::text_secondary())
        }
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.read(cx).active_segment();
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(48.))
            .px(px(14.))
            .border_b_1()
            .border_color(theme::hairline())
            .child(div().w(px(52.)).flex_none())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .ml(px(8.))
                    .text_color(theme::text_secondary())
                    .child(sidebar_toggle())
                    .child(arrow("‹", 1.0))
                    .child(arrow("›", 0.4)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .ml(px(6.))
                    .p(px(3.))
                    .rounded(px(9.))
                    .bg(theme::sunken())
                    .child(self.segment(Segment::Channel, active, "Channel", "segment-channel", cx))
                    .child(self.segment(Segment::Direct, active, "Direct", "segment-direct", cx))
                    .child(self.segment(Segment::Agents, active, "Agents", "segment-agents", cx)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme::text_muted())
                            .child("tuclaw · local"),
                    )
                    .child(settings_affordance()),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thread = match self.state.read(cx).thread() {
            Some(_) => Some(
                card()
                    .w(px(360.))
                    .flex_none()
                    .overflow_hidden()
                    .child(self.thread.clone()),
            ),
            None => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::window())
            .text_color(theme::text_primary())
            .text_size(px(13.5))
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap(px(10.))
                    .px(px(10.))
                    .pb(px(10.))
                    .child(
                        div()
                            .w(px(250.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .min_h(px(0.))
                            .child(self.sidebar.clone()),
                    )
                    .child(
                        card()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child(self.feed.clone()),
                    )
                    .children(thread),
            )
    }
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

fn sidebar_toggle() -> impl IntoElement {
    div().p(px(5.)).rounded(px(7.)).child(
        div()
            .w(px(15.))
            .h(px(13.))
            .border_1()
            .rounded(px(3.5))
            .border_color(theme::text_secondary())
            .child(
                div()
                    .w(px(4.5))
                    .h_full()
                    .border_r_1()
                    .border_color(theme::text_secondary()),
            ),
    )
}

fn arrow(glyph: &'static str, opacity: f32) -> impl IntoElement {
    div()
        .p(px(5.))
        .rounded(px(7.))
        .text_size(px(16.))
        .opacity(opacity)
        .child(SharedString::new_static(glyph))
}

fn settings_affordance() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(3.))
        .p(px(6.))
        .rounded(px(8.))
        .child(rule(px(14.)))
        .child(rule(px(9.)))
        .child(rule(px(12.)))
}

fn rule(width: Pixels) -> Div {
    div()
        .w(width)
        .h(px(1.5))
        .rounded(px(1.))
        .bg(theme::text_secondary())
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
    use time::macros::datetime;
    use tuclaw_core::store::Store;

    use super::Shell;
    use crate::state::{AppState, Segment};

    fn seeded(cx: &mut TestAppContext) -> Entity<AppState> {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        let state = AppState::new(store).expect("the workspace loads");
        cx.new(|_| state)
    }

    fn shell(cx: &mut TestAppContext) -> (Entity<AppState>, &mut VisualTestContext) {
        let state = seeded(cx);
        let built = state.clone();
        let (_shell, cx) = cx.add_window_view(move |_window, cx| Shell::new(built, cx));
        (state, cx)
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
}
