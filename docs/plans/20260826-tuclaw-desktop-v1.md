# tuclaw-desktop v1

## Overview

A native macOS client for the tuclaw agent system, written in Rust on GPUI — Zed's GPU-accelerated UI
framework. It is the first step toward replacing Telegram as the interface to a set of AI agents.

This is a from-scratch build in a clean repository. A SwiftUI implementation of the same v1 exists in
`../tuclaw-client` and is **not** a reference to read or match: it was rejected because it reads as a
generic macOS app rather than the design, and because most of its build was spent fighting system
components. Everything this plan needs is written down here.

What v1 does: shows the channels and direct messages of a single local workspace, renders one
conversation grouped by day, lets the user type and send a message, open a thread on any message and
reply in it, and lists the agents. Data lives in SQLite, seeded once from fixtures. There is no
network.

The load-bearing difference from a typical macOS app: **the window draws its own chrome**. The
titlebar is transparent, the traffic lights are positioned inside the app's own top bar, and the feed
and thread are rounded cards floating on a warm background. Nothing here is a system control.

### Non-goals

Out of scope for v1. Do not build these, and do not treat their absence as a defect:

- Networking of any kind. No daemon connection, no sync, no notifications.
- Voice. The `Talk` button in the feed composer is drawn and does nothing.
- Attachments, emoji picker, mentions picker, formatting. The feed composer's icon row — `@`,
  paperclip, smiley, `Aa` — and the thread composer's `@` and microphone are drawn exactly as the
  mockup shows them and none of them does anything.
- Structured message cards — the film picker, the run log, the task progress card, the decision card.
  Messages are text with inline agent mentions and inline code, nothing else.
- Search behaviour. The search field is drawn as an affordance and searches nothing.
- The `Inbox` sidebar row and the `Agent crews` sidebar section.
- Per-channel agent membership and running-task counts: the sidebar's `2 running` pill, the feed
  header's `N tasks running`, the feed header's member-count pill (the chip showing `4` in the
  mockup), and the plain activity dot on a channel row. The domain has no data for any of them. What
  the header does show is derived — see Technical Details.
- The feed header's thread-toggle chip and `···` overflow chip: drawn, inert. No overflow menu.
- A third agent status colour. `AgentStatus` has two variants; status dots are green or amber.
- The agent settings panel (Role / Permissions / How it replies / Voice replies / Disconnect).
- Editing, deleting or reacting to messages.
- The sidebar toggle and the back / forward arrows in the top bar: drawn, inert.
- Multi-window, a preferences window, menu bar work beyond GPUI's defaults.
- Selection inside the text input, and placing the caret with the mouse. Typing, backspace, arrow
  keys, IME composition, and a visible caret are in scope; dragging to select is not. Clicking the
  input **focuses** it — that is not caret placement and is required.
- An upper bound on the composer's height. The field grows with its text; there is no internal
  scrolling and no line cap.
- iOS, `gpui-mobile`, or any second platform.
- Accessibility work beyond what GPUI provides.

### Rejected alternatives

- **SwiftUI.** Rejected after a full implementation: the system components (toolbar item capsules,
  `List` styling, a titlebar that swallows clicks in its own region) fought the design at every step.
- **Electron / Tauri / Flutter.** Rejected by the author: no JavaScript stack, and no non-native
  renderer pretending to be a Mac app.
- **`uniform_list` for the feed.** Rejected: it requires uniform row heights, and message rows wrap to
  different heights. `list()` with `ListState` is the variable-height equivalent.
- **A single-line composer.** Considered, because GPUI's own input example is single-line and it
  would have kept Task 11 small. Rejected: Shift+Enter breaking the line was a settled requirement of
  the previous build, and the cost is known — see the text facts below.
- **A six-line cap with internal scrolling on the composer.** Rejected: it hides a viewport, clipping
  and caret-visibility subsystem behind one clause. The field grows instead.
- **A draft field on `AppState`.** Rejected: the input element must own its buffer anyway (the
  `EntityInputHandler` contract requires it), and two composers on screen at once would fight over one
  field. The state receives a body on submit and nothing else.
- **A marker syntax inside the body string.** Rejected: any in-band marker needs escaping rules three
  cold sessions would have to agree on. The body is stored as JSON and the question does not arise.
- **Verifying clicks only through user checkpoints.** Rejected once it was checked: under
  `test-support` GPUI lets a test address a `div()` by a string selector, so click paths are testable
  in-process. Checkpoints stay for look and feel, not as the only proof a row is wired.
- **One crate for everything.** Rejected: the domain and the store must be testable without a window,
  so they live in a crate that does not depend on `gpui` at all.
- **crates.io for GPUI.** Not available: only a stale `gpui` core is published and `gpui_platform` is
  not published at all. The git pin below is mandatory, not a preference.

## Skills to invoke

Load each skill below with the Skill tool and follow its conventions before implementing any task in
this plan.

- `rust-style` — every `.rs` file in this repository must follow it; it is the source of the
  Code-Quality Rules section below.
- `rustdoc` — for doc comments on the public items of the `tuclaw-core` crate, which has a real public
  API consumed by the app crate.

## Design reference — look at this before building any view

`docs/design/mockup.html` is the designer's original; `docs/design/screenshots/` holds five renders of
it. `docs/design/README.md` states which parts of the mockup are in scope for this version and which
are not, and agrees with the Non-goals above — read it once before the first view task.

Every view task below names the screenshot to open first. Where this plan's prose and a screenshot
disagree about a detail the plan does not pin, follow the screenshot. Where the plan pins it, the plan
wins.

| screenshot | used by |
|---|---|
| `01-full-mockup.png` | Task 7 (shell), Task 12 (composer) |
| `02-sidebar.png` | Task 8 |
| `03-feed-and-thread.png` | Task 9, Task 10, Task 13 |
| `04-direct-message.png` | Task 9 (the direct variant of the feed) |
| `05-agents-and-settings.png` | Task 14 (the card list only) |

## Toolchain

- Rust **1.98.0**, pinned in `mise.toml`. The floor is 1.97: GPUI's main branch uses
  `std::hint::cold_path`, and anything earlier fails to compile `gpui` with `E0658` — 1.93 was tried
  and failed exactly that way. Zed's own `rust-toolchain.toml` pins 1.97.1, but 1.98.0 was verified
  against the pinned revision here and compiles the whole dependency tree clean.
- **The pin does not reach a bare `cargo`.** On the author's machine `which cargo` resolves to a
  1.93 install placed ahead of mise's shims by the global mise config, and a non-interactive session
  runs no directory hook, so `mise.toml` alone changes nothing. Every gate in this plan therefore runs
  through `make`, and the Makefile wraps each cargo call in `mise exec --`. Never run bare `cargo` as
  a gate.
- The GPUI dependencies, written out in full because the URL appears nowhere else:

  ```toml
  gpui = { git = "https://github.com/zed-industries/zed", rev = "fecc3273ed32643c2ea1b04a74c8780e2c9ffaf8" }
  gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "fecc3273ed32643c2ea1b04a74c8780e2c9ffaf8", features = ["font-kit"] }
  ```

  plus `gpui` again under `[dev-dependencies]` with `features = ["test-support"]`. Without
  `font-kit` text lays out but renders no glyphs.
- **The entry point lives in `gpui_platform`, not `gpui`.** At this revision `gpui::Application` has
  no `new()`; the app starts with `gpui_platform::application().run(|cx: &mut App| { ... })`, opens
  its window with `cx.open_window(WindowOptions { window_bounds, titlebar, ..Default::default() },
  |_, cx| cx.new(|cx| Root::new(cx)))`, and calls `cx.activate(true)`. Every published GPUI example
  starts with `Application::new()`, which does not compile here.
- Xcode's **Metal toolchain** must be installed — `gpui_apple` compiles `shaders.metal` in a build
  script. If missing, the build fails with "cannot execute tool 'metal'"; install with
  `xcodebuild -downloadComponent MetalToolchain` (~690 MB).
- `rusqlite = { version = "*", features = ["bundled", "time"] }` — `time` is what makes
  `OffsetDateTime` bind and read; `bundled` alone does not. `serde` + `serde_json` for the message
  body; `time` for timestamps.
- Build times to expect: ~1 minute clean, ~2-3 seconds incremental.

## Context (from discovery)

### GPUI facts established from the pinned source

Written down here so no task has to go reading Zed. Each was checked against the pinned revision.

