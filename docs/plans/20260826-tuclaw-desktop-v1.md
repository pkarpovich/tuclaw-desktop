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
- Voice. The `Talk` button in the composer is drawn and does nothing.
- Attachments, emoji picker, mentions picker, formatting. The composer's icon row — `@`, paperclip,
  smiley, `Aa` — is drawn exactly as the mockup shows it and none of the four does anything.
- Structured message cards — the film picker, the run log, the task progress card, the decision card.
  Messages are text with inline agent mentions and inline code, nothing else.
- Search behaviour. The search field is drawn as an affordance and searches nothing.
- The `Inbox` sidebar row and the `Agent crews` sidebar section.
- Per-channel agent membership and running-task counts: the sidebar's `2 running` pill, the feed
  header's `N tasks running`, and the plain activity dot on a channel row. The domain has no data for
  any of them. What the header does show is derived — see Technical Details.
- A third agent status colour. `AgentStatus` has two variants; status dots are green or amber.
- The agent settings panel (Role / Permissions / How it replies / Voice replies / Disconnect).
- Editing, deleting or reacting to messages.
- The sidebar toggle and the back / forward arrows in the top bar: drawn, inert.
- Multi-window, a preferences window, menu bar work beyond GPUI's defaults.
- Selection inside the text input, and placing the caret with the mouse. Typing, backspace, arrow
  keys, IME, and a visible caret are in scope; dragging to select is not.
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
- **A draft field on `AppState`.** Rejected: the input element must own its buffer anyway (the
  `EntityInputHandler` contract requires it), and two composers on screen at once would fight over one
  field. The state receives a body on submit and nothing else.
- **A marker syntax inside the body string.** Rejected: any in-band marker needs escaping rules three
  cold sessions would have to agree on. The body is stored as JSON and the question does not arise.
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
- `gpui` and `gpui_platform` from git, rev `fecc3273ed32643c2ea1b04a74c8780e2c9ffaf8`.
  `gpui_platform` needs the `font-kit` feature on macOS; without it text lays out but renders no
  glyphs. `gpui` needs `test-support` as a dev-dependency feature for the test tier.
- Xcode's **Metal toolchain** must be installed — `gpui_apple` compiles `shaders.metal` in a build
  script. If missing, the build fails with "cannot execute tool 'metal'"; install with
  `xcodebuild -downloadComponent MetalToolchain` (~690 MB).
- `rusqlite` with the `bundled` feature; `serde` + `serde_json` for the message body; `time` for
  timestamps.
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

**State.** `Context::notify()` re-renders an entity's observers. Cross-entity signalling is
`impl EventEmitter<E> for T` (a marker trait), `cx.emit(event)` from inside the emitter, and
`cx.subscribe(&entity, |this, emitter, event, cx| ...)` on the listener. This is how a view that owns
a `ListState` learns that `AppState` changed the item set.

**Text.** `shape_line(text, font_size, runs, force_width)` is single-line by contract: it carries
`debug_assert!(text.find('\n').is_none())`, and `cargo run` and `cargo test` build in debug, so the
assert is live. Multi-line text goes through `shape_text(text, font_size, runs, wrap_width:
Option<Pixels>, line_clamp: Option<usize>) -> SmallVec<[WrappedLine; 1]>`. A multi-line input
therefore lays out with `shape_text`, derives its height from the wrapped line count, and maps caret
position and `bounds_for_range` across wrapped lines. GPUI's own `examples/input.rs` uses
`shape_line` and strips newlines on paste — it is a single-line reference and does not show any of
this.

**Text input.** GPUI ships no text field. Three things make one work, and all three are required:

1. The element is a manual `impl Element` — `request_layout`, `prepaint`, `paint` — because it must
   shape text in `prepaint` and draw its own caret with `window.paint_quad(...)` in `paint`. A `div()`
   composition cannot do it.
2. Implementing `EntityInputHandler` does nothing on its own. The element's `paint` must call
   `window.handle_input(&focus_handle, ElementInputHandler::new(bounds, entity), cx)`, and that only
   registers while the focus handle is focused. Keyboard events reach the entity through a
   `FocusHandle`, a `key_context(...)` and `KeyBinding::new(...)` bound to actions.
