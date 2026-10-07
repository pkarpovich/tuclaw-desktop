use std::cell::Cell;
use std::rc::Rc;

use gpui::{App, Entity};
use tuclaw_core::model::{ChannelId, MessageId};

use crate::state::{AppState, StateEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub channel: ChannelId,
    pub message: MessageId,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Foreground,
    Background,
}

pub trait Notifier {
    fn badge(&self, count: usize);
    fn alert(&self, alert: &Alert, presence: Presence);
}

pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn badge(&self, _count: usize) {}

    fn alert(&self, _alert: &Alert, _presence: Presence) {}
}

pub fn system() -> Rc<dyn Notifier> {
    #[cfg(target_os = "macos")]
    return Rc::new(mac::MacNotifier::new());
    #[cfg(not(target_os = "macos"))]
    return Rc::new(NoNotifier);
}

pub fn attach(state: &Entity<AppState>, notifier: Rc<dyn Notifier>, cx: &mut App) {
    let shown = Rc::new(Cell::new(None));
    let count = state.read(cx).badge_count();
    show_badge(&*notifier, &shown, count);
    let badge = notifier.clone();
    cx.observe(state, move |state, cx| {
        let count = state.read(cx).badge_count();
        show_badge(&*badge, &shown, count);
    })
    .detach();
    cx.subscribe(state, move |state, event, cx| {
        let StateEvent::Alert(alert) = event else {
            return;
        };
        let presence = if state.read(cx).is_window_active() {
            Presence::Foreground
        } else {
            Presence::Background
        };
        notifier.alert(alert, presence);
    })
    .detach();
}

fn show_badge(notifier: &dyn Notifier, shown: &Cell<Option<usize>>, count: usize) {
    if shown.get() == Some(count) {
        return;
    }
    shown.set(Some(count));
    notifier.badge(count);
}