**Lists.** `list(state: ListState, render_item)` is the variable-height virtualised list, where
`render_item` is `FnMut(usize, &mut Window, &mut App) -> AnyElement`. `ListState::new(item_count,
alignment, overdraw: Pixels)` takes **three** arguments and **stores the item count inside the
state**. Nothing but `ListState::reset(new_count)` or `ListState::splice(range, count)` changes it —
`cx.notify()` does not, and the closure is driven by the stored count, not by the app's data. So
whenever the flattened item sequence changes length (a channel switch, a sent message), the owning
view must call `reset(count)` **before** any `scroll_to_end()`, or the list indexes past the end of
the new data. `ListAlignment::Bottom` is documented as "scrolling from bottom to top, like a chat
log", which is what makes a feed open on its newest row without a scroll call.

**State.** `Context::notify()` re-renders an entity's observers. An entity becomes an observer with
`cx.observe(&other, |this, other, cx| ...)`; **every view that holds `Entity<AppState>` registers
`cx.observe(&state, |_, _, cx| cx.notify())` at construction**, so a plain `notify()` on the state is
enough to repaint every view. Cross-entity signalling that needs more than a repaint — a `ListState`
resync, a focus move — is `impl EventEmitter<E> for T` (a marker trait), `cx.emit(event)` from inside
the emitter, and `cx.subscribe(&entity, |this, emitter, event, cx| ...)` on the listener.

**Text.** `shape_line(text, font_size, runs, force_width)` is single-line by contract: it carries
`debug_assert!(text.find('\n').is_none())`, and `cargo run` and `cargo test` build in debug, so the
assert is live. Multi-line text goes through `shape_text(text, font_size, runs, wrap_width:
Option<Pixels>, line_clamp: Option<usize>) -> Result<SmallVec<[WrappedLine; 1]>>` — note the
`Result`; `Element::prepaint` cannot propagate it, so on `Err` the element logs and lays out as one
empty line for that frame. A multi-line input therefore lays out with `shape_text`, derives its
height from the wrapped line count, and maps caret position and `bounds_for_range` across wrapped
lines. GPUI's own `examples/input.rs` uses `shape_line` and strips newlines on paste — it is a
single-line reference and does not show any of this.

**Text input.** GPUI ships no text field. Four things make one work, and all four are required:

1. The element is a manual `impl Element` — `request_layout`, `prepaint`, `paint` — because it must
   shape text in `prepaint` and draw its own caret with `window.paint_quad(...)` in `paint`. A `div()`
   composition cannot do it.
2. Implementing `EntityInputHandler` does nothing on its own. The element's `paint` must call
   `window.handle_input(&focus_handle, ElementInputHandler::new(bounds, entity), cx)`, and that only
   registers **while the focus handle is focused**. Keyboard events reach the entity through a
   `FocusHandle`, a `key_context(...)` and `KeyBinding::new(...)` bound to actions.
3. **Focus has to be acquired somewhere.** Nothing focuses a handle by itself. The input's outer
   `div()` calls `.track_focus(&focus_handle)` and `.on_mouse_down(MouseButton::Left, ...)` calling
   `focus_handle.focus(window, cx)` — clicking the field focuses it, without placing the caret. The
   window focuses the feed composer when it opens; `open_thread` moves focus to the thread composer
   and `close_thread` returns it to the feed's.
4. `EntityInputHandler` has **eight required methods**: `text_for_range`, `selected_text_range`,
   `marked_text_range`, `unmark_text`, `replace_text_in_range`, `replace_and_mark_text_in_range`,
   `bounds_for_range`, `character_index_for_point`. Four have defaults: `paste`,
   `set_selected_text_range`, `text_length_utf16`, `accepts_text_input`. **Every range crossing the
   trait is in UTF-16 code units**, not byte offsets — the signatures say `UTF16Selection` and
   `range_utf16`. The element keeps a `String` and converts both ways at the boundary. Tests that use
   only ASCII cannot catch a byte-offset implementation; the tests below use Cyrillic and an emoji.

   The marked-text contract, for IME: `replace_and_mark_text_in_range(range, text, new_selection)`
   replaces the marked range if one exists, else the given range, with `text`, and marks the inserted
   text; `marked_text_range` reports that range in UTF-16 or `None`; `unmark_text` clears the mark and
   keeps the text; `replace_text_in_range` on a marked input replaces the marked range and clears the
   mark; `bounds_for_range` returns the on-screen bounds of the caret's line for the given range so
   the candidate window can be positioned.

**Window chrome.** `WindowOptions::titlebar` takes `TitlebarOptions { title, appears_transparent,
traffic_light_position }`. Transparent titlebar plus an explicit traffic-light position is how the app
draws its own top bar.

**Tests.** `#[gpui::test]` provides a `TestAppContext`. `VisualTestContext` adds `draw(...)`,
`simulate_input(...)`, `simulate_keystrokes(...)` and `simulate_click(position, modifiers)`. Under
`test-support`, `App::flush_effects` redraws every dirty window automatically, so a `notify()` inside
a test forces a repaint without an explicit redraw call. **Clicks can be addressed by identifier**:
under `test-support` (and `cfg(test)`) `InteractiveElement::debug_selector(|| "name".into())` on a
`div()` records that element's laid-out bounds, and `VisualTestContext::debug_bounds("name")` returns
them; `simulate_click(bounds.center(), Modifiers::default())` then clicks it. A draw test proves only
that rendering did not panic; it says nothing about what was drawn.

### The test tier

Pure logic tested directly; state transitions tested through `AppState` methods; the input element
tested through `simulate_input` and `simulate_keystrokes` with focus set explicitly; click paths —
sidebar rows, the top-bar segments, the reply affordance, the thread's close control — tested through
`debug_selector` + `debug_bounds` + `simulate_click`; and a draw test per view to catch panics. The
selector convention is `"<view>-<thing>-<key>"`, e.g. `sidebar-row-movie-night`, `segment-agents`,
`message-reply-<id>`, `thread-close`. Selectors are test-only strings and never appear in rendered
output.

User checkpoints remain for what tests cannot judge: whether it looks like the design and feels right.

## Development Approach

- **Testing approach**: regular — implement, then test in the same task. Tests are a required
  deliverable of every task, not an afterthought.
- Complete each task fully before starting the next. All gates green before any `[x]`.
- **Tasks 1-6 build `tuclaw-core` and the state behind an intentionally empty window.** The render
  rule starts at Task 7: from then on every task must leave the app building, running, and showing
  real fixture data.
- **The agent never drives the running app.** It does not launch it in the foreground, click, or type
  into it. Three tasks — 7, 12 and 13 — end with a **user checkpoint**: the agent stops, asks the
  user to run `make run` and follow the named steps, and does not tick the task until the user
  answers. "The window must render" is a user-verified criterion, not an agent-verified one.
- Update this plan as scope changes: `[x]` when done, `➕` for discovered work, `⚠️` for blockers.

## Code-Quality Rules (verify before marking each task complete)

From the `rust-style` skill. These are not suggestions:

- `for` loops with mutable accumulators, not iterator chains (`.iter().filter().map().collect()`).
- `let ... else` for early returns; the happy path stays unindented. `if let` only for a short action
  with no else branch.
- Shadow variables through transformations; no `raw_` / `parsed_` / `trimmed_` prefixes.
- **No comments.** No inline explanations, no trailing comments, no section dividers, no TODOs, no
  commented-out code. The only exceptions: `///` doc comments on public items of `tuclaw-core`, and
  `//!` module docs where a task below asks for one.
- Newtypes over bare `String` where the string carries meaning; enums over `bool` parameters.
- Never a wildcard `_ =>` match; match every variant. Avoid `matches!`.
- Destructure structs and tuples explicitly.

Per-task gate — all four green, run through `make`, never bare `cargo`:

- `make fmt-check`
- `make lint` — `cargo clippy --workspace --all-targets -- -D warnings`
- `make test`
- `make build` — no warnings, dead code included. A file with no consumer fails this gate in a binary
  crate; every `Create:` below is paired with the task that first uses it.

## Testing Strategy

Two tiers, split by what they need to run.

**Tier 1 — `tuclaw-core`, plain `#[test]`.** The crate does not depend on `gpui`, so these run with no
window and no GPU. Covers: domain invariants, the body encoding round trip, day grouping, the SQLite
schema and every store method, seeding idempotence, and the fixture data itself.

Store tests run against an in-memory SQLite database, one per test, so they are order-independent and
parallel-safe.