3. `EntityInputHandler` has **eight required methods**: `text_for_range`, `selected_text_range`,
   `marked_text_range`, `unmark_text`, `replace_text_in_range`, `replace_and_mark_text_in_range`,
   `bounds_for_range`, `character_index_for_point`. Four have defaults: `paste`,
   `set_selected_text_range`, `text_length_utf16`, `accepts_text_input`. **Every range crossing the
   trait is in UTF-16 code units**, not byte offsets — the signatures say `UTF16Selection` and
   `range_utf16`. The element keeps a `String` and converts both ways at the boundary. Tests that use
   only ASCII cannot catch a byte-offset implementation; the tests below use Cyrillic and an emoji.

**Window chrome.** `WindowOptions::titlebar` takes `TitlebarOptions { title, appears_transparent,
traffic_light_position }`. Transparent titlebar plus an explicit traffic-light position is how the app
draws its own top bar.

**Tests.** `#[gpui::test]` provides a `TestAppContext`. `VisualTestContext` adds `draw(...)`,
`simulate_input(...)`, `simulate_keystrokes(...)` and `simulate_click(position, modifiers)`. All of
it is behind the `test-support` feature and runs in-process — it does not open a window on the user's
screen or take the cursor. A draw test proves only that rendering did not panic; it says nothing about
what was drawn.

### Consequence for the test tier

`simulate_click` addresses a **point**, not an identifier. There is no accessibility-identifier
contract to write, and tests must not be built around guessing coordinates. The tier is therefore:
pure logic tested directly, state transitions tested through `AppState` methods, the input element
tested through `simulate_input` and `simulate_keystrokes` with focus set explicitly, and a draw test
per view to catch panics. Interaction that only a click can exercise — a sidebar row, a reply
affordance, the thread's close control — is verified by the user at the checkpoints below.

## Development Approach

- **Testing approach**: regular — implement, then test in the same task. Tests are a required
  deliverable of every task, not an afterthought.
- Complete each task fully before starting the next. All gates green before any `[x]`.
- **Tasks 1-6 build `tuclaw-core` and the state behind an intentionally empty window.** The render
  rule starts at Task 7: from then on every task must leave the app building, running, and showing
  real fixture data.
- **The agent never drives the running app.** It does not launch it in the foreground, click, or type
  into it. Three tasks — 7, 12 and 13 — end with a **user checkpoint**: the agent stops, asks the
  user to run `make run` and compare against the named screenshot, and does not tick the task until
  the user answers. "The window must render" is a user-verified criterion, not an agent-verified one.
- Update this plan as scope changes: `[x]` when done, `➕` for discovered work, `⚠️` for blockers.

## Code-Quality Rules (verify before marking each task complete)

From the `rust-style` skill. These are not suggestions:

- `for` loops with mutable accumulators, not iterator chains (`.iter().filter().map().collect()`).
- `let ... else` for early returns; the happy path stays unindented. `if let` only for a short action
  with no else branch.
- Shadow variables through transformations; no `raw_` / `parsed_` / `trimmed_` prefixes.
- **No comments.** No inline explanations, no section dividers, no TODOs, no commented-out code. The
  only exceptions: `///` doc comments on public items of `tuclaw-core`, and `//!` module docs where a
  task below asks for one.
- Newtypes over bare `String` where the string carries meaning; enums over `bool` parameters.
- Never a wildcard `_ =>` match; match every variant. Avoid `matches!`.
- Destructure structs and tuples explicitly.

Per-task gate — all four green, run through `make`, never bare `cargo`:

- `make fmt-check`
- `make lint` — `cargo clippy --workspace --all-targets -- -D warnings`
- `make test`
- `make build` — no warnings, dead code included

## Testing Strategy

Two tiers, split by what they need to run.

**Tier 1 — `tuclaw-core`, plain `#[test]`.** The crate does not depend on `gpui`, so these run with no
window and no GPU. Covers: domain invariants, the body encoding round trip, day grouping, the SQLite
schema and every store method, seeding idempotence, and the fixture data itself.

Store tests run against an in-memory SQLite database, one per test, so they are order-independent and
parallel-safe.

