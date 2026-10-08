use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, Pixels, Point, SharedString,
    Subscription, Task, div, linear_color_stop, linear_gradient, prelude::*, px,
};
use tuclaw_desktop::chrome::Hold;
use tuclaw_desktop::icon::{Glyph, icon};
use tuclaw_desktop::state::{AppState, Recording};
use tuclaw_desktop::theme;

use crate::frame;

const LOCK: Pixels = px(70.);
const CANCEL: Pixels = px(110.);
const TAP: Duration = Duration::from_millis(350);
const SAMPLE: Duration = Duration::from_millis(100);
const BARS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    Holding { since: Instant, drag: Point<Pixels> },
    Locked,
}

pub struct Talk {
    state: Entity<AppState>,
    phase: Phase,
    levels: VecDeque<f32>,
    _sampler: Option<Task<()>>,
    _observation: Subscription,
}

impl Talk {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Talk {
        let observation = cx.observe(&state, |talk, state, cx| {
            let live = match state.read(cx).recording() {
                Recording::Live {
                    since: _,
                    channel: _,
                } => true,
                Recording::Idle => false,
                Recording::Sending => false,
                Recording::Failed(_) => false,
            };
            if !live && talk.phase != Phase::Idle {
                talk.stop(cx);
            }
        });
        Talk {
            state,
            phase: Phase::Idle,
            levels: VecDeque::new(),
            _sampler: None,
            _observation: observation,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn hold(&mut self, hold: Hold, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match (hold, self.phase) {
            (Hold::Pressed, Phase::Idle) => self.press(now, cx),
            (Hold::Pressed, Phase::Holding { since: _, drag: _ }) => {}
            (Hold::Pressed, Phase::Locked) => {}
            (Hold::Moved(drag), Phase::Holding { since, drag: _ }) => {
                if drag.y < -LOCK {
                    self.phase = Phase::Locked;
                } else if drag.x < -CANCEL {
                    self.state
                        .update(cx, |state, cx| state.cancel_recording(cx));
                    self.stop(cx);
                } else {
                    self.phase = Phase::Holding { since, drag };
                }
            }
            (Hold::Moved(_), Phase::Idle) => {}
            (Hold::Moved(_), Phase::Locked) => {}
            (Hold::Released, Phase::Holding { since, drag: _ }) => {
                if now.saturating_duration_since(since) < TAP {
                    self.phase = Phase::Locked;
                } else {
                    self.state
                        .update(cx, |state, cx| state.finish_recording(cx));
                    self.stop(cx);
                }
            }
            (Hold::Released, Phase::Idle) => {}
            (Hold::Released, Phase::Locked) => {}
        }
        cx.notify();
    }

    fn press(&mut self, now: Instant, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.start_recording(cx));
        let live = match self.state.read(cx).recording() {
            Recording::Live {
                since: _,
                channel: _,
            } => true,
            Recording::Idle => false,
            Recording::Sending => false,
            Recording::Failed(_) => false,
        };
        if !live {
            return;
        }
        self.phase = Phase::Holding {
            since: now,
            drag: Point::default(),
        };
        self.levels.clear();
        self._sampler = Some(cx.spawn(async move |talk, cx| {
            loop {
                cx.background_executor().timer(SAMPLE).await;
                let Ok(()) = talk.update(cx, |talk, cx| talk.sample(cx)) else {
                    return;
                };
            }
        }));
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let level = self.state.read(cx).recording_level().unwrap_or(0.);
        self.levels.push_back(level);
        while self.levels.len() > BARS {
            self.levels.pop_front();
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Idle;
        self._sampler = None;
        self.levels.clear();
        cx.notify();
    }

    pub fn overlay(&self, cx: &App) -> Option<AnyElement> {
        let Phase::Holding { since, drag } = self.phase else {
            return None;
        };
        let Recording::Live {
            since: _,
            channel: _,
        } = self.state.read(cx).recording()
        else {
            return None;
        };
        let elapsed = cx
            .background_executor()
            .now()
            .saturating_duration_since(since);
        let insets = frame::insets();
        let cancelling = (-drag.x / CANCEL).clamp(0., 1.);
        let mut bars = div()
            .flex()
            .items_center()
            .gap(px(3.))
            .h(px(34.))
            .mb(px(10.));
        for index in 0..BARS {
            let offset = BARS.saturating_sub(self.levels.len());
            let level = match index.checked_sub(offset) {
                Some(at) => self.levels.get(at).copied().unwrap_or(0.),
                None => 0.,
            };
            bars = bars.child(
                div()
                    .flex_1()
                    .h(px(4. + level * 30.))
                    .rounded(px(2.))
                    .bg(theme::accent()),
            );
        }
        let card = div()
            .id("talk-card")
            .debug_selector(|| "talk-card".to_string())
            .w_full()
            .rounded(px(20.))
            .px(px(16.))
            .py(px(14.))
            .bg(theme::raised())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .mb(px(10.))
                    .child(div().size(px(8.)).rounded_full().bg(theme::accent()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child("Listening"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child(SharedString::from(clock(elapsed))),
                    ),
            )
            .child(bars)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pt(px(11.))
                    .border_t_1()
                    .border_color(theme::hairline())
                    .text_size(px(12.5))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_color(theme::text_secondary())
                            .child(icon(Glyph::Up, px(13.), theme::text_secondary()))
                            .child("Slide up to lock"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(if cancelling > 0.5 {
                                theme::accent()
                            } else {
                                theme::text_label()
                            })
                            .child("Slide left to cancel"),
                    ),
            );
        let mic = div()
            .absolute()
            .right(px(8.))
            .bottom(insets.bottom - px(21.))
            .flex()
            .items_center()
            .justify_center()
            .size(px(96.))
            .rounded_full()
            .bg(theme::accent_halo())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(68.))
                    .rounded_full()
                    .bg(theme::accent())
                    .child(icon(Glyph::Voice, px(28.), theme::chip_text())),
            );
        let strip = div()
            .relative()
            .flex_none()
            .h(px(54.) + insets.bottom)
            .px(px(16.))
            .pt(px(17.))
            .bg(theme::card())
            .border_t_1()
            .border_color(theme::hairline())
            .text_size(px(13.))
            .text_color(theme::text_secondary())
            .child("Release to send")
            .child(mic);
        Some(
            div()
                .id("talk-overlay")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .justify_end()
                        .px(px(16.))
                        .pb(px(14.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(theme::scrim_clear(), 0.3),
                            linear_color_stop(theme::scrim_deep(), 1.),
                        ))
                        .child(card),
                )
                .child(strip)
                .into_any_element(),
        )
    }
}

fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
