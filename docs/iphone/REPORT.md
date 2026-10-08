# tuclaw for iPhone - night report (2026-10-08)

Branch `iphone-app`, on top of the brief (`614517b..`), all pushed, nothing merged. The iPhone app is a new crate `ios/` (tuclaw-ios) hosted by gpui-mobile. It reuses the Mac app's `AppState`, link task, reducer glue and conversation views. The Mac app is unchanged in behaviour, and its four gates are green after every commit: fmt-check, lint, test (core 135, desktop 221, ios 20 plus 5 phone visual scenes, 8 Mac visual scenes), build.

## Outcome

Tonight was a blind comparison: a second session built the same iPhone app natively in SwiftUI from the same brief. Pavel chose the native build. The reason is the framework, not this branch's work. `iphone-app` stays pushed and unmerged as the record of the GPUI attempt.

GPUI-on-iOS gaps this branch ran into:
- **Text input.** gpui-mobile's input view has no autocorrect or predictive text, Russian included, and never implements `show_soft_keyboard`: the app raises and hides the keyboard itself (`ios/src/keyboard.rs`). The phone composer also doesn't grow while you type; only its first line stays visible (seen live, not fixed).
- **Unreleased pins.** gpui-mobile is a git dependency at rev 9075e3a, because crates.io 0.1.0 pins an older gpui-pre. Its `camera` and `video_player` features are forced on by ungated upstream code. libc is pinned to `=0.2.189` for `backtrace` on iOS.
- **Every control hand-drawn.** Sheets, menus, the tab bar and hold-to-talk are GPUI divs. Overlays must `.occlude()`, and gesture listeners must check their own hitbox, or taps fall through.
- **Packaging.** It's a separate bundle, so it can't be the watch app's companion. The Photos picker blocks the main thread under gpui-mobile, so it wasn't done.

## Live verification on bravo (10:15-10:40)

Pavel allowed the build in Little Snitch. Relaunching the existing install, with no reinstall and no tunnel, reached `192.168.199.72:9090`. The earlier attempt at 09:56-10:00 timed out on the event socket while curl got 200; the rule matched only the install's path. Six posts went to #phone-qa (23): 9816, 9818, 9820, 9822, 9824, 9826. There was no live voice recording, since the simulator would use the Mac's microphone.