**Tier 2 — `tuclaw-desktop`, `#[gpui::test]`.** Needs `gpui` with `test-support`. Covers state
transitions through `AppState` methods, the input element through simulated input, pure view-model
functions, and one draw test per view. What it does NOT do: assert on pixels, walk a view hierarchy
looking for text, or click by guessed coordinates.

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
the channel list, the selected channel and its messages, the open thread with its root message, and
which view is showing. Views hold `Entity<AppState>` and read through it. Every mutation is a method
on `AppState` that ends in `cx.notify()` and, where another view has to react, `cx.emit(...)`. No
view mutates another view's data, and no view talks to the store directly.

The one thing `AppState` does **not** own is text being typed. Each composer creates a `TextInput`
entity that owns its own buffer, as the input-handler contract requires; the composer hands the body
to `AppState` on submit and clears the input afterwards.

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
body, sent_at, reply_count)`, `seed_marker`.

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
pub struct OpenThread { pub root: Message, pub channel: ChannelId, pub replies: Vec<Message> }
pub enum StateEvent { SelectionChanged, MessageAppended, ThreadChanged, ViewChanged }
pub struct AppState { /* store, agents, channels, selected: ChannelId, messages, thread: Option<OpenThread>, view: View */ }
impl EventEmitter<StateEvent> for AppState {}
pub fn select(&mut self, channel: ChannelId, cx: &mut Context<Self>);
pub fn set_view(&mut self, view: View, cx: &mut Context<Self>);
pub fn send(&mut self, body: String, cx: &mut Context<Self>);
pub fn open_thread(&mut self, root: MessageId, cx: &mut Context<Self>);
pub fn close_thread(&mut self, cx: &mut Context<Self>);
pub fn reply_in_thread(&mut self, body: String, cx: &mut Context<Self>);
```

Rules these methods enforce, each of which gets a test:

- `send` and `reply_in_thread` trim the body; a blank result reaches no store call and emits nothing.
- A row appears in `messages` only after the store accepted it, and only then is `MessageAppended`
  emitted. A failed write leaves the channel untouched and logs.
- `reply_in_thread` appends to `thread.replies` and bumps `thread.root.reply_count` **and** the
  matching row in `messages` when the root is in the selected channel, so the feed's affordance
  updates without a refetch.
- `select` emits `SelectionChanged` and does not touch `thread`.
- `open_thread` loads the root via `Store::message` and the replies via `Store::thread`, stores all
  three, and emits `ThreadChanged`; `close_thread` sets `thread` to `None` and emits the same.
- The active top-bar segment is derived, not stored: `Agents` when `view == Agents`; otherwise
  `Direct` when the selected channel's kind is `Direct`, else `Channel`. Clicking `Channel` or
  `Direct` calls `set_view(Conversation)`; clicking `Agents` calls `set_view(Agents)`.

### Derived labels

The feed header's subtitle shows `N agents` where N is the number of distinct agent authors among the
channel's messages — derivable, no domain field. The status bar shows `N of M agents busy` from
`AgentStatus` across all agents. Nothing else in the mockup's counters is shown; see Non-goals.

### Text input (`app/src/input.rs`)

```rust
pub struct TextInput { /* text: String, cursor_utf16: usize, marked: Option<Range<usize>>, focus: FocusHandle, placeholder */ }
pub enum Submit { Send, Newline }
pub fn text(&self) -> &str;
pub fn is_blank(&self) -> bool;
pub fn clear(&mut self, cx: &mut Context<Self>);
impl EventEmitter<Submit> for TextInput {}
impl EntityInputHandler for TextInput { /* the eight required methods */ }
```

Enter emits `Submit::Send`; Shift+Enter inserts `\n` at the caret and emits nothing. The element that
renders it is a separate manual `Element` that shapes with `shape_text` at the composer's width, sizes
itself to the wrapped line count (minimum one line, maximum six, then scrolls), and paints the caret.

### Theme (`app/src/theme.rs`)

One module of named colour constants and nothing else — no colour literal anywhere in the view code.
The palette is warm and light, taken from the mockup: a cream window, white cards, a terracotta
accent, three levels of text, one border and one hairline tone. v1 ships a single palette and does not
follow the system appearance; that is a deliberate simplification, not an oversight.

