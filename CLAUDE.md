# CLAUDE.md

Rules for working in this repository. They are load-bearing: each one was settled the hard way, and
breaking one is how this app regresses. Read `README.md` for what the app is and for the toolchain
traps (the `mise run` rule above all — never run a bare `cargo`).

## The crate split

Two crates in one workspace.

`core/` is `tuclaw-core`: the domain types the views render (`model.rs`), day grouping (`grouping.rs`) and the client of the daemon's `/api/v3` (`v3/`). The local SQLite store and its fixtures were removed on 2026-10-04: the app runs on the v3 client only.

`core/src/v3/` is the client of the daemon's `/api/v3`, built to `docs/contracts/v3-client-contract.md` (a verbatim copy of tuclaw's contract; the two copies stay identical apart from the header):

- `dto.rs` - the REST bodies, field for field, with `Unknown` fallbacks on every enum
- `frames.rs` - `Frame` and `decode` for the socket, `ClientFrame` and `encode`
- `run.rs` - the `Run` reducer that folds a run's frames
- `transport.rs` - the `Transport` seam, `Connection`, `ApiError`, `Backoff`
- `runtime.rs` - the core-owned one-worker tokio runtime and `spawn` with abort-on-drop
- `http.rs` - `HttpTransport` (reqwest + tokio-tungstenite) and `ClientToken`
- `mock.rs` - `MockTransport`, the in-process daemon with `Pace::{Realtime, Stepped}`
- `client.rs` - `Client`, every call of the contract
- `golden.rs` - the golden-fixture loader for tests (`core/testdata/v3/`)

`core/src/testing.rs` (feature `test-support`) is `FakeDaemon`, a loopback HTTP and WebSocket server for transport tests.