**Tier 2 — `tuclaw-desktop`, `#[gpui::test]`.** Needs `gpui` with `test-support`. Covers state
transitions through `AppState` methods, the input element through simulated input, click paths
through `debug_bounds`, pure view-model functions, and one draw test per view. What it does NOT do:
assert on pixels, walk a view hierarchy looking for text, or click by guessed coordinates.

Every task states its own tests. A task with no test line is a task that added no logic.

## Progress Tracking

- Mark completed items `[x]` immediately, not in batches.
- New work discovered mid-flight gets a `➕` line in the task it belongs to.
- Blockers get `⚠️` with the reason, and the plan is updated rather than worked around silently.

## Solution Overview

Two crates in one workspace.

`core/` is `tuclaw-core`: domain types, the SQLite store, the fixtures, and day grouping. It has no
`gpui` dependency, which is what makes it testable without a window and what keeps storage concerns out
of the render path.

`app/` is `tuclaw-desktop`: the binary. One `AppState` entity owns all mutable application state —
the channel list, the selected channel and its messages, the open thread with its root message, which
view is showing, and the last channel visited of each kind. Views hold `Entity<AppState>`, observe it
so that `notify()` repaints them, and read through it. Every mutation is a method on `AppState` that
ends in `cx.notify()` and, where a view has to do more than repaint, `cx.emit(...)`. No view mutates
another view's data, and no view talks to the store directly.

The one thing `AppState` does **not** own is text being typed. Each composer creates a `TextInput`
entity that owns its own buffer, as the input-handler contract requires; the composer hands the body
to `AppState` on submit and clears the input **only if the state reports success**.

The window is chromeless: transparent titlebar, traffic lights positioned into the app's own top bar,
a warm background, and the feed and thread as rounded cards with a shadow.

Two behaviours are settled up front because they were decided the hard way already:

- **The thread is independent of the selected channel.** Switching channels leaves an open thread
  open, so `AppState` caches the thread's root message and channel; the panel never reads them from
  the current selection.
- **Enter sends, Shift+Enter breaks the line.** Both are explicit key bindings, not defaults.

## Technical Details

### Domain (`core/src/model.rs`)

```rust
pub struct ChannelId(pub i64);
pub struct MessageId(pub i64);
pub struct AgentId(pub i64);
pub enum ChannelKind { Channel, Direct(AgentId) }
pub enum Author { User, Agent(AgentId) }
pub enum AgentStatus { Idle, Busy(String) }
pub struct Agent { pub id: AgentId, pub name: String, pub initials: String, pub role: String, pub status: AgentStatus, pub sort_index: i64 }
pub struct Channel { pub id: ChannelId, pub name: String, pub group: Option<String>, pub kind: ChannelKind, pub unread: usize, pub sort_index: i64 }
pub struct Message { pub id: MessageId, pub author: Author, pub body: Vec<Span>, pub sent_at: OffsetDateTime, pub reply_count: usize }
pub enum Span { Text(String), Mention(String), Code(String) }
```

`Span` is part of the domain: the mockup renders agent mentions and paths inline inside a paragraph,
and the store must round-trip them. **The body column holds JSON** — `Span` derives `Serialize` and
`Deserialize` with `serde`'s externally tagged form, so a message is `[{"Text":"On it"},
{"Mention":"allspeak"}]`. There is no in-band marker and nothing to escape; a body typed by the user
is `vec![Span::Text(text)]` and comes back as exactly that.

### Store (`core/src/store.rs`)

```rust
pub fn open(path: &Path) -> Result<Store>;
pub fn open_in_memory() -> Result<Store>;
pub fn channels(&self) -> Result<Vec<Channel>>;
pub fn agents(&self) -> Result<Vec<Agent>>;
pub fn messages(&self, channel: ChannelId) -> Result<Vec<Message>>;
pub fn message(&self, id: MessageId) -> Result<Message>;
pub fn thread(&self, root: MessageId) -> Result<Vec<Message>>;
pub fn send(&self, channel: ChannelId, body: &[Span], at: OffsetDateTime) -> Result<Message>;
pub fn reply(&self, root: MessageId, channel: ChannelId, body: &[Span], at: OffsetDateTime) -> Result<Message>;
pub fn seed_if_needed(&self, now: OffsetDateTime) -> Result<()>;
```

`send` and `reply` take spans, never a bare string: the caller decides what the body means. The app
passes `[Span::Text(typed)]`; the fixtures pass real mentions. `at` is a parameter so tests state the
time.

`message(id)` exists because the thread panel needs its root after the user has switched channels,
when the root is in neither `messages(selected)` nor `thread(root)` — `thread` returns replies only.

Schema: `agents`, `channels`, `messages(id, channel_id, thread_root_id NULL, author, agent_id NULL,
body, sent_at, reply_count)`, `seed_marker`. `sent_at` binds as `OffsetDateTime` through rusqlite's
`time` feature.

Two rules the schema exists to enforce:

- The seed marker is written **in the same transaction** as the fixtures. An interrupted first run
  then leaves either everything or nothing — never a marked-but-empty database that the next launch
  refuses to fill.
- `reply()` inserts the reply and increments the root's `reply_count` **in one transaction**. The feed
  reads that number to render the "N replies" affordance; a reply stored without it is invisible from
  the feed.

Ordering is explicit everywhere: messages sort by `sent_at`, then by `id` as a tie-break. Without the
tie-break two messages sharing a timestamp come back in a different order on each run.

Database path: `~/Library/Application Support/tuclaw-desktop/tuclaw.sqlite`, resolved by one function
in `core` so tests and the app share it.

### Fixtures (`core/src/fixtures.rs`)

Seeded once, and sized so the feed is worth scrolling:

- 4 agents: `magnet feed sync` (mf, busy), `allspeak` (as, busy), `media review` (mr, idle),
  `tuclaw general` (tg, idle).
- 10 channels: `movie-night`, `media-archive`, `apartment-reno` grouped under *Movie nights*;
  `smart-home`, `downloads` under *Home*; `personal` ungrouped; and four directs — `tuclaw general`,
  `magnet feed sync` (unread 2), `allspeak`, `media review`.
- `movie-night` carries **58 top-level messages across 15 days**, one of which is a thread root with
  **4 replies**. Every other channel carries its own shorter conversation across at least two days.
  `personal` is seeded **empty**, and is the only empty channel — the empty state has to render
  somewhere.
- Across all channels the fixtures cover **16 distinct days**, so day separators have something to
  separate and both "Today" and "Yesterday" appear.
- At least one `movie-night` message carries a `Span::Mention` and one a `Span::Code`, so the inline
  rendering has data on the first screen.

All timestamps derive from a `now` passed in, never from the wall clock, so "Today" and "Yesterday"
are correct whenever the app is first launched and so tests can state what they mean.

### State (`app/src/state.rs`)

```rust
pub enum View { Conversation, Agents }
pub enum Segment { Channel, Direct, Agents }
pub struct OpenThread { pub root: Message, pub channel: ChannelId, pub replies: Vec<Message> }
pub enum StateEvent { SelectionChanged, MessageAppended, ReplyAppended, ThreadOpened, ThreadClosed }
pub struct AppState { /* store, agents, channels, selected: ChannelId, messages, thread: Option<OpenThread>, view: View, last_channel: Option<ChannelId>, last_direct: Option<ChannelId> */ }
impl EventEmitter<StateEvent> for AppState {}
pub fn select(&mut self, channel: ChannelId, cx: &mut Context<Self>);
pub fn activate_segment(&mut self, segment: Segment, cx: &mut Context<Self>);
pub fn active_segment(&self) -> Segment;
pub fn send(&mut self, body: String, cx: &mut Context<Self>) -> Result<()>;
pub fn open_thread(&mut self, root: MessageId, cx: &mut Context<Self>);
pub fn close_thread(&mut self, cx: &mut Context<Self>);
pub fn reply_in_thread(&mut self, body: String, cx: &mut Context<Self>) -> Result<()>;
```

Rules these methods enforce, each of which gets a test:

- `select` loads the channel's messages, records it as `last_channel` or `last_direct` by kind, sets
  `view = Conversation`, emits `SelectionChanged`, and does not touch `thread`. Selecting a channel
  from the agents view therefore shows the conversation.
- `active_segment` is derived, not stored: `Agents` when `view == Agents`; otherwise `Direct` when
  the selected channel's kind is `Direct`, else `Channel`.
- `activate_segment(Channel)` selects `last_channel`, or the first channel of kind `Channel` when
  there is none; `activate_segment(Direct)` does the same for kind `Direct`; both go through `select`.
  `activate_segment(Agents)` sets `view = Agents` and notifies. Every view repaints through its
  observation; no event is needed for a view switch.
- `send` and `reply_in_thread` trim the body; a blank result reaches no store call, emits nothing,
  and returns `Ok(())`. A store error is logged and returned as `Err`, leaving `messages` and
  `thread` untouched — the composer keeps the text so the user can retry.
- `send` appends the accepted row to `messages` and emits `MessageAppended`.
- `reply_in_thread` appends to `thread.replies`, bumps `thread.root.reply_count` **and** the matching
  row in `messages` when the root is in the selected channel, and emits `ReplyAppended`.
- `open_thread` loads the root via `Store::message` and the replies via `Store::thread`, stores all
  three, and emits `ThreadOpened`; `close_thread` sets `thread` to `None` and emits `ThreadClosed`.

Who listens to what:

| event | listener | reaction |
|---|---|---|
| `SelectionChanged` | feed | `ListState::reset(count)` |
| `MessageAppended` | feed | `reset(count)` then `scroll_to_end()` |
| `ReplyAppended` | feed, thread panel | repaint (counts unchanged; the feed's affordance re-reads `reply_count`) |
| `ThreadOpened` | thread panel | focus its composer's input |
| `ThreadClosed` | feed composer | focus its input |

Everything else — selection highlight, the active segment, panel visibility, the agents view —
repaints through `observe`.

### Derived labels and the feed header

The feed header shows `#` + name + `N agents` where N is the number of distinct agent authors among the
channel's messages — derivable, no domain field. Its trailing controls are exactly two, both drawn
and inert: the thread-toggle chip and the `···` chip. There is no member-count pill. For a `Direct`
channel the header shows the agent's chip, name and role instead of `#` + name. The status bar shows
`N of M agents busy` from `AgentStatus` across all agents.