### Window

`WindowOptions` with `titlebar: Some(TitlebarOptions { title: None, appears_transparent: true,
traffic_light_position: Some(...) })`, default size 1280×820. The app's own top bar reserves space on
the left for the traffic lights and holds the (inert) sidebar toggle, the (inert) back and forward
arrows, the `Channel / Direct / Agents` segmented control, and `tuclaw · local` with a settings
affordance on the right.

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

- [ ] create the workspace with members `core` and `app`; `core` must not list `gpui` as a dependency
- [ ] pin Rust 1.98.0 in `mise.toml` and add the two GPUI git dependencies with the rev from Toolchain,
      `gpui` with `test-support` under `dev-dependencies`
- [ ] write the `Makefile` with `build`, `run`, `test`, `lint`, `fmt`, `fmt-check` targets, **every
      one wrapping cargo in `mise exec --`**
- [ ] confirm `mise exec -- rustc --version` prints 1.98.0 before the first build
- [ ] open an empty window from `app/src/main.rs` so the Metal build step and window creation are
      exercised; the window is expected to be blank until Task 7
- [ ] write one placeholder test in each crate and confirm `make test` passes
- [ ] run the per-task gate

### Task 2: Domain types and the body encoding

**Files:**
- Create: `core/src/model.rs`
- Modify: `core/src/lib.rs`, `core/Cargo.toml`

- [ ] define every type from the Domain section, deriving `Debug`, `Clone`, `PartialEq` and, for the
      id newtypes, `Eq` and `Hash`
- [ ] derive `Serialize` and `Deserialize` on `Span` and provide `encode(&[Span]) -> String` and
      `decode(&str) -> Result<Vec<Span>>` over `serde_json`
- [ ] write tests: plain text, a mention mid-sentence, inline code, several spans in one body, an
      empty body, text containing `[`, `{`, `"` and a backslash — all survive `encode` then `decode`;
      malformed JSON is an error, not a panic
- [ ] run the per-task gate

### Task 3: Day grouping

**Files:**
- Create: `core/src/grouping.rs`
- Modify: `core/src/lib.rs`

- [ ] implement grouping of messages into day sections, oldest first, preserving input order inside a
      section; the section title is `Today`, `Yesterday`, or a formatted date
- [ ] take the timezone offset and `now` as parameters — never read the wall clock inside the function
- [ ] write tests: empty input, all messages today, today plus yesterday, a run spanning several
      older days, and two messages either side of local midnight landing in different sections
- [ ] run the per-task gate

### Task 4: SQLite store

**Files:**
- Create: `core/src/store.rs`, `core/src/schema.rs`, `core/src/paths.rs`
- Modify: `core/src/lib.rs`, `core/Cargo.toml`

- [ ] implement `open`, `open_in_memory`, schema creation on first open, and the database path
      function in `paths.rs`
- [ ] implement every read method from the Store contract with explicit ordering, including the
      `sent_at` then `id` tie-break, and `message(id)`
- [ ] implement `send` and `reply`; `reply` inserts and increments the root's `reply_count` in one
      transaction
- [ ] write tests against in-memory databases: a message written is read back in its channel and not
      in another; `message(id)` returns exactly the row `send` returned; a reply lands in the thread
      and not in the feed, and raises the root's count; a thread query on a message with no replies is
      empty; ordering holds when two messages share a timestamp; a body with mentions round-trips
      through the database as the same spans
- [ ] run the per-task gate

### Task 5: Fixtures and seeding

**Files:**
- Create: `core/src/fixtures.rs`
- Modify: `core/src/store.rs`, `core/src/lib.rs`

Seeding lives here rather than in Task 4 because it cannot be written, or tested, before the data it
writes exists.

- [ ] build the agents, channels and conversations described in the Fixtures section, all timestamps
      derived from the passed-in `now`
- [ ] give `movie-night` its 58 messages across 15 days with one thread root carrying 4 replies, and
      at least one mention and one code span
