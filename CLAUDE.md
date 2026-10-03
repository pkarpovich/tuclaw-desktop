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

`app/` is `tuclaw-desktop`: the binary — `state.rs`, the views (`shell.rs`, `sidebar.rs`, `feed.rs`,
`message.rs`, `agents.rs`, `failure.rs`), the link between v3 and the views (`link.rs`), the live run card (`live.rs`), the text input (`input.rs`), the composer
(`composer.rs`), the theme (`theme.rs`) and the app menu (`menu.rs`: About with the version and
the commit `build.rs` bakes in, Quit on Cmd+Q). Menu action handlers that open a prompt go through
`cx.defer`: an action dispatched while a window is active runs inside that window's update, so a
second `window.update` from the handler fails silently.

**`core` must never depend on `gpui`.** Not directly, not transitively. Two reasons: the domain and
the client have to be testable with no window and no GPU, and transport concerns must stay out of the
render path. `cargo tree -p tuclaw-core` must not mention `gpui`. If a core type seems to need a
`gpui` type, the conversion belongs on the app side.

Public items in `core` carry `///` docs (`rustdoc` skill); `app` items do not.

**`failure.rs` is not only a view: it owns the startup path.** `start(Config) -> Startup` picks the source - the real daemon when `TUCLAW_DAEMON_URL` and `TUCLAW_CLIENT_TOKEN` are set, else the built-in `MockTransport` in real time - and builds `AppState`, returning `Ready(Box<AppState>)` or `Failed(FailureView)` (a URL without a token, or not `http://`); `main` opens one window with either as its root and then calls `AppState::start`, which spawns the link task. `Ready` boxes its payload or clippy's `large_enum_variant` fails the `-D warnings` gate.

**`link.rs` is the seam between v3 and the views.** It maps surfaces onto `Channel`, agents onto `Agent` (busy while a live run of theirs is tracked), messages onto `Message` (`Author::System` for notices, the text as one `Span::Text` until Markdown rendering lands), and picks the source from the environment. Views never see a v3 type except the `Run`s of `AppState::live_runs`.

**The link task** (`run_link` in `state.rs`) is the contract's fresh start: connect without `since`, read `hello`, then fetch surfaces, agents and the selected surface's page, then apply every frame in order through `AppState::apply`. A closed socket sets `Link::Reconnecting`, waits `Backoff::next_delay` on `cx.background_executor().timer` (tests `advance_clock` through it), and reconnects with the last seq; a `gap` refetches. Selecting a surface sends `focus` and loads its page. Posting appends an optimistic row with a negative local id and a `ClientMessageId`, reconciled by the `202` and by the echoed `message.created`; a failed post removes the row and emits `SendFailed(text)`, which the feed hands back to the composer.

## State ownership

One `AppState` entity owns all mutable application state: the v3 client and the link status, the surfaces and agents, the selected channel and its messages, the live runs and the queued placeholders, the optimistic posts, which view is showing, and whether the sidebar is shown.

- Views hold `Entity<AppState>` and read through it. **No view mutates another view's data, and no
  view talks to the client directly.**
- Every mutation is a method on `AppState` that ends in `cx.notify()` and, where a listener has to do
  more than repaint, `cx.emit(...)`.
- `active_segment()` is derived, not stored. Anything derivable stays derived — the feed header's
  `N agents`, the status bar's `N of M agents busy`, the sidebar's sections.

**The one thing `AppState` does not own is text being typed.** The composer creates a `TextInput`
entity that owns its own buffer, because `EntityInputHandler` requires the element to hold the string. The composer hands the
body to `AppState` on submit and clears the input **only if the state reports `Ok`**; since a post completes later, a post that fails after that comes back as `StateEvent::SendFailed` and `Composer::restore` puts the text back. The placeholder names the *selected* channel, so it
cannot be fixed at construction: the feed calls `Composer::set_placeholder` from its
`SelectionChanged` arm rather than rebuilding the composer, which would drop focus mid-session.

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
| `SendFailed(text)` | feed | `Composer::restore(text)` |

Every `match` on `StateEvent` lists all five variants, including the empty arms. No `_ =>`.

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

**Nothing focuses a handle by itself.** Three things have to be in place for a text input to receive
keys:

1. The input's outer `div()` calls `.track_focus(&focus_handle)` and focuses the handle on
   `on_mouse_down`, so clicking the field focuses it.