### Inline spans in a paragraph

GPUI has no inline element inside text: `TextRun` carries a background colour but no padding or
corner radius, so a run cannot draw the mockup's chip, and `flex_wrap()` wraps children, not text
inside a child. A message body therefore renders as one `div().flex().flex_wrap()` whose children are
**one text element per word** of each `Span::Text` and one padded, rounded chip `div()` per
`Span::Mention` or `Span::Code`. Row gap and column gap approximate line spacing and word spacing.
This is the accepted trade-off for v1.

### Text input (`app/src/input.rs`)

```rust
pub struct TextInput { /* text: String, cursor_utf16: usize, marked_utf16: Option<Range<usize>>, focus: FocusHandle, placeholder: SharedString */ }
pub struct Submitted;
pub fn text(&self) -> &str;
pub fn is_blank(&self) -> bool;
pub fn clear(&mut self, cx: &mut Context<Self>);
pub fn focus_handle(&self) -> &FocusHandle;
impl EventEmitter<Submitted> for TextInput {}
impl EntityInputHandler for TextInput { /* the eight required methods */ }
```

Enter emits `Submitted`; Shift+Enter inserts `\n` at the caret and emits nothing. The element that
renders it is a separate manual `Element` that shapes with `shape_text` at the composer's width,
sizes itself to the wrapped line count (minimum one line, no maximum), and paints the caret.

### Composer (`app/src/composer.rs`)

```rust
pub enum ComposerKind { Feed, Thread }
pub struct Composer { /* input: Entity<TextInput>, kind, on_submit */ }
pub fn new(kind: ComposerKind, placeholder: SharedString, on_submit: Box<dyn Fn(String, &mut App) -> Result<()>>, cx: &mut Context<Self>) -> Self;
```

One component, two shapes from the mockup: `Feed` draws the four-icon row, the `Hold ⌥Space to talk`
hint, the `Talk` chip and the 32 px send button; `Thread` draws `@` and a microphone, no hint, no
`Talk`, and a 30 px send button. Both subscribe to their input's `Submitted`, call `on_submit`, and
clear the input only on `Ok`. The send button is disabled while the input is blank.

### Theme (`app/src/theme.rs`)

One module of named colour constants and nothing else — no colour literal anywhere in the view code.
The palette is warm and light, taken from the mockup: a cream window, white cards, a terracotta
accent, three levels of text, one border and one hairline tone. v1 ships a single palette and does not
follow the system appearance; that is a deliberate simplification, not an oversight.

### Window

`WindowOptions` with `titlebar: Some(TitlebarOptions { title: None, appears_transparent: true,
traffic_light_position: Some(...) })` and `window_bounds` set to a centred 1280×820. The app's own
top bar reserves space on the left for the traffic lights and holds the (inert) sidebar toggle, the
(inert) back and forward arrows, the `Channel / Direct / Agents` segmented control, and
`tuclaw · local` with a settings affordance on the right.

## What Goes Where

- **Implementation Steps** — everything inside this repository.
- **Post-Completion** — running the app, judging how it feels, and anything needing a decision.

## Implementation Steps

### Task 1: Workspace skeleton and a green empty test run

**Files:**
- Create: `Cargo.toml`, `mise.toml`, `Makefile`, `rustfmt.toml`
- Create: `core/Cargo.toml`, `core/src/lib.rs`
- Create: `app/Cargo.toml`, `app/src/main.rs`

`.gitignore` already exists and already ignores `target/`; leave it alone.

- [x] create the workspace with members `core` and `app`; `core` must not list `gpui` as a dependency
- [x] pin Rust 1.98.0 in `mise.toml` and add the dependencies exactly as written in Toolchain
- [x] write the `Makefile` with `build`, `run`, `test`, `lint`, `fmt`, `fmt-check` targets, **every
      one wrapping cargo in `mise exec --`**
- [x] confirm `mise exec -- rustc --version` prints 1.98.0 before the first build
- [x] open an empty window from `app/src/main.rs` through `gpui_platform::application()` per the
      Toolchain bullet, so the Metal build step and window creation are exercised; the window is
      expected to be blank until Task 7
- [x] write one placeholder test in each crate and confirm `make test` passes
- [x] run the per-task gate

➕ The GPUI dependency lines from Toolchain are declared once in the root `[workspace.dependencies]`
together with `rusqlite`, `serde`, `serde_json`, `time` and `anyhow`; each crate opts in with
`workspace = true` as the task that needs it arrives, which is what the later `Modify:
core/Cargo.toml` steps do. `app` takes `gpui` + `gpui_platform` now, plus `gpui` with `test-support`
under `[dev-dependencies]`.

➕ `cx.new(...)` needs `gpui::AppContext` in scope at this revision; without the import it fails with
`E0599: no method named 'new' found for &mut App`.

⚠️ The window opening is verified by compilation only. `make build` compiles `gpui_apple`, which is
the Metal shader build step, but the agent does not launch the app (Development Approach) and the
plan defers "the window must render" to the Task 7 user checkpoint.

### Task 2: Domain types and the body encoding

**Files:**
- Create: `core/src/model.rs`
- Modify: `core/src/lib.rs`, `core/Cargo.toml`

- [x] define every type from the Domain section, deriving `Debug`, `Clone`, `PartialEq` and, for the
      id newtypes, `Eq` and `Hash`
- [x] derive `Serialize` and `Deserialize` on `Span` and provide `encode(&[Span]) -> String` and
      `decode(&str) -> Result<Vec<Span>>` over `serde_json`
- [x] write tests: plain text, a mention mid-sentence, inline code, several spans in one body, an
      empty body, text containing `[`, `{`, `"` and a backslash — all survive `encode` then `decode`;
      malformed JSON is an error, not a panic
- [x] run the per-task gate

➕ The id newtypes also derive `Copy`: they are passed by value through the store and state contracts
(`messages(channel: ChannelId)`, `selected: ChannelId`) and a clone at every call site buys nothing.
`ChannelKind` and `Author` derive `Copy` and `Eq` for the same reason, since `AgentId` is `Copy`.

➕ `encode` panics rather than returning a `Result`, as its signature in Technical Details requires:
`Vec<Span>` holds only strings, so `serde_json::to_string` has no reachable failure. Documented under
`# Panics`.

➕ Two tests beyond the list: the encoded form is asserted to be serde's externally tagged shape
(`[{"Text":"on it "},{"Mention":"allspeak"}]`), pinning the on-disk format the store will read, and a
Cyrillic + emoji body round-trips.