- [ ] give every other channel except `personal` its own conversation across at least two days
- [ ] implement `seed_if_needed` in `store.rs`: skip when the marker exists, otherwise write the
      fixtures and the marker in one transaction
- [ ] write tests: the exact channel and agent counts and order; `movie-night` holds 58 top-level
      messages; `personal` is empty and is the only empty channel; exactly one channel carries an
      unread count; the fixtures span 16 distinct days; a message seeded "yesterday" lands on the
      previous calendar day; seeding twice leaves one copy; a database carrying a marker but no rows
      is left alone
- [ ] run the per-task gate

### Task 6: AppState and the startup path

**Files:**
- Create: `app/src/state.rs`
- Modify: `app/src/main.rs`, `app/Cargo.toml`

From this task on the app opens the real database, so every later view task renders fixture data.

- [ ] define `View`, `OpenThread`, `StateEvent` and `AppState` as in the State section, with the
      `EventEmitter` impl
- [ ] implement every method from the State contract, each ending in `cx.notify()` and, where the
      section says so, `cx.emit(...)`
- [ ] load agents, channels and the first channel's messages at construction
- [ ] in `main.rs`: resolve the path, create the directory if absent, open the store, call
      `seed_if_needed` with the current time, and construct `AppState` — all before the window opens
- [ ] write `#[gpui::test]` tests, each against `open_in_memory` plus `seed_if_needed`, for every rule
      in the State section: blank body sends nothing and emits nothing; sending appends only what the
      store accepted and emits `MessageAppended`; selecting another channel leaves an open thread
      open and emits `SelectionChanged`; opening a thread caches its root and channel; replying
      appends to the thread and raises the root's count in both places; the derived active segment
      is right for a channel, a direct, and the agents view
- [ ] run the per-task gate

### Task 7: App shell — window, titlebar and top bar

**Files:**
- Create: `app/src/shell.rs`, `app/src/theme.rs`
- Modify: `app/src/main.rs`

Open `docs/design/screenshots/01-full-mockup.png` before starting.

- [ ] define the theme constants; no colour literal may appear outside this module afterwards
- [ ] open the window with a transparent titlebar and the traffic lights positioned into the app's own
      bar, per the Window section
- [ ] build the top bar: left space for the traffic lights, the inert sidebar toggle and arrows, the
      segmented `Channel / Direct / Agents` control with the active segment raised and derived from
      state, and `tuclaw · local` with a settings affordance on the right
- [ ] lay out the three regions — sidebar column on the warm background, feed and thread as rounded
      cards with a border, a shadow and a gap between them — with placeholder content
- [ ] write a `#[gpui::test]` that draws the shell without panicking
- [ ] **user checkpoint**: stop and ask the user to run `make run` and compare against
      `01-full-mockup.png`; do not tick this task until they answer
- [ ] run the per-task gate

### Task 8: Sidebar

**Files:**
- Create: `app/src/sidebar.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/02-sidebar.png` before starting.

- [ ] build the search affordance with its `⌘K` hint, the `Agents` row, the emoji-titled group
      sections, channel rows, the direct-message rows with initials chips and two-colour status dots,
      and the pinned footer with the user row and the settings gear. No `Inbox` row, no `2 running`
      pill, no activity dot — those are non-goals
- [ ] render the unread badge on rows whose `unread > 0`
- [ ] make channel and direct rows clickable, calling `AppState::select`, with the selection highlight
      driven by state and a hover style; make the `Agents` row call `set_view(Agents)`
- [ ] mount the sidebar in the shell's left column
- [ ] write a `#[gpui::test]` that draws the sidebar against seeded state without panicking
- [ ] run the per-task gate

### Task 9: Feed card and the virtualised list

**Files:**
- Create: `app/src/feed.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` and `04-direct-message.png` before starting.

- [ ] build the channel header: `#`, the channel name, and the derived `N agents` subtitle, with the
      trailing controls from the mockup; for a `Direct` channel show the agent's chip, name and role
      instead, per `04-direct-message.png`
- [ ] render the conversation with `list()` and a `ListState` owned by the view, constructed with
      `ListAlignment::Bottom`; rows are plain text for now, replaced in Task 10
- [ ] flatten day separators into the same item sequence as messages, so the virtualiser sees one list
      and the count is `messages + separators`