`app/` is `tuclaw-desktop`: a library (`src/lib.rs`, every module and `run()`) plus a three-line binary (`src/main.rs` calls `tuclaw_desktop::run()`), split so `app/examples/` can reach the views. `mise run snapshot [channel]` (`app/examples/snapshot.rs`) is how the agent side looks at the UI without opening a window on Pavel's screen: it builds `AppState` over the mock (or `TUCLAW_MOCK_WORLD`), opens the real `Shell` in GPUI's `HeadlessAppContext` with the macOS text system and the headless Metal renderer (`gpui_platform::current_headless_renderer`, `test-support` dev feature), and writes `target/snapshots/<channel>.png` at 2x. It runs as an example, not a test, because the macOS platform must be created on the main thread. For a real end-to-end check (allowed since 2026-10-04) `mise run preview` opens the bundled app on the live daemon; `swift script/window-id.swift` prints its CGWindowID for `screencapture -x -o -l<id> out.png`, and `swift script/click.swift <x> <y>` clicks at window-relative points (pixels in the capture are 2x points). Compare a view with its design screenshot this way before calling it done. The library is — `state.rs`, the views (`shell.rs`, `sidebar.rs`, `feed.rs`,
`message.rs`, `agents.rs`, `failure.rs`), the link between v3 and the views (`link.rs`), the live run card (`live.rs`), the run log under every answer (`runlog.rs`: design option 1a from the Claude Design file `Agent Runs.dc.html` - a muted summary line `> 3 tools · 13 s` / `Failed · …` / `Stopped by you · …` from `run_summary`, nothing for a toolless answer, whose byline gets the duration instead; clicking it fetches `GET /runs/{id}` once (`AppState::toggle(Disclosure::Log)`, cached in `run_logs`) and opens the log in place: thoughts muted with a `Thinking` label, tools as short name (the `mcp__…__` prefix dropped) + main argument + duration + ✓/✕, consecutive calls of one tool grouped as `Bash ×5`, background tasks, status lines, each tool opening to Input / Result (clipped to 12 lines, Show full), context and tokens in the footer; the last text step equal to the answer is dropped, a failed run opens by default and so does a failed tool, and an answer with a run shows no separate Thinking fold; the live run card (`live.rs`) renders its steps through the same `runlog` rows (`live_rows` from the `Run` reducer, `live_steps`), so a running and a finished run look alike, and background-task events merge into one row per `task_id` that follows its state; disclosure keys carry an `Owner`, a message or a live run; only an `answer` or a `notice` ends a live run - a `post` or `a2a` message carrying its run id leaves it running - and only an `answer` carries the run summary line, a bare `[SILENT]` answer drawing none of its text; the expanded log ends in `Open in panel`, which opens the Run inspector (`inspector.rs`, design option 1c, a 400px card right of the feed mounted by `Shell::inspector_card`: status, steps/tools/time, context and tokens, failed-call count, All / Tools / Thoughts / Errors filters (`AppState::set_filter`), every step with its `+m:ss` offset from the run start and the same expandable rows as the inline log, which share its disclosures; `Disclosure::Inspect` opens it, `close_inspector` or a channel switch closes it)), voice playback (`audio.rs`: `decode` turns an Ogg Opus recording into PCM with `opus-pure` and an m4a, mp3 or wav one with rodio's symphonia decoders, and `Speaker` is the output seam, `RodioSpeaker` in the app and `testing::FakeSpeaker` in tests), Markdown (`rich.rs`: GPUI Kit's `TextView` in the app's colours, and the split of a leading `<details>` Thinking fold the prod data still carries), the composer
(`composer.rs`), the theme (`theme.rs`) and the app menu (`menu.rs`: About with the version and
the commit `build.rs` bakes in, Quit on Cmd+Q). Menu action handlers that open a prompt go through
`cx.defer`: an action dispatched while a window is active runs inside that window's update, so a
second `window.update` from the handler fails silently.

**`core` must never depend on `gpui`.** Not directly, not transitively. Two reasons: the domain and
the client have to be testable with no window and no GPU, and transport concerns must stay out of the
render path. `cargo tree -p tuclaw-core` must not mention `gpui`. If a core type seems to need a
`gpui` type, the conversion belongs on the app side.

Public items in `core` carry `///` docs (`rustdoc` skill); `app` items do not.

**`failure.rs` is not only a view: it owns the startup path.** `start(Config) -> Startup` picks the source from `link::Config` - `TUCLAW_MOCK_WORLD=<path>` the snapshot world, else `TUCLAW_MOCK=1` the built-in `MockTransport` in real time, else the live daemon at `TUCLAW_DAEMON_URL`, defaulting to bravo (`link::DEFAULT_DAEMON_URL`), with `TUCLAW_CLIENT_TOKEN` as an optional bearer (`/api/v3` is open on the LAN unless the daemon sets its token; `hello.capabilities.auth` says which). The default must stay the live daemon: an app opened through LaunchServices (`mise run preview`/`install`, Finder) gets none of these variables, so anything mock-only by default never reaches Pavel's bundle. `Info.plist` carries `NSLocalNetworkUsageDescription` for the Local Network prompt that reaching bravo needs - and builds `AppState`, returning `Ready(Box<AppState>)` or `Failed(FailureView)` (a URL that is not `http://`); `main` opens one window with either as its root and then calls `AppState::start`, which spawns the link task. `Ready` boxes its payload or clippy's `large_enum_variant` fails the `-D warnings` gate.

**`link.rs` is the seam between v3 and the views.** It maps surfaces onto `Channel`, agents onto `Agent` (busy while a live run of theirs is tracked), messages onto `Message` (`Author::System` for notices, the text as one `Span::Text`; the first `voice` attachment becomes `Message::voice` and drops the `[Voice message...]` header line from the transcript), and picks the source from the environment. Views never see a v3 type except the `Run`s of `AppState::live_runs`.

**The link task** (`run_link` in `state.rs`) is the contract's fresh start: connect without `since`, read `hello`, then fetch surfaces, agents and the selected surface's page, then apply every frame in order through `AppState::apply`. A closed socket sets `Link::Reconnecting`, waits `Backoff::next_delay` on `cx.background_executor().timer` (tests `advance_clock` through it), and reconnects with the last seq; a `gap` refetches. Selecting a surface sends `focus` and loads its page. Older history pages in on demand: `AppState::history()` (`Unknown`/`More`/`Loading`/`Complete`, from the page's `has_more`) gates `load_older`, which fetches the 50 messages before the oldest stored id (`Client::messages_before`, `?before=`); the feed's list scroll handler calls it when a user scroll brings an item within `PREFETCH` (3) of the top, and on `OlderLoaded` the feed rebuilds its items and scrolls back to the message that was at the top (or the first message after a top separator), so prepending never moves what the user is reading. Posting appends an optimistic row with a negative local id and a `ClientMessageId`, reconciled by the `202` and by the echoed `message.created`; a failed post removes the row and emits `SendFailed(text)`, which the feed hands back to the composer.

**GPUI Kit is initialised before anything renders.** `main` and `testing::{mocked, seeded}` call `gpui_kit::init(cx)`, which installs the Kit theme and state `TextView` reads; a test that builds a view rendering Markdown without it panics on the missing global.

## State ownership

One `AppState` entity owns all mutable application state: the v3 client and the link status, the surfaces and agents, the selected channel and its messages, the live runs and the queued placeholders, the optimistic posts, which view is showing, whether the sidebar is shown, and the voice playback (`toggle_voice` fetches the recording with `Client::attachment`, decodes it on the background executor, hands it to the injected `Speaker` and polls its position every 200 ms until it finishes; one recording plays at a time, a second press or a channel switch stops it; `message.rs` draws it as the mockup's voice card stretched to the message column like any other message (the mockup's 540px cap was dropped on Pavel's call): play button, a waveform painted on a `canvas` with as many 3px bars at 2.5px gaps as the width holds (64 peaks grouped when narrower, linearly interpolated when wider), the duration, and the transcript inside the card; the bars come from the recording's measured peaks). Peaks and the duration are computed client-side (agreed with the daemon side: v3.1 carries no waveform, and the backfill left `duration_ms` null): after a page or a new message arrives, `fill_waveforms` fetches and decodes each voice recording without a `Waveform`, one at a time on the background executor; `audio::waveform` gives 64 absolute levels, 0-255 (peak amplitude per bucket times 255) and the decoded length. They are kept in memory and, in the app, in `~/Library/Caches/tuclaw-desktop/waveforms/<source>/<attachment id>.peaks` (64 raw bytes, the pinned shape a server-side waveform in step D must match) plus `<attachment id>.ms` (the duration in milliseconds; peaks without it are recomputed). The card shows `duration_ms` when the daemon has it, else the measured duration. Until a recording's peaks exist the card draws a stable per-recording placeholder pattern.

- Views hold `Entity<AppState>` and read through it. **No view mutates another view's data, and no
  view talks to the client directly.**
- Every mutation is a method on `AppState` that ends in `cx.notify()` and, where a listener has to do
  more than repaint, `cx.emit(...)`.
- `active_segment()` is derived, not stored. Anything derivable stays derived — the feed header's
  `N agents`, the status bar's `N of M agents busy`, the sidebar's sections.

**Controls and icons come from GPUI Kit, never from glyphs or drawn divs.** Icons are lucide SVGs: `icon.rs` embeds only the ones in use (`icon_assets!` -> `Icons`, registered with `with_assets` in `run()` and passed to the headless context in the snapshot example) behind the app's own `Glyph` enum, with `icon(glyph, size, color)` and the Kit `Spinner` (`spinner`) for pending states; a new icon is a `Glyph` variant, its `IconName` and an `icon_assets!` entry, and `every_glyph_is_embedded` fails when the entry is missing. Clickable elements are `gpui_kit::base` controls built in `control.rs`: `button`/`row_button` (a `Button` with the debug selector, a pointer and `focusable(false)`, so a click never pulls the caret out of the composer; give icon-only ones an `accessibility_label`), `segments`/`segment` (`ToggleGroup` + `Toggle`) for the Channel/Agents switch and the inspector filters, `avatar(initials, color, AvatarSize)` (`Avatar` with a fallback, ready for bot photos). Base controls center their content and set a line height of 1, so a row-shaped one starts from `row_button` (start-aligned, default line height). Unstyled base controls are used rather than `gpui_kit::component` ones because the app has its own design; only `Spinner` and `Icon` come from the component layer.

**Avatars are pictures the daemon owns, drawn when loaded and as initials otherwise.** `GET /agents` carries a versioned `avatar_url` per agent and `GET /me` the user's `{name, avatar_url}` (a daemon without `/me` leaves "You" with initials); `AppState::fill_pictures` fetches every picture once by its URL (`Client::avatar`, under the same token), decodes it by its leading bytes (`people::decode`) into the `Gallery`, and views reach agents, the user and the gallery through one `People` value (`AppState::people()`), whose `picture` yields only a loaded image, so a missing or failed picture is silently initials. Writes are `Client::set_avatar`/`clear_avatar`/`update_me` over `Transport::send` (`PUT`/`DELETE`/`PATCH`, an image body with its MIME type); the mock serves, replaces and clears them (it seeds pictures for Jarvis and the user). The user's own profile (v3.4) is the sidebar gear: `AppState::open_profile` puts `profile_panel.rs` in the same right slot as agent settings (`Shell`'s `SlotPanel::{Agent, Me}`), with avatar upload/remove, the name (saved on blur or Enter, never empty) and "About you" (`me.description`, 280 characters, saved on blur) through `PATCH /me` (`MePatch`). `mise run snapshot <channel> --profile` renders it.

**Agents are configured from the desktop (v3.3), replacing the Telegram mini-app.** Clicking an agent's avatar in the feed or on the Agents page opens a card (`card.rs`, a GPUI Kit `Popover`: profile, busy line, Message (disabled until direct messages exist), Mention (`AppState::mention` emits `StateEvent::Mention`, the feed inserts `@ident ` into the composer), Settings, View run); an Agents row also has a gear. Settings live in the right slot the run inspector uses (`settings_panel.rs`, 384px, `Shell::sync_settings` keeps one `SettingsPanel` per opened agent): `AppState::open_settings` remembers the inspector it replaced so "‹ Run" (`back_to_run`) restores it, and opening a run clears settings. The panel saves every field as it changes - description on blur, model on blur or Enter (empty or Default = `ModelChange::Default`), Hears via a Kit `Switch`, Make Lead and Remove from a `···` popover and Add to topic from another - through `Client::update_agent`, `set_wiring` and `remove_wiring`, and shows Saving/Saved/Not saved; a rejected field keeps its text with the daemon's reason below it. Make Lead and Remove leave a six-second Undo toast (`agent_settings::Undo`): undoing Make Lead first gives the lead back to the demoted agent and only then turns this one back into a mention, in that order, since the daemon refuses to demote a topic's lead. An agent's topics, home topic and joinable topics are derived from `GET /surfaces` (`agent_settings::topics_of`, `joinable`). Avatar upload asks for a file (`prompt_for_paths`) and refetches `/agents`. `mise run snapshot <channel> --settings=<agent name>` renders the panel.

**The one thing `AppState` does not own is text being typed.** The composer owns a GPUI Kit `TextareaState` (`gpui_kit::base::input`: selection, clipboard, undo, IME and the context menu come with it; it auto-grows from 1 to 10 rows). Never hand-roll a text input again: the first one lacked selection and paste and was replaced on 2026-10-04. The composer hands the body to `AppState` on submit and clears the field **only if the state reports `Ok`**; since a post completes later, a post that fails after that comes back as `StateEvent::SendFailed` and `Composer::restore` puts the text back with the caret at its end. The placeholder names the *selected* channel, so the feed calls `Composer::set_placeholder` from its `SelectionChanged` arm rather than rebuilding the composer, which would drop focus mid-session. Both need a `&mut Window`, so the feed subscribes to the state with `subscribe_in` and `Feed::new`/`Shell::new` take the window.

## Observe at construction

**Every view that holds `Entity<AppState>` registers `cx.observe(&state, |_, _, cx| cx.notify())` in
its constructor**, and keeps the returned `Subscription` in a field. That is what makes a plain
`notify()` on the state repaint the whole app. A view that forgets it goes stale silently — nothing
fails, the pixels just stop updating.

`cx.subscribe(&state, ...)` is for the cases where a repaint is not enough. Who listens to what:

| event | listener | reaction |
|---|---|---|
| `SelectionChanged` | feed | rebuild items, `ListState::reset(count)`, push the new placeholder into the composer |
| `MessagesLoaded` | feed | rebuild, `reset(count)`, then `scroll_to_end()` |
| `MessageAppended` | feed | rebuild, `reset(count)`, then `scroll_to_end()` |
| `RunsChanged` | feed | rebuild items; `remeasure_items` over the run rows when the count is unchanged, else `reset(count)` |
| `FoldToggled` | feed | `ListState::remeasure()` - a Thinking fold opened or closed |
| `SendFailed(text)` | feed | `Composer::restore(text)` |

Every `match` on `StateEvent` lists all six variants, including the empty arms. No `_ =>`.

## The ListState resync rule

`ListState::new(item_count, alignment, overdraw)` **stores the item count inside the state**. Nothing
but `reset(new_count)` or `splice(range, count)` changes it — `cx.notify()` does not, and the
`list()` render closure is driven by the stored count, not by the app's data.

So whenever the flattened item sequence changes length — a channel switch, a sent message — the
owning view must call `reset(count)` **before** any `scroll_to_end()`, or the list indexes past the
end of the new data and panics. `Feed::resync` is the single place this happens; go through it.

The feed's items are an `Rc<Vec<Item>>` rebuilt from `AppState`, because the `list()` closure is
`FnMut(usize, &mut Window, &mut App)` and never sees the view. Day separators are flattened into the
same sequence as messages, so the count is `messages + separators`.

The feed uses `list()` with `ListState`, not `uniform_list`: message rows wrap to different heights.

## Focus

Focus moves are explicit: the feed focuses its composer (`Composer::focus_handle`, the textarea's `Focusable` handle) when it opens, through a `Focus { Requested, Taken }` field consumed during render, because a focus call needs a `&mut Window` that a subscription callback does not have.

## Two settled behaviours

- **No threads and no direct messages until step D.** v3.0 has neither, so the thread panel, the Reply pill and the Direct segment were removed on 2026-10-04; `ChannelKind::Direct` and the sidebar's direct rows stay in the model for when the API brings them.
- **Enter sends, Shift+Enter breaks the line.** The textarea runs with `submit_on_enter`; its plain `Enter` action propagates, and the composer's wrapper `div` (selector `input-feed`) handles it with `on_action` so the key is consumed (an unhandled propagated Enter would insert a newline). `shift-enter` is propagated back to the textarea, which inserts the break.

**Automations (v3.4) are a third top segment and marks in the feed.** `View::Automations` (`automations_view.rs`) lists `GET /tasks?status=all` grouped Active / Paused / Recently finished: prompt, schedule (`automation::schedule_text`), agent, topic, next and last fire tinted by `last_outcome`, Pause/Resume and a two-step cancel; a selected row expands to the full prompt, the check, the window and `GET /tasks/{id}/runs` (newest first). The client only controls tasks, it never creates or edits them. In a channel, every page's `automations` and each live `task.fired` (`AppState::fires`, merged by `merge_fires`) are interleaved with the messages by `feed::items`: a `ran` mark whose `message_id` is a loaded message becomes a tag in that answer's byline (`automation::trigger_tag`), every other mark is its own muted row (`automation::fire_row`), and consecutive `skipped` marks of one task collapse into one row ("skipped N× since …"). Both open the task in the Automations view. `mise run snapshot <channel> --automations` or `--task=<id>` renders it.

**Voice messages are recorded in the composer (v3.5).** `recorder.rs` is the seam: `Recorder { start, finish -> Take{kind, bytes}, cancel }`, `AvRecorder` on macOS (AVFAudio's `AVAudioRecorder` through `objc2-avf-audio`, AAC in m4a, mono 44.1 kHz, `recordForDuration` capped at `recorder::LIMIT` = 10 minutes, a temp file read and deleted on finish), `NoRecorder` elsewhere and by default, `testing::FakeRecorder` in tests. `AppState::recording()` is `Idle | Live{since, channel} | Sending | Failed(reason)`: Talk or ⌥Space (`composer::ToggleTalk`, bound in `composer::bind_keys`) starts a take and the second press posts it, Cancel discards it. `finish_recording` sends `POST /surfaces/{id}/voice` (`Client::post_voice`, a raw body with its MIME type and `X-Client-Message-Id`, a 180 s timeout because the daemon answers only after transcribing); there is no optimistic row, the message arrives through `message.created`. The composer shows the timer, then "Transcribing…", or the failure with a dismiss. The bundle's `Info.plist` carries `NSMicrophoneUsageDescription`, without which macOS kills the app on first microphone access. `mise run snapshot <channel> --recording` renders the recording state.

## The v3 client

**One async seam.** `Transport` has four calls (`get`, `post`, `fetch` for raw bytes, `connect`), each returning a `BoxFuture<'static, _>`; `HttpTransport` and `MockTransport` implement it and `Client` owns every path and every decode, so the mock and the daemon go through the same code. `HttpTransport` runs its I/O on a one-worker tokio runtime `core` owns (zed's `reqwest_client` pattern), so its futures need no runtime in the caller, start when the call is made, and abort when dropped. `core` therefore depends on tokio but never on `gpui` or `gpui_tokio`.

**One socket task.** The event socket is served by one tokio task that `select!`s over the socket, the client's frames and a heartbeat deadline; it closes the socket on silence, on a server close, on any error, or when the `Connection` is dropped, and the closing of `Connection::frames` is how a caller learns to reconnect (with `Backoff` and `Some(last_seq)`).

**The reducer's text model.** `Run::segment` is the text block in progress only: `text.delta` appends, `step.text` turns it into a text step and clears it, `run.reset` drops it and the text steps. A `run.snapshot` replaces segment and steps and seeds `last_seq` from `as_of_seq`, so persisted frames at or below it are replays. The ordering facts a caller handles (a delta before its `run.started`, an `input.accepted` after it, the answer's `message.created` before `run.finished`, no answer for an interrupted run) are on `run.rs`'s module doc; `core/tests/v3_mock.rs`'s `Session` is the reference caller.

**The test rule.** Tests drive `MockTransport` with `Pace::Stepped` and call `step()`/`play_all()` from the test thread. Never `Pace::Realtime` or `HttpTransport` under `#[gpui::test]`: GPUI's test scheduler forbids parking on a wake from a foreign thread, and both wake from the tokio runtime.

## The design is the spec

`docs/design/mockup.html` is the designer's original and `docs/design/screenshots/` holds five
renders of it. **Open the relevant screenshot before touching a view.**
`docs/design/README.md` states which parts are in scope and which are not.

The full list of non-goals lives in `docs/plans/completed/20260826-tuclaw-desktop-v1.md`, the plan
this repository was built from. The short version (networking and the voice player have since landed): no other attachments or
pickers, no structured message cards, no working search, no message editing or reactions, no dark
mode, and no local time — grouping and the
clock both run at UTC, because `time`'s `now_local` needs `local-offset` and is unsound in a threaded
process. Several controls are drawn and inert on purpose — the sidebar toggle, the back/forward
arrows, the feed header's two trailing chips, the composer's icon row and `Talk` chip. Their
inertness is a decision, not a bug; do not wire them up without being asked.

`theme.rs` holds every colour. **No colour literal appears anywhere else.** The palette grows one
tone at a time as views need them, because an unused `pub` colour is dead code under the
`-D warnings` gate. Tones are `pub fn name() -> Hsla`, not consts — `rgb`/`rgba` are not `const fn`
at the pinned revision.

## Code style

From the `rust-style` skill. Not suggestions:

- `for` loops with mutable accumulators, not `.iter().filter().map().collect()` chains.
- `let ... else` for early returns; the happy path stays unindented.
- Shadow variables through transformations; no `raw_` / `parsed_` / `trimmed_` prefixes.
- **No comments.** No inline explanations, no trailing comments, no section dividers, no TODOs, no
  commented-out code. The only exceptions are `///` doc comments on `tuclaw-core`'s public items and
  `//!` module docs.
- Newtypes over bare `String` where the string carries meaning; enums over `bool` parameters.
- Never a wildcard `_ =>` match; match every variant. Avoid `matches!`.

## Tests

Tests are a required deliverable of every change, not an afterthought.

`core` is plain `#[test]`, against golden JSON, `FakeDaemon` and the mock. `app` is `#[gpui::test]`
with `TestAppContext` / `VisualTestContext`, every state built by `testing::loaded` (or `mocked` with a `Scenario`) over `MockTransport` in `Pace::Stepped`: `testing::play` pumps the client's frames and plays the queue, then `run_until_parked`.

Click paths are testable in-process: `InteractiveElement::debug_selector(|| "name".into())` on a
`div()` records its laid-out bounds under `test-support`, `VisualTestContext::debug_bounds("name")`
returns them, and `simulate_click(bounds.center(), Modifiers::default())` clicks it. The convention is
`"<view>-<thing>-<key>"` — `sidebar-row-General`, `segment-agents`, `message-<id>`, `link-status`.
Selectors are test-only strings and never appear in rendered output.

**Any `#[gpui::test]` that simulates keys runs `cx.update(gpui_kit::init)` first** (the `testing` helpers do): GPUI Kit registers the input key bindings there, and without them `enter`, `cmd-v` and `cmd-a` do nothing. Test-only accessors that reach across module privacy, such as `Composer::text`, are `#[cfg(test)]`-gated, because an accessor with no caller is dead code under the `-D warnings` gate.

A draw test proves only that rendering did not panic. It says nothing about what was drawn, so it is
never the only test for a behaviour.

All four gates green before any change is done: `mise run fmt-check`, `mise run lint`,
`mise run test`, `mise run build`.