2. The element's `paint` calls `window.handle_input(&focus_handle, ElementInputHandler::new(bounds,
   entity), cx)`. Implementing `EntityInputHandler` does nothing on its own, and the handler only
   registers while the handle is focused.
3. Focus moves are explicit: the feed focuses its composer when it opens, through a
   `Focus { Requested, Taken }` field consumed during render, because a focus call needs a
   `&mut Window` that a subscription callback does not have.

**Every range crossing `EntityInputHandler` is in UTF-16 code units, not byte offsets.** The element
keeps a `String` and converts at the boundary. ASCII-only tests cannot catch a byte-offset bug, so
input tests use Cyrillic and an emoji.

Multi-line text goes through `shape_text`, never `shape_line` — the latter carries
`debug_assert!(text.find('\n').is_none())` and debug asserts are live under `cargo run` and `cargo
test`.

## Two settled behaviours

- **No threads and no direct messages until step D.** v3.0 has neither, so the thread panel, the Reply pill and the Direct segment were removed on 2026-10-04; `ChannelKind::Direct` and the sidebar's direct rows stay in the model for when the API brings them.
- **Enter sends, Shift+Enter breaks the line.** Both are explicit `KeyBinding`s registered in
  `input::bind_keys`, not defaults.

## The v3 client

**One async seam.** `Transport` has three calls, each returning a `BoxFuture<'static, _>`; `HttpTransport` and `MockTransport` implement it and `Client` owns every path and every decode, so the mock and the daemon go through the same code. `HttpTransport` runs its I/O on a one-worker tokio runtime `core` owns (zed's `reqwest_client` pattern), so its futures need no runtime in the caller, start when the call is made, and abort when dropped. `core` therefore depends on tokio but never on `gpui` or `gpui_tokio`.

**One socket task.** The event socket is served by one tokio task that `select!`s over the socket, the client's frames and a heartbeat deadline; it closes the socket on silence, on a server close, on any error, or when the `Connection` is dropped, and the closing of `Connection::frames` is how a caller learns to reconnect (with `Backoff` and `Some(last_seq)`).

**The reducer's text model.** `Run::segment` is the text block in progress only: `text.delta` appends, `step.text` turns it into a text step and clears it, `run.reset` drops it and the text steps. A `run.snapshot` replaces segment and steps and seeds `last_seq` from `as_of_seq`, so persisted frames at or below it are replays. The ordering facts a caller handles (a delta before its `run.started`, an `input.accepted` after it, the answer's `message.created` before `run.finished`, no answer for an interrupted run) are on `run.rs`'s module doc; `core/tests/v3_mock.rs`'s `Session` is the reference caller.

**The test rule.** Tests drive `MockTransport` with `Pace::Stepped` and call `step()`/`play_all()` from the test thread. Never `Pace::Realtime` or `HttpTransport` under `#[gpui::test]`: GPUI's test scheduler forbids parking on a wake from a foreign thread, and both wake from the tokio runtime.

## The design is the spec

`docs/design/mockup.html` is the designer's original and `docs/design/screenshots/` holds five
renders of it. **Open the relevant screenshot before touching a view.**
`docs/design/README.md` states which parts are in scope and which are not.

The full list of non-goals lives in `docs/plans/completed/20260826-tuclaw-desktop-v1.md`, the plan
this repository was built from. The short version: no networking, no voice, no attachments or
pickers, no structured message cards, no working search, no message editing or reactions, no dark
mode, no selection or mouse caret placement inside the text input (clicking it focuses it, nothing
more), no height cap or internal scrolling on the composer, and no local time — grouping and the
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
Selectors are test-only strings and never appear in rendered output. `TextInput::new` takes its selector as a constructor argument, because it has to land on the same `div()` that owns `track_focus`.

**Any `#[gpui::test]` that simulates keys calls `cx.update(input::bind_keys)` before it builds its
harness.** The bindings live under the `TuclawInput` key context and are registered per `App`;
without them `enter` and `shift-enter` arrive as a literal newline and the test fails as if the logic
were wrong. Test-only accessors that reach across module privacy — `Composer::text` — are `#[cfg(test)]`-gated, because an accessor with no caller in the bin
target is dead code under the `-D warnings` gate.

A draw test proves only that rendering did not panic. It says nothing about what was drawn, so it is
never the only test for a behaviour.

All four gates green before any change is done: `mise run fmt-check`, `mise run lint`,
`mise run test`, `mise run build`.
