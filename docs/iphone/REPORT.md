# tuclaw for iPhone - night report (2026-10-08)

Branch `iphone-app`, 10 commits on top of the brief (`614517b..`), all pushed, nothing merged. The iPhone app is a new crate `ios/` (tuclaw-ios) hosted by gpui-mobile. It reuses the Mac app's `AppState`, link task, reducer glue and conversation views. The Mac app is unchanged in behaviour, and its four gates are green after every commit: fmt-check, lint, test (core 135, desktop 220, ios 14, 8 visual scenes), build.

**Read this first: nothing here was run against bravo.** Little Snitch on this Mac holds the simulator build's traffic to `192.168.199.72:9090`:
- From inside the app, a TCP connect succeeds, but no bytes come back for a plain `GET /api/v3/surfaces` or for the WebSocket upgrade.
- curl inside the same simulator, and curl on the Mac, both get 200/101 immediately.
- Each reinstall puts the app at a new path, so an ad-hoc-signed build triggers a new prompt.

Approving or routing around a firewall prompt is your decision. I declined an ssh-tunnel workaround (`ssh -N -L 127.0.0.1:19090:localhost:9090 pi-bravo` plus `SIMCTL_CHILD_TUCLAW_DAEMON_URL=http://127.0.0.1:19090`) for that reason. Everything below ran on the in-process mock daemon, `MockTransport` in real time, driving the real `AppState` and reducer. No message was posted to any real topic, #phone-qa included.

**Morning actions**
1. In Little Snitch, allow the simulator app `dev.pkarpovich.tuclaw.ios` (or "any process") to reach 192.168.199.72:9090. Or run the tunnel above yourself. Then `mise run ios-run`.
2. Run the acceptance scenario live in #phone-qa (23), and re-check on a device what the simulator can't prove (see Rough edges).

## Deliverables

- `docs/iphone/screenshots/` - iPhone 17 Pro simulator, iOS 26.5, demo world:
  - `01-surfaces.png` - surface list: Running now, groups, both badge kinds, the marked dot
  - `02-streaming-answer.png` - answer streaming: live card, Stop, a running tool, the text cursor; `02b-answer-done.png`
  - `03-run-log-expanded.png` - a run log opened in place; `03b-live-run-card.png` - the live run card
  - `04-mention-picker.png` - composer with the @ picker open
  - `05-voice-message.png` - a voice message playing; `05b-hold-to-talk.png` - the held-mic overlay; `05c-voice-sent.png`
  - `06-agent-settings.png` - agent settings sheet
  - `07-automations.png`, `07b-automation-detail.png` - automations
  - extras: `08-inspector.png` (run inspector), `09-suggested-replies.png`