Verified live:
- **Connection.** REST and the WebSocket (`hello`, then `focus` on opening #phone-qa) work, and Home loads surfaces, groups, avatars, badges and previews.
- **Streaming.** An unaddressed post auto-provisioned a lead (agent 22), and its answer streamed token by token into the live run card, with tool rows, a working state, Stop, and "1 running" in the header.
- **Run log.** "3 tools · 38 s" under the answer opens the log in place: tools, context and tokens, Open in panel.
- **Suggested replies.** The lead called `suggest_replies`, and the chips appeared. Tapping one posted 9818 with `reply_to_message_id=9817`; the daemon marked the option chosen and the other chips greyed out.
- **Reply with the @ picker.** Long press, Reply, then @, meetings: 9822 has `reply_to_message_id=9821` and `addressed_agent_id=18`.
- **Ranges by curl** on `/attachments/796`: 200 with `Accept-Ranges: bytes` and `audio/mp4`, 206 for `bytes=0-99` and `bytes=100-` with correct `Content-Range`, and 416 past the end and for the suffix form, which is by contract.
- **Recording.** `target/ios/acceptance-live.mp4` (575 s) covers opening the app with Home filtered to "phone", streaming, the run log, the chip, and the reply with the @ picker. It stays local, not committed: the repository is public, and Home shows real chats.

Not verified live, **needs Pavel**:
- **Stop.** All six runs ended `ok`. Meetings finished in 15 s, before the tap. On the two 60-second Bash runs, the streamed tool output pushed the card header with Stop off screen almost at once, so the tap never landed. A Stop pinned in view while a run streams is the fix this calls for (not done).
- **Voice post and playback in the app.**
- **Mark read.** Opening #phone-qa read it, but there was no check of the server's cursor.

Differences from the mock:
- **"unknown agent".** An agent created after start, like the auto-provisioned lead, shows as "unknown agent" and is missing from the @ picker: `/agents` is fetched once at start and never refetched. Client fix, not done.
- **Tool durations show "0.0 s".** Tool steps carry no `duration_ms`, and `started_at` equals `finished_at` at second precision. Server backlog.
- **Reply context.** The addressee of a reply (meetings) said the replied-to message hadn't reached it. Server backlog.
- **Keyboard.** A tap on the feed doesn't close the keyboard; only a drag does.

## Deliverables

- `docs/iphone/screenshots/` - iPhone 17 Pro simulator, iOS 26.5, demo world:
  - `01-surfaces.png` - surface list: Running now, groups, both badge kinds, the marked dot
  - `02-streaming-answer.png` - answer streaming: live card, Stop, a running tool, the text cursor; `02b-answer-done.png`
  - `03-run-log-expanded.png` - a run log opened in place; `03b-live-run-card.png` - the live run card
  - `04-mention-picker.png` - composer with the @ picker open
  - `05-voice-message.png` - a voice message playing; `05b-hold-to-talk.png` - the held-mic overlay; `05c-voice-sent.png`
  - `06-agent-settings.png` - agent settings sheet
  - `07-automations.png`, `07b-automation-detail.png` - automations
  - extras: `08-inspector.png` (run inspector), `09-suggested-replies.png`, `10-notification.png` (a notification from the backgrounded app)
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
| | paging back (`before`) | done | the demo world's Night Log has 320 messages; flicking back loads page after page (`?before=`) and the message under your finger stays put |
| | Markdown, code, tables, links | done | GPUI Kit `TextView` |
| | avatars of agents and the user | done | user is round on the phone, agents square |
| | pictures in messages, viewer | partial | viewer mounted, and pictures fit a 300×320 pt room on the phone (unit-tested); no picture post was seen on the simulator, because the demo world has no public picture to fetch |
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
| | avatar upload | partial | `prompt_for_paths` maps to gpui-mobile's document picker (Files, not Photos); untested. gpui-mobile's `image_picker` presents `PHPickerViewController` and then blocks on a channel, which UIKit forbids on the main thread, so Photos needs that package reworked |
| You | name, about | done | the Mac `ProfilePanel` |
| | avatar | partial | same as agent avatars |
| Automations | list, pause, resume, two-step cancel, detail with runs | done | Mac view with a narrow layout |
| | `task.fired` marks in the conversation | done | trigger tags, quiet rows, failure cards are the shared feed's |
| | per-channel automations panel | done | a button in the conversation header (with a dot for unseen failures) opens the Mac panel as a sheet; a task in it, or a trigger tag in the feed, opens it on the Automations tab |
| Channels management | groups, rename, order, archive | done | the Mac `ChannelsView`, pushed from Home's pencil |
| Notifications | local notification while backgrounded | done | an agent's answer that arrives while the app is in the background is posted through `UNUserNotificationCenter` and lands in Notification Center (`10-notification.png`); only while iOS keeps the process alive, see Rough edges |
| | app badge | done | `setBadgeCount`; Home screen showed "7" |
| Agent card "View run" | | done | opens the run's conversation (`StateEvent::RunViewed`) |

**Event stream: WebSocket, not per-surface SSE.** The Mac's link task is the WebSocket, and reusing it gives the phone every surface's frames. Home needs them for badges, previews, "Running now" and alerts on channels that aren't open. Per-surface SSE would need a second client and a stream per surface, or would lose all of that.

**Mockup mapping.** The mockup's tabs Home / Threads / Inbox / Agents became Home / Automations / Agents / You: v3 has no threads or inbox, and Automations and the profile need a home. Direct messages map to nothing; groups render as the mockup's sections. The accent is the Mac's `#b45c3c`, not the mockup's `#b8452c`, so the reused rows and the phone chrome share one accent (agreed with mimi).

## 2. What had to be built or bridged

About 3,500 lines in `ios/` (views, navigation, tests and visual scenes, the host, the demo world generator; the generated `world.json` not counted) and +1,400/-325 lines in `app/` and `core/`.

Bridging to platform APIs, about 450 lines in total:
- **Host** (`ios/xcode/main.m` 77, header 17, XcodeGen spec 54, `script/ios.fish` 76): an Objective-C app delegate drives the Rust static library with a `CADisplayLink`.
- **Entry** (`ios/src/entry.rs` 77): gpui-mobile's `run_app` can't register an `AssetSource`, so ours repeats its steps with `with_assets(Icons)`.
- **Keyboard** (`keyboard.rs` 19, plus hooks in `chrome.rs`): gpui-mobile never implements GPUI's `show_soft_keyboard`. The phone raises the keyboard on a tap into a field and hides it on a list drag or a blur.
- **Gestures** (`chrome.rs`, 127 lines): a long press and a claimed touch drag (hold-to-talk, swipe-back), both hit-tested through a hitbox the way gpui's own tooltip does it.
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

- **Live daemon partly verified** (above: Stop, voice and mark read are not). On a device iOS will ask for Local Network access first. The plist carries `NSLocalNetworkUsageDescription`.
- **Notifications only while the app is alive.**
  - Authorization, the app badge, and a notification for an answer that arrives after you leave the app all work: it lands in Notification Center with the app icon. Live banners were not captured on the simulator.
  - iOS suspends the app seconds after it goes to the background, so anything later needs APNs. The daemon already pushes to devices registered through `POST /api/v3/devices` (contract v3.11). The phone needs its bundle id and push entitlement, and a token registration, which were out of scope tonight.
- **Typing.**
  - gpui-mobile turns autocorrect off and keeps UIKit's text view empty between keystrokes, so the predictive bar never offers words (no ёжик for ежик).
  - Dictation couldn't be tested on the simulator.
  - ё by long-press is untested; my touch tool can't hold and slide.
  - The keyboard snaps instead of animating with the composer.
- **Swipe-back exists but does not slide.** A drag from the left edge goes back past 80 pt, but the screen does not follow the finger. There is no pull-to-refresh and no haptics.
- **"Allow the microphone, then tap again"** stays in the composer after you grant permission, until the next tap. The permission callback runs off the main thread.
- **Playback right after a recording** needs a device check: the session drops back to Playback without re-activating it.
- **No live transcript while holding the mic.** The daemon transcribes after the upload, and the contract has no streaming speech-to-text.
- In the simulator, `axe swipe` (a few sparse touch samples) sometimes produced a reversed or huge fling; real-finger-like drags (`axe drag` with 20 steps) and slow drags page smoothly. Worth a check with a finger on a device.
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

The phone-specific code is mostly navigation and chrome. The test discipline carried over too: the phone has 20 GPUI tests that drive real touches through GPUI's gesture recognizer. Three of them pin bugs that review caught: taps passing through overlays, a reopened channel not being read, and a sheet's tap reaching the mic beneath it.

**What makes it painful.**
- The platform layer is young. gpui-mobile 0.1 is pinned to an unpublished commit, and I hit a run of gaps:
  - no soft-keyboard hook;
  - no asset source in `run_app`;
  - features that don't build when off;
  - autocorrect off by design;
  - fonts that fail to load;
  - taps that zero-length synthetic touches drop.

  Each was cheap to work around, but every one was discovered at runtime, not compile time.
- Nothing occludes by default: a GPUI overlay lets taps through unless it says otherwise, and paint-time gesture listeners ignore occlusion unless they hit-test a hitbox. Both bit tonight, and both are now fixed and tested; any new overlay or gesture has to follow the same rule (CLAUDE.md says so).
- iOS niceties are missing and would have to be hand-built: an interactive swipe-back (there is only a gesture, no slide), scroll-to-top on a status-bar tap, keyboard animation curves, haptics, a real share sheet and a photo picker.
- A Rust static library inside an Xcode shell with `-force_load` is workable but heavy: the debug `.a` is 1.2 GB.
- The simulator loop is slower than the Mac's. `mise run ios-visual` now renders five phone scenes headless at 402×874 with the real Metal renderer and compares them with baselines. Each scene also asserts behaviour, as on the Mac, and runs in `mise run test`. Its 70-line image comparison is copied from `app/tests/visual.rs`; sharing it needs a test-support module both harnesses can reach.

**Verdict.** For one user on a LAN, sharing all the logic with the Mac is a real win, and the app is maintainable here. The cost is owning a thin, growing layer of platform glue and accepting gpui-mobile's maturity: plan on upstreaming the keyboard hook, the asset source and the feature gates. If the phone must feel fully native (swipe-back, system text features, rich notifications), expect that glue to keep growing.

## 5. Build and run on the simulator

Prerequisites: Xcode 27 (DeviceHub must be open for `axe` touches), `xcodegen` (`brew install xcodegen`), and mise. `mise install` fetches Rust 1.98.1 with the iOS targets listed in `mise.toml`.

```
mise run ios-run       # build and launch on "Tuclaw iPhone 17 Pro" (iOS 26.5) against bravo
mise run ios-mock      # the same, on the mock daemon over ios/demo/world.json
mise run ios-build     # build only; --release via `fish script/ios.fish --release`
mise run ios-lint      # clippy for aarch64-apple-ios-sim, warnings as errors
mise run ios-visual    # the phone's headless visual scenes (UPDATE_BASELINE=1 rewrites them)
mise run ios-demo-world  # regenerate the synthetic demo world (deterministic)
```

The simulator is created on first run. The app's stdout and stderr go to `target/ios/console.log`. `ios/xcode/Tuclaw.xcodeproj` is generated (gitignored); open it in Xcode to debug. Before typing into the simulator, read the simulator traps in CLAUDE.md.