⚠️ Task 17's comment check, `grep -rnE '(^|[^:"])//($|[^/!])' core/src app/src`, also matches `///`
doc comments (it hits the second and third slash of the triple). `core` is required to carry `///`
docs on its public items, so that grep will not print nothing. Task 17 must exclude `///` and `//!`
lines instead of expecting empty output.

### Task 3: Day grouping

**Files:**
- Create: `core/src/grouping.rs`
- Modify: `core/src/lib.rs`

- [x] implement grouping of messages into day sections, oldest first, preserving input order inside a
      section; the section title is `Today`, `Yesterday`, or a formatted date
- [x] take the timezone offset and `now` as parameters — never read the wall clock inside the function
- [x] write tests: empty input, all messages today, today plus yesterday, a run spanning several
      older days, and two messages either side of local midnight landing in different sections
- [x] run the per-task gate

➕ The contract is `group_by_day(messages: &[Message], offset: UtcOffset, now: OffsetDateTime) ->
Vec<DaySection>`, where `DaySection { date: Date, title: String, messages: Vec<Message> }`. The `date`
field is what the sections are sorted by, so the function is correct for unsorted input too, and Task
9 flattens `title` + `messages` into the list's item sequence. The formatted title for an older day is
`[weekday], [day] [month repr:long]` — "Thursday, 20 August".

### Task 4: SQLite store

**Files:**
- Create: `core/src/store.rs`, `core/src/schema.rs`, `core/src/paths.rs`
- Modify: `core/src/lib.rs`, `core/Cargo.toml`

- [x] implement `open`, `open_in_memory`, schema creation on first open, and the database path
      function in `paths.rs`
- [x] implement every read method from the Store contract with explicit ordering, including the
      `sent_at` then `id` tie-break, and `message(id)`
- [x] implement `send` and `reply`; `reply` inserts and increments the root's `reply_count` in one
      transaction
- [x] write tests against in-memory databases: a message written is read back in its channel and not
      in another; `message(id)` returns exactly the row `send` returned; a reply lands in the thread
      and not in the feed, and raises the root's count; a thread query on a message with no replies is
      empty; ordering holds when two messages share a timestamp; a body with mentions round-trips
      through the database as the same spans; a timestamp with a non-UTC offset reads back equal
- [x] run the per-task gate

➕ `Store` methods take `&self`, so writes open their transaction with
`Connection::unchecked_transaction()` — `Connection::transaction()` needs `&mut self` and would force
`&mut Store` on `send`, `reply` and later `seed_if_needed`, which the state and the composers would
have to carry all the way down.

➕ `reply` errors when no row carries `root` instead of silently writing an orphan: the `UPDATE`
reports zero changed rows and the transaction is dropped without a commit, so neither statement lands.

➕ `schema.rs` is a private module (`mod schema;`), so `rusqlite` stays out of `tuclaw-core`'s public
API surface. The `messages.sent_at` column is `TEXT`; rusqlite's `time` feature writes RFC-3339 with
an explicit offset and reads it back with that offset preserved, which the non-UTC test asserts
directly.

⚠️ `ORDER BY sent_at, id` sorts that TEXT column lexicographically, so it is only chronologically
correct while all rows share one UTC offset. The fixtures derive every timestamp from one `now`, so
they do; anything later that writes mixed offsets into one channel would need a normalized sort
column.

➕ `paths::database_path` reads `HOME` rather than `std::env::home_dir`, and does not create the
directory — Task 6's `main.rs` owns that, as its checklist states.

### Task 5: Fixtures and seeding

**Files:**
- Create: `core/src/fixtures.rs`
- Modify: `core/src/store.rs`, `core/src/lib.rs`

Seeding lives here rather than in Task 4 because it cannot be written, or tested, before the data it
writes exists.

- [x] build the agents, channels and conversations described in the Fixtures section, all timestamps
      derived from the passed-in `now`
- [x] give `movie-night` its 58 messages across 15 days with one thread root carrying 4 replies, and
      at least one mention and one code span
- [x] give every other channel except `personal` its own conversation across at least two days
- [x] implement `seed_if_needed` in `store.rs`: skip when the marker exists, otherwise write the
      fixtures and the marker in one transaction
- [x] write tests: the exact channel and agent counts and order; `movie-night` holds 58 top-level
      messages; `personal` is empty and is the only empty channel; exactly one channel carries an
      unread count; the fixtures span 16 distinct days; a message seeded "yesterday" lands on the
      previous calendar day; seeding twice leaves one copy; a database carrying a marker but no rows
      is left alone
- [x] run the per-task gate

➕ `fixtures.rs` is a private module (`mod fixtures;`), like `schema.rs`: the app never builds
fixtures itself, it only calls `Store::seed_if_needed`, so `Fixtures`, `Conversation`, `SeedMessage`
and `SeedReply` stay out of `tuclaw-core`'s public API and need no `///` docs.

➕ Day offsets are pinned so the distinct-day count is exact regardless of when the app first runs:
`movie-night` uses days 0, 1, 2, 4, 6, 7, 9, 11, 13, 15, 17, 19, 21, 23 and 25 back from `now` (15
days), and the only day any other channel adds is day 3 (`media-archive`), giving 16 across all
channels. Each message keeps a fixed clock time inside its day — `now - days(n)` then
`replace_time` — so a day boundary is never crossed by accident and "Today"/"Yesterday" are always
right.

➕ The group label carries its emoji (`🎬 Movie nights`, `🏠 Home`). `Channel::group` is the only
field the sidebar section has, and the design's section titles are emoji + name, so the emoji lives
in the data rather than in a name-to-emoji table inside the view.

⚠️ Today's fixture messages sit at fixed morning times (07:45 to 11:30 local). Launching the app for
the first time before ~11:30 therefore shows a few of today's messages with a timestamp slightly
ahead of the wall clock. Deriving those times backwards from `now` instead would trade this for a
worse bug: a first launch just after local midnight would push them into yesterday and collapse a
day section.

### Task 6: AppState and the startup path

**Files:**
- Create: `app/src/state.rs`
- Modify: `app/src/main.rs`, `app/Cargo.toml`

From this task on the app opens the real database, so every later view task renders fixture data.

- [x] define `View`, `Segment`, `OpenThread`, `StateEvent` and `AppState` as in the State section,
      with the `EventEmitter` impl
- [x] implement every method from the State contract, each ending in `cx.notify()` and emitting
      exactly the events the listener table names
- [x] load agents, channels and the first channel's messages at construction
- [x] in `main.rs`: resolve the path, create the directory if absent, open the store, call
      `seed_if_needed` with the current time, and construct `AppState` — all before the window opens
- [x] write `#[gpui::test]` tests, each against `open_in_memory` plus `seed_if_needed`, for every rule
      in the State section: blank body sends nothing, emits nothing and returns `Ok`; sending appends
      only what the store accepted and emits `MessageAppended`; a store that rejects the write (a
      closed connection) makes `send` return `Err` with `messages` unchanged; selecting another
      channel leaves an open thread open, emits `SelectionChanged`, and switches `view` back from
      `Agents`; opening a thread caches its root and channel and emits `ThreadOpened`; replying
      appends to the thread, raises the root's count in both places and emits `ReplyAppended`;
      `active_segment` is right for a channel, a direct, and the agents view; `activate_segment`
      moves Channel → Direct → Channel through the remembered channels, and from Agents to each kind
- [x] write a `#[gpui::test]` proving the observation rule: an entity that registered `cx.observe`
      on the state has its callback run when `select` is called
- [x] run the per-task gate

➕ `AppState::new(store) -> Result<AppState>` takes no `Context`: `cx.new` cannot return a `Result`,
so `main` loads the workspace first and only then calls `cx.new(|_| state)`. A store carrying no
channels is an error, because `selected: ChannelId` has no empty value.

➕ `state.rs` carries `#![allow(dead_code)]`. The State contract is complete here, but its consumers
arrive across Tasks 7-14, and `make build` compiles the bin target where a `pub` method with no
caller is a warning. **Remove the attribute in Task 13**, once the last method (`reply_in_thread`)
has a caller, and confirm the gate stays green.

➕ `send` and `reply_in_thread` timestamp with `OffsetDateTime::now_utc()`, matching the fixtures'
single offset. The `time` crate's `now_local` needs the `local-offset` feature and is unsound in a
threaded process, and mixing offsets would break the store's lexicographic `ORDER BY sent_at` (Task
4's ⚠️).

➕ The "store that rejects the write" test seeds a temporary file database, drops the connection,
makes the file read-only and reopens it: `CREATE TABLE IF NOT EXISTS` and every read still succeed,
while `INSERT` fails with `attempt to write a readonly database`. `Store` exposes no way to close or
poison an in-memory connection, so this is the only reachable failure path.

➕ Store errors are reported with `eprintln!`; the workspace has no logging dependency. `select`
falls back to an empty message list when a load fails, rather than leaving the previous channel's
messages under a new header.

### Task 7: App shell — window, titlebar and top bar

**Files:**
- Create: `app/src/shell.rs`, `app/src/theme.rs`
- Modify: `app/src/main.rs`

Open `docs/design/screenshots/01-full-mockup.png` before starting.

- [x] define the theme constants; no colour literal may appear outside this module afterwards
- [x] open the window with a transparent titlebar and the traffic lights positioned into the app's own
      bar, per the Window section
- [x] build the top bar: left space for the traffic lights, the inert sidebar toggle and arrows, the
      segmented `Channel / Direct / Agents` control with the active segment raised and read from
      `active_segment()`, and `tuclaw · local` with a settings affordance on the right
- [x] make the three segments clickable: each calls `activate_segment` with its `Segment`; give each
      a `debug_selector` (`segment-channel`, `segment-direct`, `segment-agents`)
- [x] lay out the three regions — sidebar column on the warm background, feed and thread as rounded
      cards with a border, a shadow and a gap between them — with placeholder content, the shell
      observing `AppState` so the active segment repaints
- [x] write `#[gpui::test]` tests: drawing the shell does not panic; clicking `segment-agents` via
      `debug_bounds` makes `active_segment()` return `Agents`, and clicking `segment-channel` returns
      it to `Channel`
- [x] **user checkpoint** (skipped — not automatable): the agent never launches the running app, so
      `make run`, the visual comparison against `01-full-mockup.png` and clicking the three segments
      are carried to the hand-over list in Task 18
- [x] run the per-task gate

➕ The palette in `theme.rs` grows task by task rather than landing whole here. `make build` compiles
the bin target with `-D warnings`, where an unused `pub` colour is dead code and fails the gate, so
the module holds exactly the ten tones Task 7 paints with — `window`, `card`, `raised`, `sunken`,
`border`, `hairline`, `shadow`, and the three text levels. The terracotta accent arrives with the
first view that draws it.

➕ Colours are `pub fn ... -> Hsla` rather than `const`: `rgb`/`rgba` are not `const fn` at this
revision, and a `const Rgba { r, g, b, a }` literal would trade readable hex for four floats.

➕ The top bar's icons are drawn from `div()`s and text glyphs, not SVG. `gpui::svg()` needs an
`AssetSource` registered on the `App`, and no task in this plan sets one up; since the sidebar
toggle, the arrows and the settings affordance are all inert (Non-goals), a bordered box, `‹` / `›`
and three stacked rules carry the shape without an asset pipeline.

➕ `Root` is gone from `main.rs`, replaced by `Shell`; its Task 1 placeholder test is replaced by the
shell's draw test.

### Task 8: Sidebar

**Files:**
- Create: `app/src/sidebar.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/02-sidebar.png` before starting.

- [x] build the search affordance with its `⌘K` hint, the `Agents` row, the emoji-titled group
      sections, channel rows, the direct-message rows with initials chips and two-colour status dots,
      and the pinned footer with the user row and the settings gear. No `Inbox` row, no `2 running`
      pill, no activity dot — those are non-goals
- [x] render the unread badge on rows whose `unread > 0`
- [x] make channel and direct rows clickable, calling `AppState::select`, with the selection highlight
      read from state and a hover style; make the `Agents` row call `activate_segment(Agents)`; give
      every row a `debug_selector` (`sidebar-row-<name>`, `sidebar-agents`)
- [x] mount the sidebar in the shell's left column, observing `AppState`
- [x] write `#[gpui::test]` tests: drawing the sidebar against seeded state does not panic; clicking
      `sidebar-row-personal` via `debug_bounds` changes the selected channel to `personal` and leaves
      `messages` empty; clicking `sidebar-agents` makes `active_segment()` return `Agents`
- [x] run the per-task gate

➕ Sections are derived by walking `channels()` in order and opening a new section whenever the title
changes: a channel's title is its `group`, a direct's is `Direct messages`. `personal` carries no
group, so it lands in an untitled section of its own between `🏠 Home` and the directs, rather than
under `Home` as the mockup draws it — the grouping follows the data, and the domain has no field that
would put it there. A fourth test covers the derived shape: four sections of 3 / 2 / 1 / 4 rows, every
direct row carrying a chip, and the selected row carrying the highlight.

➕ Theme grew by the ten tones this view paints with: `text_label`, `field`, `selection`, `badge`,
`chip_text`, `accent`, `status_idle`, `status_busy` and `agent_chip(index)`, the last returning one of
four chip tones indexed by the agent's `sort_index`. `agent_chip` indexes an array rather than
matching, so no wildcard arm is needed.

➕ The row's `selected` flag and the `Agents` row's `active` flag are one `Highlight { On, Off }` enum
rather than a `bool`, per the Code-Quality Rules.

➕ Icons stay drawn from `div()`s, as Task 7 established: the magnifier is a bordered circle plus a
bar, the `Agents` glyph a bordered rounded rect with two dots, and the footer gear a bordered circle
with an inner ring. The channel lead is `#` for every channel — the mockup's lock on `personal` has no
field in the domain to key off.

### Task 9: Feed card and the virtualised list

**Files:**
- Create: `app/src/feed.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` and `04-direct-message.png` before starting.

- [x] build the channel header exactly as the "Derived labels and the feed header" section states:
      `#` + name + derived `N agents`, the two inert trailing chips, and the direct variant with the
      agent's chip, name and role
- [x] render the conversation with `list()` and a `ListState` owned by the view, constructed with
      `ListAlignment::Bottom`; rows are plain text for now, replaced in Task 10
- [x] flatten day separators into the same item sequence as messages, so the virtualiser sees one list
      and the count is `messages + separators`
- [x] observe `AppState`, and subscribe to it per the listener table: `SelectionChanged` →
      `reset(count)`; `MessageAppended` → `reset(count)` then `scroll_to_end()`; `ReplyAppended` →
      `notify`
- [x] render the empty state for a channel with no messages, filling the card
- [x] build the status bar along the card's bottom edge with the derived `N of M agents busy`
- [x] write `#[gpui::test]` tests: drawing the feed for the 58-message channel does not panic; drawing
      it, selecting the empty channel through state, and drawing again does not panic and the list's
      `item_count()` is 0; selecting a direct channel and drawing does not panic
- [x] run the per-task gate

➕ The flattened items live on the view as `Rc<Vec<Item>>`, rebuilt from `AppState` on every event the
listener table names, and the `list()` closure captures a clone of that `Rc`. The closure is
`FnMut(usize, &mut Window, &mut App)` and never sees the view, so it cannot read `AppState` cheaply per
row; a cached sequence also makes `reset(count)` exact. `Item::Message` holds the whole `Message` rather
than its rendered text, so `ReplyAppended` refreshes `reply_count` for Task 10's affordance without a
`reset` — the repaint-only rule the listener table states is a `Resync::Repaint` arm rather than a
skipped rebuild.

➕ Grouping runs at `UtcOffset::UTC` with `OffsetDateTime::now_utc()`, matching the single offset the
store and the fixtures already carry (Task 4's ⚠️ and Task 6's ➕). `time`'s `now_local` needs
`local-offset` and is unsound in a threaded process, so a real local offset is not available here.

➕ A fourth test covers the `MessageAppended` path: sending resyncs the list so `item_count()` matches
the rebuilt sequence and the sent body is the last item. It asserts growth rather than `+1`, because a
message sent today after a fixture seeded at an older `now` opens a new day section and adds two items.

➕ The status bar's left edge lists the busy agents with their task text, as the mockup draws it; the
derived `N of M agents busy` sits on the right. No new theme tone was needed — the header chips, the
day separators and the status bar all paint with the tones Tasks 7 and 8 established.

⚠️ The empty state and the list are alternatives, not siblings: `Feed::body` returns the empty state
when the item sequence is empty, so a channel with no messages never constructs a zero-item `list()`.
`ListState::item_count()` is still 0 there, which is what the task's test asserts.

### Task 10: Message row

**Files:**
- Create: `app/src/message.rs`
- Modify: `app/src/feed.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` before starting.

- [x] build a row: initials chip, author name, the `AGENT` badge for agent authors, the timestamp, and
      the body rendered per "Inline spans in a paragraph"
- [x] render the "N replies" affordance when `reply_count > 0`, and a hover-revealed reply affordance
      when it is zero; both take an `on_open` callback the feed supplies and carry a `debug_selector`
      of `message-reply-<id>`
- [x] replace the feed's plain rows with this one, the feed passing an `on_open` that calls
      `AppState::open_thread`
- [x] keep the row free of channel knowledge — it is reused by the thread panel
- [x] write tests: the reply label reads "1 reply" for one and "N replies" otherwise; clicking
      `message-reply-<root id>` via `debug_bounds` on the drawn feed opens that thread in state; the
      feed draw tests from Task 9 still pass
- [x] run the per-task gate

➕ `on_open` is `pub type OnOpen = Rc<dyn Fn(MessageId, &mut Window, &mut App)>`, built once in
`Feed::body` from a clone of `Entity<AppState>` and cloned per row. The `list()` closure never sees the
view, so `cx.listener` is unavailable; the callback updates the state entity through the `&mut App` the
closure is handed. The same `&mut App` supplies the row's agents with `state.read(cx)`, so `message_row`
takes `&[Agent]` rather than reading state itself and stays reusable by Task 13's thread panel.

➕ The hover-revealed affordance is absolutely positioned at the row's top-right, drawn with
`.invisible()` and `.group_hover(group, |style| style.visible())` against a per-row `.group("message-<id>")`.
gpui carries no `visible_on_hover` at this revision — that helper lives in Zed's own `ui` crate — and a
`Visibility::Hidden` element still takes its layout space, so placing it in the column would leave a
pill-sized gap under every reply-less message.

➕ `VisualTestContext::debug_bounds` takes `&'static str`, not `&str`, so the click test leaks its
formatted `message-reply-<id>` selector. A hidden element still records its debug bounds — the
visibility check in `div`'s `paint` comes after the insert — but registers no click listener, so only
the visible pill is clickable.

➕ Theme grew by `mention_field` and `mention_text`, the two tones the mockup's inline mention chip
needs. The `AGENT` badge and the inline code chip paint with `sunken()`, and the code chip keeps the
window's font: `font_family` on a name font-kit cannot resolve is a failure path no task here handles.

⚠️ A `Span::Text` is split on whitespace, so runs of spaces and the newlines a Shift+Enter body carries
(Task 12) collapse to one word gap on screen. The stored body keeps them; this is the cost of the
"one text element per word" rule the Inline spans section settles on.

### Task 11: Text input element

**Files:**
- Create: `app/src/input.rs`
- Modify: `app/src/feed.rs`, `app/src/main.rs`

The largest task in the plan. Read the **Text** and **Text input** facts above before writing a line;
all four requirements there are mandatory, and the UTF-16 rule is the one that fails silently. The
bare input is mounted at the bottom of the feed card here so the file has a consumer; Task 12 dresses
it into the composer.

- [x] define `TextInput` per the Text input section, holding its text, a UTF-16 caret, the marked
      range, and its `FocusHandle`
- [x] implement the eight required `EntityInputHandler` methods, with byte ↔ UTF-16 conversion at the
      boundary and nowhere else, and the marked-text transitions as the facts state them
- [x] implement the manual `Element`: shape with `shape_text` at the given wrap width in `prepaint`,
      size to the wrapped line count with no maximum, paint the text and the caret quad in `paint`,
      and register `window.handle_input(...)` there; wrap it in a `div()` with `track_focus` and a
      mouse-down handler that focuses the handle
- [x] bind the keys: characters via the input handler; `backspace`, `left`, `right`, `enter`,
      `shift-enter` as actions under a `key_context`; Enter emits `Submitted`, Shift+Enter inserts a
      newline
- [x] mount one bare `TextInput` at the bottom of the feed card and focus it when the window opens
- [x] write `#[gpui::test]` tests with the input focused: `simulate_input("hello")` gives text
      `hello` and caret 5; `simulate_input("привет 🐢")` gives that text and a caret at its UTF-16
      length, not its byte length; backspace after the emoji removes the whole emoji; left then a
      typed character inserts before the caret; `shift-enter` leaves a `\n` in the text and emits
      nothing; `enter` emits `Submitted`; `clear` empties the text and resets the caret
- [x] write `#[gpui::test]` tests for IME: `replace_and_mark_text_in_range` with `"ぱ"` marks it and
      `marked_text_range` reports a UTF-16 range of length 1; a second call replaces the marked text;
      `unmark_text` keeps the text and clears the range; `replace_text_in_range` over a marked input
      replaces the marked text and clears the mark
- [x] write `#[gpui::test]` tests for shaping: an input drawn at a narrow width with a long line
      reports a height of more than one line; three `shift-enter`s make it four lines tall; clicking
      the input via `debug_bounds` (`input-feed`) focuses its handle
- [x] run the per-task gate

➕ The height comes from `Window::request_measured_layout`, not from a `Style` height: the wrapped
line count is only knowable once taffy offers a width, and the measure closure is where that width
arrives. The element therefore shapes twice a frame — once to measure, once in `prepaint` at the final
bounds — which gpui's line-layout cache turns into a lookup the second time. `prepaint` keeps the
`WrappedLine`s (they are not `Clone`), `paint` moves them onto the entity, and `bounds_for_range` and
`character_index_for_point` read them from there.

➕ `TextInput::new(placeholder, selector, cx)` takes a debug selector, because the selector belongs on
the same `div()` that owns `track_focus` and the focus-on-click handler, and Tasks 12 and 13 put two
inputs on screen at once (`input-feed`, `input-thread`).

➕ Boundaries are `char` boundaries, not grapheme clusters: the workspace carries no
`unicode-segmentation` dependency and GPUI's own example is the only thing that pulls one in. One
backspace removes a whole emoji, as the task requires, but not a whole ZWJ sequence.

➕ `bind_keys(cx)` binds the five actions under the `TuclawInput` key context once per `App`; `main`
calls it before the window opens and each test calls it before building its harness. Without a
binding, `enter` and `shift-enter` would reach the input handler as a literal `\n` — GPUI's
`with_simulated_ime` fills `key_char` for both.

➕ `input.rs` carries `#![allow(dead_code)]` for the reason `state.rs` does: `text()`, `is_blank()`
and `clear()` have no caller in the bin target until Task 12's composer. **Remove it in Task 12.**

➕ The feed takes the initial focus on its first `render` (`InitialFocus::Pending` → `Taken`), since
`Feed::new` is handed no `Window` and this task changes neither `shell.rs` nor `Shell::new`'s
signature. `Window::focus` during a draw skips its refresh but still sets the focus, and paint runs
after render in the same frame, so `window.handle_input` registers on that first frame.

### Task 12: Composer

**Files:**
- Create: `app/src/composer.rs`
- Modify: `app/src/feed.rs`, `app/src/main.rs`

Open `docs/design/screenshots/01-full-mockup.png` before starting — the composer at the bottom of the
feed card.

- [x] build `Composer` per its section: it owns a `TextInput`, takes `ComposerKind`, a placeholder
      and an `on_submit`, subscribes to `Submitted`, and clears the input only when `on_submit`
      returns `Ok`
- [x] draw the `Feed` shape: the placeholder naming the target (`Message #channel` or `Message
      <agent>`), the four inert icons, the hint, the inert `Talk` chip, and the 32 px send button,
      disabled while the input is blank; the send button calls the same submit path
- [x] replace Task 11's bare mount with a `Feed` composer whose `on_submit` calls `AppState::send`;
      the feed already scrolls on `MessageAppended`; subscribe to `ThreadClosed` to refocus this
      input
- [x] write `#[gpui::test]` tests: with the composer's input focused, `simulate_input("hi")` then
      `simulate_keystrokes("enter")` appends a message with body `[Span::Text("hi")]` to the selected
      channel and leaves the input empty; `enter` on a blank input appends nothing;
      `simulate_input("a")`, `shift-enter`, `simulate_input("b")`, `enter` appends a body containing
      `a\nb`; when `on_submit` returns `Err`, the input still holds its text
- [x] **user checkpoint** (skipped — not automatable): the agent never launches the running app, so
      `make run`, typing in `movie-night` with a Shift+Enter break and Cyrillic, and watching the
      feed scroll to the sent message are carried to the hand-over list in Task 18
- [x] run the per-task gate

➕ `ComposerKind` carries only its `Feed` variant here; **Task 13 adds `Thread`**. `make build`
compiles the bin target with `-D warnings`, where a variant nothing constructs is dead code, and an
exhaustive `match self.kind` would otherwise need a `Thread` arm drawing a shape Task 13 owns. The
`#![allow(dead_code)]` route Tasks 6 and 11 took does not help: it silences the unused variant, not
the missing arm.

➕ The placeholder names the *selected* channel, so it cannot be fixed at construction as the
`Composer::new` contract implies. `Composer::set_placeholder` and `TextInput::set_placeholder` were
added, and the feed calls the first from its `SelectionChanged` arm. Re-creating the composer per
selection was the alternative and would drop focus mid-session.

➕ Focusing on `ThreadClosed` reuses Task 11's deferred-focus trick rather than `cx.subscribe_in`:
`Feed::new` is handed no `Window`, so the subscription sets `Focus::Requested` and the next `render`
takes the focus. Task 11's `InitialFocus` enum is now that `Focus` enum, since first focus and
refocus-after-close are the same move.

➕ `input.rs` lost its `#![allow(dead_code)]` as Task 11 required: `text()`, `is_blank()`, `clear()`
and `focus_handle()` all have callers in the composer now. `state.rs` keeps its attribute until Task
13 gives `reply_in_thread` a caller.

➕ A fifth test beyond the four the task lists: clicking `composer-send-feed` via `debug_bounds`
sends the typed body, which is what "the send button calls the same submit path" claims. No new theme
tone was needed.

### Task 13: Thread panel

**Files:**
- Create: `app/src/thread.rs`
- Modify: `app/src/composer.rs`, `app/src/feed.rs`, `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` before starting — the right-hand card.

- [x] build the panel as its own card: the `Thread` header with the `#channel · author` subtitle read
      from `OpenThread`, a close control with `debug_selector` `thread-close`, the cached root
      message, the replies as a plain column (four replies need no virtualiser), and a `Thread`
      composer placeheld "Reply in thread"
- [x] add the `Thread` shape to `composer.rs`: `@` and a microphone, no hint, no `Talk`, a 30 px
      send button
- [x] show the panel only when `thread` is `Some` — the shell observes state for this; wire the close
      control to `AppState::close_thread`
- [x] wire the composer's `on_submit` through `AppState::reply_in_thread`; subscribe to
      `ThreadOpened` to focus the reply input and to `ReplyAppended` to repaint
- [x] write `#[gpui::test]` tests: drawing the panel with a thread open does not panic; opening a
      thread then selecting another channel and drawing still renders — the root comes from
      `OpenThread`, not from the selection; replying through the panel's input appends to the thread
      and raises the root's count; clicking `thread-close` via `debug_bounds` sets `thread` to
      `None`; after `open_thread` the thread input's handle is focused, and after `close_thread` the
      feed input's is
- [x] **user checkpoint** (skipped — not automatable): the agent never launches the running app, so
      `make run`, opening a thread from `movie-night`, replying in it, switching channel to confirm
      the thread stays, and closing it are carried to the hand-over list in Task 18
- [x] run the per-task gate

➕ `message.rs` gained a `Replies { Affordance(OnOpen), Hidden }` parameter in place of
`message_row`'s bare `on_open`, and the thread panel passes `Hidden`. Task 10 settled that the row is
reused here, but reusing it unchanged would draw the root's "4 replies" pill *inside* the thread the
pill opens, and would register a second `message-reply-<id>` debug selector for the same row while
both cards are on screen. The mockup's thread root carries no pill, only the hairline under it.

➕ `message.rs` also exposes `author_name(author, agents)`, which the header's `#channel · author`
subtitle needs and which `writer` now calls, so the "You" / "unknown agent" fallbacks are stated once.

➕ Both `Feed` and `ThreadPanel` carry a `#[cfg(test)] pub fn input_focus`. The focus test needs the
two composers' handles from one window, and each composer field is private to its own module. Gating
the accessor on `cfg(test)` keeps it out of the bin target, where an accessor with no caller is dead
code under `-D warnings`. The test mounts a `Harness` holding both views rather than a `Shell`,
because `Shell`'s fields are private to `shell.rs` too, and it asserts the thread input gives the
focus up as well as that the feed input takes it, so the assertion cannot pass vacuously.

➕ `state.rs` lost its `#![allow(dead_code)]` as Task 6 required, and the gate stayed green:
`reply_in_thread` has the thread composer as a caller now, and every other public method already had
one.

➕ The mockup's inert bell in the thread header is not drawn. The task enumerates the header's
contents and a bell is in none of the Non-goals' "drawn but inert" lists, so it is left out rather
than added on the screenshot's authority.

### Task 14: Agents view

**Files:**
- Create: `app/src/agents.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/05-agents-and-settings.png` before starting; build the card list on the
left of that screenshot and **not** the settings panel on its right, which is a non-goal.

- [ ] extract a pure `agent_cards(agents: &[Agent]) -> Vec<AgentCard>` producing the ordered
      view-model — name, initials, role, status label — in `sort_index` order
- [ ] render one card per entry, and a summary line counting the agents and how many are busy
- [ ] show this view in place of the feed and thread when `view == Agents`; the sidebar row (Task 8)
      and the top-bar segment (Task 7) already call `activate_segment(Agents)`, and the shell's
      observation repaints the swap
- [ ] write tests: `agent_cards` over the seeded agents returns four entries in `sort_index` order
      with the right busy labels; drawing the agents view does not panic; clicking `sidebar-agents`
      and then `sidebar-row-movie-night` via `debug_bounds` ends with `active_segment() == Channel`
- [ ] run the per-task gate

### Task 15: Failure view

**Files:**
- Create: `app/src/failure.rs`
- Modify: `app/src/main.rs`

- [ ] when the store cannot be opened, show a plain view naming the path and the error instead of
      panicking; the window must still open
- [ ] write a test: constructing the app against an unwritable path produces the failure state
- [ ] run the per-task gate

### Task 16: README and repository documentation

**Files:**
- Create: `README.md`, `CLAUDE.md`

- [ ] write the README: what the app is, the toolchain traps from the Toolchain section including the
      `mise exec` rule and the `gpui_platform` entry point, and how to build, run and test through
      `make`
- [ ] write `CLAUDE.md`: the crate split and why `core` must never depend on `gpui`, the state
      ownership rule and the input-owns-its-buffer rule, the observe-at-construction rule, the
      `ListState` resync rule, the focus rule, the two settled behaviours, and where the design lives
- [ ] run the per-task gate

### Task 17: Verify acceptance criteria

- [ ] `make fmt-check`, `make lint`, `make test`, `make build` — all clean
- [ ] confirm `core` has no `gpui` dependency: `tree=$(mise exec -- cargo tree -p tuclaw-core)` must
      succeed, and then `printf '%s' "$tree" | grep -q gpui` must exit 1
- [ ] confirm no comments were added: `grep -rnE '(^|[^:"])//($|[^/!])' core/src app/src` and
      `grep -rn '/\*' core/src app/src` both print nothing
- [ ] confirm no colour literal exists outside `app/src/theme.rs`
- [ ] confirm every non-goal is still absent — nothing from that list crept in
- [ ] state in this plan what could not be verified without the user running the app

### Task 18: [Final] Hand over

- [ ] list in this plan what the user should check by hand: scroll smoothness at 58 messages, typing
      latency, clicking each channel and direct, the three segments, opening and closing a thread,
      Cyrillic and emoji in the composer, and whether the window still matches
      `docs/design/screenshots/01-full-mockup.png`
- [ ] move this plan to `docs/plans/completed/`

## Post-Completion

*No checkboxes — these need the user or a decision, not the agent.*

**Manual judgement.** Only the user can say whether it feels right: input latency, scrolling at 58
messages, and whether the chrome still reads as the mockup once things move.

**Decisions deferred out of v1.** Whether the app follows the system light/dark appearance; whether
the search field becomes real; whether the structured message cards from the mockup get built;
whether text selection and mouse caret placement are added to the input; whether the composer gets a
height cap; whether the SwiftUI implementation in `../tuclaw-client` is archived or deleted.

**Known risk carried forward.** GPUI is pre-1.0 and pinned to a git revision taken while its platform
crates were being split apart. Moving the pin later is a real task, not a version bump, and nothing in
v1 should assume the API is stable.