- `docs/iphone/acceptance.mp4` (89 s, half resolution) - the acceptance scenario, with General standing in for #phone-qa, on the mock:
  1. open the app, pick the channel;
  2. send a text and watch the answer stream;
  3. stop a run (Magnet Feed's live run);
  4. hold the mic, release, and watch the voice message arrive with its transcript;
  5. long-press a message, Reply, send with the quote bar;
  6. tap a suggested reply in Movie Night.

## 1. Parity with the Mac app

| Area | Feature | Status | Notes |
|---|---|---|---|
| Sidebar → Home | surfaces with groups and order | done | `link::sidebar_order`, the Mac's code |
| | archived hidden | done | |
| | unread and unread-replies badges | done | `badge::indicator`, the sidebar's five rules extracted and shared |
| | mark unread / read | done | long press a row → sheet |
| | live-run indicator | done | "Running now" card on top (the mockup's) + accent "X is working…" row line |
| | last message preview | done | not in the API: one `messages?limit=1` per surface on load, then kept from `message.created` |
| | search | done | local filter over names (the Mac's search is inert) |
| Conversation | day groups, New divider, fresh-message stripes | done | the Mac `Feed`, phone chrome |
| | paging back (`before`) | partial | shared code, not exercised on the phone (demo history is short) |
| | Markdown, code, tables, links | done | GPUI Kit `TextView` |
| | avatars of agents and the user | done | user is round on the phone, agents square |
| | pictures in messages, viewer | partial | viewer mounted; no picture post was tested; blocks are 480 pt wide at most and can overflow a phone column |
| Live runs | text streaming | done | one WebSocket, `/api/v3/events` (below) |
| | steps in a collapsible run card | done | |
| | run summary on answers, run log | done | `› 1 tool · 13 s`, opens in place, `Open in panel` → full-screen inspector |
| | Stop | done | |
| Composer | text, optimistic row, `client_message_id` | done | shared |
| | voice recording → `POST /surfaces/{id}/voice` | done | hold to talk (the mockup's primary input) and tap for hands-free; `AVAudioRecorder` |
| | @ picker → `addressed_agent_id` | done | @ button or typing `@`; tap to pick. Arrows/Tab work on a hardware keyboard; Return inserts a newline on the phone |
| | Reply with quote bar | done | long-press a message → Reply |
| Voice messages | play the original, transcript | done | cpal → CoreAudio. Fetches the whole file like the Mac, no HTTP ranges |
| Suggested replies | chips, tap endpoint, chosen and closed states | done | |
| Read state | mark read when shown | done | fixed during review: reopening a channel now runs the seen pass |
| Agents | list, card, settings (model, description, wiring) | done | the Mac `SettingsPanel` as a sheet |
| | avatar upload | partial | `prompt_for_paths` maps to gpui-mobile's document picker (Files, not Photos); untested |
| You | name, about | done | the Mac `ProfilePanel` |
| | avatar | partial | same as agent avatars |
| Automations | list, pause, resume, two-step cancel, detail with runs | done | Mac view with a narrow layout |
| | `task.fired` marks in the conversation | done | trigger tags, quiet rows, failure cards are the shared feed's |
| | per-channel automations panel | missing | the Mac's header button has no phone place yet |
| Channels management | groups, rename, order, archive | done | the Mac `ChannelsView`, pushed from Home's pencil |
| Notifications | local banner while backgrounded | partial | see Rough edges |
| | app badge | done | `setBadgeCount`; Home screen showed "7" |
| Agent card "View run" | | partial | it selects the channel but the phone does not navigate to it |

**Event stream: WebSocket, not per-surface SSE.** The Mac's link task is the WebSocket, and reusing it gives the phone every surface's frames. Home needs them for badges, previews, "Running now" and alerts on channels that aren't open. Per-surface SSE would need a second client and a stream per surface, or would lose all of that.

**Mockup mapping.** The mockup's tabs Home / Threads / Inbox / Agents became Home / Automations / Agents / You: v3 has no threads or inbox, and Automations and the profile need a home. Direct messages map to nothing; groups render as the mockup's sections. The accent is the Mac's `#b45c3c`, not the mockup's `#b8452c`, so the reused rows and the phone chrome share one accent (agreed with mimi).

## 2. What had to be built or bridged

About 2,800 lines in `ios/` (views, navigation, tests, the host, the demo world) and +1,300/-275 lines in `app/` and `core/`.

Bridging to platform APIs, about 450 lines in total:
- **Host** (`ios/xcode/main.m` 77, header 17, XcodeGen spec 54, `script/ios.fish` 76): an Objective-C app delegate drives the Rust static library with a `CADisplayLink`.
- **Entry** (`ios/src/entry.rs` 77): gpui-mobile's `run_app` can't register an `AssetSource`, so ours repeats its steps with `with_assets(Icons)`.
- **Keyboard** (`keyboard.rs` 19, plus hooks in `chrome.rs`): gpui-mobile never implements GPUI's `show_soft_keyboard`. The phone raises the keyboard on a tap into a field and hides it on a list drag or a blur.
- **Gestures** (`chrome.rs`, 127 lines): long press (claimed with a hitbox, the way gpui's tooltip does it) and a claimed touch drag for hold-to-talk.
- **Audio** (`app/src/audio_session.rs` 50): `AVAudioSession` categories, the microphone permission, metering.
- **Notifications** (iOS part of `notify.rs`, about 45): `UNUserNotificationCenter`, the badge, a system sound in the foreground. The banner code is now shared with the Mac.
- **Safe areas and keyboard height** (`frame.rs` 33).

App-side changes, each kept behaviour-neutral for the Mac:
- `Chrome::{Desktop, Phone(Touch)}` for the Feed and Composer.
- `AppState::set_feed(Presence)` with the `FeedShown` event, `keep_previews`, `running()`, `recording_level()`.
- `badge::indicator`, `inspector::from_state`, a narrow width for `AutomationsView`, a fixed `ProfilePanel`, `text_fields()` on the panels.
- `testing` exposed under a `test-support` feature.
- Mock seeds that carry groups and read state.

**Dependency risks** (also in CLAUDE.md):
- gpui-mobile is a git dependency at rev `9075e3a`, the unpublished gpui-pre 0.3.7 bump.
- Its `camera` and `video_player` features are forced on, because upstream's `platform_view.rs` uses them ungated.
- libc is pinned `=0.2.189`: 0.2.190 broke `backtrace` on iOS (rust-lang/libc#5601, fix #5606).
- The mono font on iOS is the private `.AppleSystemUIFontMonospaced`. Menlo fails gpui-mobile's CoreText trait check, and GPUI aborts on a family it cannot load.

## 3. Rough edges a user would hit

- **Live daemon unverified** (above). On a device iOS will ask for Local Network access first. The plist carries `NSLocalNetworkUsageDescription`.
- **Notifications are best effort.**
  - What works: authorization, the badge, the alert firing in the background, and SpringBoard accepting the request (it logged `shouldPresentAlert: YES` once).
  - What I couldn't show: a visible banner on the simulator, and Notification Center stayed empty. The likely cause is timing, since the in-process mock answers while the app is still transitioning.
  - Without APNs, iOS suspends the app seconds after it goes to the background, so banners only cover that window. Real background delivery needs APNs (out of scope).
- **Typing.**
  - gpui-mobile turns autocorrect off and keeps UIKit's text view empty between keystrokes, so the predictive bar never offers words (no ёжик for ежик).
  - Dictation couldn't be tested on the simulator.
  - ё by long-press is untested; my touch tool can't hold and slide.
  - The keyboard snaps instead of animating with the composer.
- **No swipe-back gesture.** Back is the chevron only. No pull-to-refresh, no haptics.
- **"Allow the microphone, then tap again"** stays in the composer after you grant permission, until the next tap. The permission callback runs off the main thread.
- **Playback right after a recording** needs a device check: the session drops back to Playback without re-activating it.
- **No live transcript while holding the mic.** The daemon transcribes after the upload, and the contract has no streaming speech-to-text.
- A `?` can wrap alone to the next line (GPUI's line breaker keeps `?` breakable for URLs; the Mac does the same).
- Agent and user avatars can't be uploaded from Photos, only from Files.

**Server wishes** (no server change made): a `last_message` (author and text) on `GET /surfaces`, which would remove N requests per start.

## 4. Assessment: is this codebase good for an iPhone app?

**What makes it good.** The architecture was ready for a second screen before I touched it:
- One `AppState` owns everything.
- Views only read it, and every mutation is a method.
- `tuclaw-core` never depends on gpui.
- The transport is a seam with an in-process mock that runs in real time.

The result: the phone reuses the hardest parts unchanged:
- the link task with reconnect, gaps and sequence numbers;
- the run reducer and live-run placement;
- unread rules, optimistic posts, read cursors;
- the conversation feed with all its list bookkeeping;
- the settings, profile, channel and automation panels.

The phone-specific code is mostly navigation and chrome. The test discipline carried over too: the phone has 14 GPUI tests that drive real touches through GPUI's gesture recognizer, two of them added after review caught real bugs.

**What makes it painful.**
- The platform layer is young. gpui-mobile 0.1 is pinned to an unpublished commit, and I hit a run of gaps:
  - no soft-keyboard hook;
  - no asset source in `run_app`;
  - features that don't build when off;
  - autocorrect off by design;
  - fonts that fail to load;
  - taps that zero-length synthetic touches drop.

  Each was cheap to work around, but every one was discovered at runtime, not compile time.
- Nothing occludes by default: a GPUI overlay lets taps through unless it says otherwise. That was the review's blocker, now fixed and tested.
- iOS niceties are missing and would have to be hand-built: swipe-back, scroll-to-top on a status-bar tap, keyboard animation curves, haptics, a real share sheet and a photo picker.
- A Rust static library inside an Xcode shell with `-force_load` is workable but heavy: the debug `.a` is 1.2 GB.
- The simulator loop is slower than the Mac's headless snapshot loop. A phone snapshot example (headless, 402×874, over the demo world) would be the next tooling investment.

**Verdict.** For one user on a LAN, sharing all the logic with the Mac is a real win, and the app is maintainable here. The cost is owning a thin, growing layer of platform glue and accepting gpui-mobile's maturity: plan on upstreaming the keyboard hook, the asset source and the feature gates. If the phone must feel fully native (swipe-back, system text features, rich notifications), expect that glue to keep growing.

## 5. Build and run on the simulator

Prerequisites: Xcode 27 (DeviceHub must be open for `axe` touches), `xcodegen` (`brew install xcodegen`), and mise. `mise install` fetches Rust 1.98.1 with the iOS targets listed in `mise.toml`.

```
mise run ios-run       # build and launch on "Tuclaw iPhone 17 Pro" (iOS 26.5) against bravo
mise run ios-mock      # the same, on the mock daemon over ios/demo/world.json
mise run ios-build     # build only; --release via `fish script/ios.fish --release`
mise run ios-lint      # clippy for aarch64-apple-ios-sim, warnings as errors
mise run ios-demo-world  # regenerate the synthetic demo world (deterministic)
```

The simulator is created on first run. The app's stdout and stderr go to `target/ios/console.log`. `ios/xcode/Tuclaw.xcodeproj` is generated (gitignored); open it in Xcode to debug. Before typing into the simulator, read the simulator traps in CLAUDE.md.