- [ ] subscribe to `AppState`: on `SelectionChanged` call `reset(count)`; on `MessageAppended` call
      `reset(count)` then `scroll_to_end()`
- [ ] render the empty state for a channel with no messages, filling the card so the composer stays at
      the bottom
- [ ] build the status bar along the card's bottom edge with the derived `N of M agents busy`
- [ ] write `#[gpui::test]` tests: drawing the feed for the 58-message channel does not panic; drawing
      it, selecting the empty channel through state, and drawing again does not panic and the list's
      `item_count()` is 0; selecting a direct channel and drawing does not panic
- [ ] run the per-task gate

### Task 10: Message row

**Files:**
- Create: `app/src/message.rs`
- Modify: `app/src/feed.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` before starting.

- [ ] build a row: initials chip, author name, the `AGENT` badge for agent authors, the timestamp, and
      the body
- [ ] render `Span::Mention` and `Span::Code` as inline chips inside the flowing paragraph, not as
      separate blocks
- [ ] render the "N replies" affordance when `reply_count > 0`, and a hover-revealed reply affordance
      when it is zero; both take an `on_open` callback the feed supplies
- [ ] replace the feed's plain rows with this one
- [ ] keep the row free of channel knowledge — it is reused by the thread panel
- [ ] write tests for the pure parts: the reply label reads "1 reply" for one and "N replies"
      otherwise; the feed draw tests from Task 9 still pass
- [ ] run the per-task gate

### Task 11: Text input element

**Files:**
- Create: `app/src/input.rs`
- Modify: `app/src/main.rs`

The largest task in the plan. Read the **Text** and **Text input** facts above before writing a line;
all three requirements there are mandatory, and the UTF-16 rule is the one that fails silently.

- [ ] define `TextInput` per the Text input section, holding its text, a UTF-16 caret, the marked
      range, and its `FocusHandle`
- [ ] implement the eight required `EntityInputHandler` methods, with byte ↔ UTF-16 conversion at the
      boundary and nowhere else
- [ ] implement the manual `Element`: shape with `shape_text` at the given wrap width in `prepaint`,
      size to the wrapped line count within one to six lines, paint the text and the caret quad in
      `paint`, and register `window.handle_input(...)` there
- [ ] bind the keys: characters via the input handler; `backspace`, `left`, `right`, `enter`,
      `shift-enter` as actions under a `key_context`; Enter emits `Submit::Send`, Shift+Enter inserts
      a newline
- [ ] write `#[gpui::test]` tests with the input focused: `simulate_input("hello")` gives text
      `hello` and caret 5; `simulate_input("привет 🐢")` gives that text and a caret at its UTF-16
      length, not its byte length; backspace after the emoji removes the whole emoji; left then a
      typed character inserts before the caret; `shift-enter` leaves a `\n` in the text and emits
      nothing; `enter` emits `Submit::Send`; `clear` empties the text and resets the caret
- [ ] run the per-task gate

### Task 12: Composer

**Files:**
- Create: `app/src/composer.rs`
- Modify: `app/src/feed.rs`, `app/src/main.rs`

Open `docs/design/screenshots/01-full-mockup.png` before starting — the composer at the bottom of the
feed card.

- [ ] build the composer around a `TextInput` entity it owns: the placeholder naming the target
      (`Message #channel` or `Message <agent>`), the four inert icons, the inert `Talk` affordance,
      and the round accent send button, disabled while the input is blank
- [ ] subscribe to the input's `Submit::Send`: call `AppState::send` with the text, then `clear` the
      input; the send button does the same
- [ ] mount it at the bottom of the feed card; the feed already scrolls on `MessageAppended`
- [ ] write `#[gpui::test]` tests: with the composer's input focused, `simulate_input("hi")` then
      `simulate_keystrokes("enter")` appends a message with body `[Span::Text("hi")]` to the selected
      channel and leaves the input empty; `enter` on a blank input appends nothing;
      `simulate_input("a")`, `shift-enter`, `simulate_input("b")`, `enter` appends a body containing
      `a\nb`