#[cfg(target_os = "macos")]
mod mac {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_app_kit::{NSApplication, NSSound};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationSound, UNUserNotificationCenter,
    };
    use tuclaw_core::model::{ChannelId, MessageId};

    use super::{Alert, Notifier, Presence};

    const SOUND: &str = "Tink";

    pub struct MacNotifier {
        center: Option<Retained<UNUserNotificationCenter>>,
    }

    impl MacNotifier {
        pub fn new() -> MacNotifier {
            if NSBundle::mainBundle().bundleIdentifier().is_none() {
                return MacNotifier { center: None };
            }
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let answered = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert
                    | UNAuthorizationOptions::Sound
                    | UNAuthorizationOptions::Badge,
                &answered,
            );
            MacNotifier {
                center: Some(center),
            }
        }

        fn chime(&self) {
            let Some(sound) = NSSound::soundNamed(&NSString::from_str(SOUND)) else {
                return;
            };
            sound.play();
        }

        fn banner(&self, alert: &Alert) -> bool {
            let Some(center) = &self.center else {
                return false;
            };
            let Alert {
                channel,
                message,
                title,
                body,
            } = alert;
            let ChannelId(channel) = channel;
            let MessageId(message) = message;
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(title));
            content.setBody(&NSString::from_str(body));
            content.setThreadIdentifier(&NSString::from_str(&format!("channel-{channel}")));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&format!("message-{message}")),
                &content,
                None,
            );
            center.addNotificationRequest_withCompletionHandler(&request, None);
            true
        }
    }

    impl Notifier for MacNotifier {
        fn badge(&self, count: usize) {
            let Some(main) = MainThreadMarker::new() else {
                return;
            };
            let tile = NSApplication::sharedApplication(main).dockTile();
            if count == 0 {
                tile.setBadgeLabel(None);
                return;
            }
            tile.setBadgeLabel(Some(&NSString::from_str(&count.to_string())));
        }

        fn alert(&self, alert: &Alert, presence: Presence) {
            match presence {
                Presence::Foreground => self.chime(),
                Presence::Background => {
                    if !self.banner(alert) {
                        self.chime();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use gpui::{Entity, TestAppContext};
    use tuclaw_core::v3::{AgentId, MockTransport, SurfaceId};

    use super::{Alert, Notifier, Presence, attach};
    use crate::state::AppState;
    use crate::testing::{channel_named, loaded};

    #[derive(Default)]
    struct Heard {
        badges: Vec<usize>,
        alerts: Vec<(Alert, Presence)>,
    }

    #[derive(Clone, Default)]
    struct FakeNotifier(Rc<RefCell<Heard>>);

    impl Notifier for FakeNotifier {
        fn badge(&self, count: usize) {
            self.0.borrow_mut().badges.push(count);
        }

        fn alert(&self, alert: &Alert, presence: Presence) {
            self.0.borrow_mut().alerts.push((alert.clone(), presence));
        }
    }

    fn listening(cx: &mut TestAppContext) -> (MockTransport, Entity<AppState>, FakeNotifier) {
        let (mock, state) = loaded(cx);
        let notifier = FakeNotifier::default();
        let attached = Rc::new(notifier.clone());
        cx.update(|cx| attach(&state, attached, cx));
        (mock, state, notifier)
    }

    fn deliver(mock: &MockTransport, cx: &mut TestAppContext) {
        while mock.step() {}
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_post_on_another_channel_chimes_and_raises_the_badge(cx: &mut TestAppContext) {
        let (mock, state, notifier) = listening(cx);
        let before = state.read_with(cx, |state, _cx| state.badge_count());
        mock.agent_posts(
            SurfaceId(3),
            AgentId(2),
            "The living room\n\nlights are off.",
        );
        deliver(&mock, cx);
        let heard = notifier.0.borrow();
        assert_eq!(heard.alerts.len(), 1);
        let (alert, presence) = &heard.alerts[0];
        assert_eq!(alert.title, "#Smart Home · Home");
        assert_eq!(alert.body, "The living room lights are off.");
        assert_eq!(*presence, Presence::Foreground);
        assert_eq!(heard.badges.last(), Some(&(before + 1)));
    }

    #[gpui::test]
    fn an_inactive_window_gets_a_banner(cx: &mut TestAppContext) {
        let (mock, state, notifier) = listening(cx);
        state.update(cx, |state, cx| state.set_window_active(false, cx));
        mock.agent_posts(SurfaceId(1), AgentId(1), "Done.");
        deliver(&mock, cx);
        let heard = notifier.0.borrow();
        assert_eq!(heard.alerts.len(), 1);
        assert_eq!(heard.alerts[0].1, Presence::Background);
    }

    #[gpui::test]
    fn a_message_already_on_screen_stays_quiet(cx: &mut TestAppContext) {
        let (mock, state, notifier) = listening(cx);
        let general = channel_named(&state, cx, "General");
        state.update(cx, |state, cx| state.select(general, cx));
        cx.run_until_parked();
        mock.agent_posts(SurfaceId(1), AgentId(1), "Done.");
        deliver(&mock, cx);
        assert!(notifier.0.borrow().alerts.is_empty());
        state.update(cx, |state, _cx| state.set_following(false));
        mock.agent_posts(SurfaceId(1), AgentId(1), "One more thing.");
        deliver(&mock, cx);
        assert_eq!(notifier.0.borrow().alerts.len(), 1);
    }

    #[gpui::test]
    fn messages_missed_while_offline_only_count(cx: &mut TestAppContext) {
        let (mock, state, notifier) = listening(cx);
        let before = state.read_with(cx, |state, _cx| state.badge_count());
        mock.disconnect_all();
        cx.run_until_parked();
        mock.agent_posts(SurfaceId(3), AgentId(2), "Lights off.");
        mock.agent_posts(SurfaceId(3), AgentId(2), "Doors locked.");
        mock.play_all();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        let heard = notifier.0.borrow();
        assert!(heard.alerts.is_empty());
        assert_eq!(heard.badges.last(), Some(&(before + 2)));
    }

    #[gpui::test]
    fn the_badge_is_set_only_when_the_count_changes(cx: &mut TestAppContext) {
        let (_mock, state, notifier) = listening(cx);
        let home = channel_named(&state, cx, "Smart Home");
        state.update(cx, |_state, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(notifier.0.borrow().badges.len(), 1);
        state.update(cx, |state, cx| state.mark_unread(home, cx));
        cx.run_until_parked();
        let heard = notifier.0.borrow();
        assert_eq!(heard.badges.len(), 2);
    }
}