- [ ] **user checkpoint**: stop and ask the user to type a message in `movie-night`, including a
      Shift+Enter line break and some Cyrillic, and confirm it appears and the feed scrolls to it
- [ ] run the per-task gate

### Task 13: Thread panel

**Files:**
- Create: `app/src/thread.rs`
- Modify: `app/src/feed.rs`, `app/src/message.rs`, `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` before starting — the right-hand card.

- [ ] build the panel as its own card: the `Thread` header with the `#channel · author` subtitle read
      from `OpenThread`, a close control, the cached root message, the replies, and a composer
      placeheld "Reply in thread" that owns its own `TextInput`
- [ ] show the panel only when `thread` is `Some`; wire the close control to `AppState::close_thread`
- [ ] wire the reply composer's `Submit::Send` through `AppState::reply_in_thread`, then clear it
- [ ] pass an `on_open` into the feed's message rows that calls `AppState::open_thread`
- [ ] write `#[gpui::test]` tests: drawing the panel with a thread open does not panic; opening a
      thread then selecting another channel and drawing still renders — the root comes from
      `OpenThread`, not from the selection; replying through the panel's input appends to the thread
      and raises the root's count
- [ ] **user checkpoint**: stop and ask the user to open a thread from `movie-night`, switch to
      another channel, confirm the thread stays, reply in it, and close it
- [ ] run the per-task gate

### Task 14: Agents view

**Files:**
- Create: `app/src/agents.rs`
- Modify: `app/src/shell.rs`, `app/src/main.rs`

Open `docs/design/screenshots/05-agents-and-settings.png` before starting; build the card list on the
left of that screenshot and **not** the settings panel on its right, which is a non-goal.

- [ ] extract a pure `agent_cards(agents: &[Agent]) -> Vec<AgentCard>` producing the ordered
      view-model — name, initials, role, status label — in `sort_index` order
- [ ] render one card per entry, and a summary line counting the agents and how many are busy
- [ ] show this view in place of the feed and thread when `view == Agents`; the sidebar row and the
      top-bar segment already call `set_view` from Tasks 7 and 8
- [ ] write tests: `agent_cards` over the seeded agents returns four entries in `sort_index` order
      with the right busy labels; drawing the agents view does not panic
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
      `mise exec` rule, and how to build, run and test through `make`
- [ ] write `CLAUDE.md`: the crate split and why `core` must never depend on `gpui`, the state
      ownership rule and the input-owns-its-buffer rule, the `ListState` resync rule, the two settled
      behaviours, and where the design lives
- [ ] run the per-task gate

### Task 17: Verify acceptance criteria

- [ ] `make fmt-check`, `make lint`, `make test`, `make build` — all clean
- [ ] confirm `core` has no `gpui` dependency: `! mise exec -- cargo tree -p tuclaw-core | grep -q gpui`
      exits 0
- [ ] confirm no comments were added: `grep -rn '^\s*//[^/!]' core/src app/src` prints nothing
- [ ] confirm no colour literal exists outside `app/src/theme.rs`
- [ ] confirm every non-goal is still absent — nothing from that list crept in
- [ ] state in this plan what could not be verified without the user running the app

### Task 18: [Final] Hand over

- [ ] list in this plan what the user should check by hand: scroll smoothness at 58 messages, typing
      latency, clicking each channel and direct, opening and closing a thread, Cyrillic and emoji in
      the composer, and whether the window still matches `docs/design/screenshots/01-full-mockup.png`
- [ ] move this plan to `docs/plans/completed/`

## Post-Completion

*No checkboxes — these need the user or a decision, not the agent.*

**Manual judgement.** Only the user can say whether it feels right: input latency, scrolling at 58
messages, and whether the chrome still reads as the mockup once things move.

**Decisions deferred out of v1.** Whether the app follows the system light/dark appearance; whether
the search field becomes real; whether the structured message cards from the mockup get built;
whether text selection and mouse caret placement are added to the input; whether the SwiftUI
implementation in `../tuclaw-client` is archived or deleted.

**Known risk carried forward.** GPUI is pre-1.0 and pinned to a git revision taken while its platform
crates were being split apart. Moving the pin later is a real task, not a version bump, and nothing in
v1 should assume the API is stable.
