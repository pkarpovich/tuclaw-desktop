# tuclaw-desktop v1

## Overview

A native macOS client for the tuclaw agent system, written in Rust on GPUI — Zed's GPU-accelerated UI
framework. It is the first step toward replacing Telegram as the interface to a set of AI agents.

This is a from-scratch build in a clean repository. A SwiftUI implementation of the same v1 exists in
`../tuclaw-client` and is **not** a reference to read or match: it was rejected because it reads as a
generic macOS app rather than the design, and because most of its build was spent fighting system
components. Everything this plan needs is written down here.

What v1 does: shows the channels and direct messages of a single local workspace, renders one
channel's conversation grouped by day, lets the user type and send a message, open a thread on any
message and reply in it, and lists the agents. Data lives in SQLite, seeded once from fixtures. There
is no network.

The load-bearing difference from a typical macOS app: **the window draws its own chrome**. The
titlebar is transparent, the traffic lights are positioned inside the app's own top bar, and the feed
and thread are rounded cards floating on a warm background. Nothing here is a system control.

### Non-goals

Out of scope for v1. Do not build these, and do not treat their absence as a defect:

- Networking of any kind. No daemon connection, no sync, no notifications.
- Voice: no push-to-talk, no recording, no transcription. The Talk button is drawn, not wired.
- Attachments, emoji picker, formatting controls. The composer sends plain text.
- Structured message cards — the film picker, the run log, the task progress card, the decision card.
  Messages are text with inline agent mentions and inline code, nothing else.
- Search behaviour. The search field is drawn as an affordance and searches nothing.
- The agent settings panel (Role / Permissions / How it replies / Voice replies / Disconnect).
- Editing, deleting or reacting to messages.
- Multi-window, preferences window, menu bar customisation beyond what GPUI gives by default.
- iOS, `gpui-mobile`, or any second platform.
- Accessibility work beyond what GPUI provides — no VoiceOver pass in v1.

### Rejected alternatives

- **SwiftUI.** Rejected after a full implementation: the system components (toolbar item capsules,
  `List` styling, a titlebar that swallows clicks in its own region) fought the design at every step.
- **Electron / Tauri / Flutter.** Rejected by the author: no JavaScript stack, and no non-native
  renderer pretending to be a Mac app.
- **`uniform_list` for the feed.** Rejected: it requires uniform row heights, and message rows wrap to
  different heights. `list()` with `ListState` is the variable-height equivalent.
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
are not — read it once before the first view task.

Every view task below names the screenshot to open first. Where this plan's prose and a screenshot
disagree about a detail the plan does not pin, follow the screenshot.

## Toolchain

- Rust **1.98.0**, pinned in `mise.toml`. The floor is 1.97: GPUI's main branch uses
  `std::hint::cold_path`, and anything earlier fails to compile `gpui` with `E0658` — 1.93 was tried
  and failed exactly that way. Zed's own `rust-toolchain.toml` pins 1.97.1, but 1.98.0 was verified
  against the pinned revision here and compiles the whole dependency tree clean, so v1 takes the newer
  one. If a future GPUI bump breaks under 1.98, dropping to 1.97.1 is the first thing to try.
- `gpui` and `gpui_platform` from git, rev `fecc3273ed32643c2ea1b04a74c8780e2c9ffaf8`.
  `gpui_platform` needs the `font-kit` feature on macOS; without it text lays out but renders no
  glyphs.
- Xcode's **Metal toolchain** must be installed — `gpui_apple` compiles `shaders.metal` in a build
  script. If missing, the build fails with "cannot execute tool 'metal'"; install with
  `xcodebuild -downloadComponent MetalToolchain` (~690 MB).
- `rusqlite` with the `bundled` feature, so no system SQLite is required.
- Build times to expect: ~1 minute clean, ~2-3 seconds incremental.

## Context (from discovery)

### GPUI facts established from the pinned source

Written down here so no task has to go reading Zed:

- **Lists.** `list(state: ListState, render_item)` is the variable-height virtualised list.
  `ListAlignment::Bottom` is documented as "scrolling from bottom to top, like a chat log", which is
  what makes a feed open at its newest row without a scroll call. `ListState::scroll_to_end()` exists.
  `uniform_list(...)` is the fixed-height sibling.
- **State.** `Context::notify()` marks an entity dirty and re-renders its observers.
  `Context::observe(...)` and `Context::subscribe(...)` are the cross-entity hooks.
- **Text input does not exist.** GPUI ships no text field. Input is assembled from a `FocusHandle`, a
  `key_context(...)`, `KeyBinding::new(...)` bound to actions, and the `EntityInputHandler` trait
  (`replace_text_in_range` and neighbours) which is what makes IME and the system caret work. GPUI's
  own `examples/input.rs` is 778 lines for a complete one.
- **Window chrome.** `WindowOptions::titlebar` takes `TitlebarOptions { title, appears_transparent,
  traffic_light_position }`. Transparent titlebar plus an explicit traffic-light position is how the
  app draws its own top bar.
- **Tests.** `#[gpui::test]` provides a `TestAppContext`. `VisualTestContext` adds `draw(...)`,
  `simulate_input(...)`, `simulate_keystrokes(...)` and `simulate_click(position, modifiers)`. All of
  it is behind the `test-support` feature and runs in-process — it does not open a window on the
  user's screen or take the cursor.

### Consequence for the test tier

`simulate_click` addresses a **point**, not an identifier. There is no accessibility-identifier
contract to write, and tests must not be built around guessing coordinates. The tier is therefore:
pure logic tested directly, state transitions tested through `AppState` methods, and only a small
number of view tests that render a view and assert on what the state became.

## Development Approach

- **Testing approach**: regular — implement, then test in the same task. Tests are a required
  deliverable of every task, not an afterthought.
- Complete each task fully before starting the next. All gates green before any `[x]`.
- After every task the app must build and run, and the window must render. A task that leaves a blank
  window is not done.
- **The agent never drives the running app.** It does not launch it in the foreground, click, or type
  into it. The user runs the app and reports back with screenshots. The `#[gpui::test]` tier is the
  agent's only automated view-level feedback.
- Update this plan as scope changes: `[x]` when done, `➕` for discovered work, `⚠️` for blockers.

## Code-Quality Rules (verify before marking each task complete)

From the `rust-style` skill. These are not suggestions:

- `for` loops with mutable accumulators, not iterator chains (`.iter().filter().map().collect()`).
- `let ... else` for early returns; the happy path stays unindented. `if let` only for a short action
  with no else branch.
- Shadow variables through transformations; no `raw_` / `parsed_` / `trimmed_` prefixes.
- **No comments.** No inline explanations, no section dividers, no TODOs, no commented-out code. Doc
  comments on public items of `tuclaw-core` are the only exception, per the `rustdoc` skill.
- Newtypes over bare `String` where the string carries meaning; enums over `bool` parameters.
- Never a wildcard `_ =>` match; match every variant. Avoid `matches!`.
- Destructure structs and tuples explicitly.

Per-task gate — all four green:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace` with no warnings, dead code included

## Testing Strategy

Two tiers, split by what they need to run.

**Tier 1 — `tuclaw-core`, plain `#[test]`.** The crate does not depend on `gpui`, so these run with no
window and no GPU. Covers: domain invariants, day grouping, the SQLite schema and every store method,
seeding idempotence, and the fixture data itself (counts, ordering, which channel is empty).

Store tests run against an in-memory SQLite database, one per test, so they are order-independent and
parallel-safe.

**Tier 2 — `tuclaw-desktop`, `#[gpui::test]`.** Needs `gpui` with `test-support`. Covers state
transitions through `AppState` methods and a handful of view tests that render through
`VisualTestContext::draw` and assert on resulting state. What it does NOT do: assert on pixels, walk a
view hierarchy looking for text, or click by guessed coordinates.

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

`app/` is `tuclaw-desktop`: the binary. One `AppState` entity owns all mutable state — the channel
list, the selected channel, that channel's messages, the open thread, and the composer draft. Views
hold `Entity<AppState>` and read through it. Every mutation is a method on `AppState` that ends in
`cx.notify()`; no view mutates another view's data, and no view talks to the store directly.

The window is chromeless: transparent titlebar, traffic lights positioned into the app's own top bar,
a warm background, and the feed and thread as rounded cards with a shadow.

Two behaviours are settled up front because they were decided the hard way already:

- **The thread is independent of the selected channel.** Switching channels leaves an open thread
  open, so `AppState` records the thread's own channel and the panel's subtitle reads from that.
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

`Span` is part of the domain, not the view: the mockup renders agent mentions and paths inline inside a
paragraph, and the store must round-trip them. Persist the body as text with a marker syntax and parse
on read; the parser is pure and tested in tier 1.

### Store (`core/src/store.rs`)

```rust
pub fn open(path: &Path) -> Result<Store>;
pub fn open_in_memory() -> Result<Store>;
pub fn channels(&self) -> Result<Vec<Channel>>;
pub fn agents(&self) -> Result<Vec<Agent>>;
pub fn messages(&self, channel: ChannelId) -> Result<Vec<Message>>;
pub fn thread(&self, root: MessageId) -> Result<Vec<Message>>;
pub fn send(&self, channel: ChannelId, body: &str) -> Result<Message>;
pub fn reply(&self, root: MessageId, channel: ChannelId, body: &str) -> Result<Message>;
pub fn seed_if_needed(&self, now: OffsetDateTime) -> Result<()>;
```

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

Database path: `~/Library/Application Support/tuclaw-desktop/tuclaw.sqlite`, resolved through one
function so tests can point elsewhere.

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

All timestamps derive from a `now` passed in, never from the wall clock, so "Today" and "Yesterday"
are correct whenever the app is first launched and so tests can state what they mean.

### State (`app/src/state.rs`)

```rust
pub struct AppState { /* store, agents, channels, selected, messages, thread, draft */ }
pub fn select(&mut self, channel: ChannelId, cx: &mut Context<Self>);
pub fn send(&mut self, cx: &mut Context<Self>);
pub fn open_thread(&mut self, root: MessageId, cx: &mut Context<Self>);
pub fn close_thread(&mut self, cx: &mut Context<Self>);
pub fn reply_in_thread(&mut self, cx: &mut Context<Self>);
pub fn set_draft(&mut self, draft: String, cx: &mut Context<Self>);
```

Rules these methods enforce, each of which gets a test:

- A blank or whitespace-only draft sends nothing and reaches no store call.
- A row appears in the feed only after the store accepted it; a failed write leaves the channel
  untouched and logs.
- `reply_in_thread` bumps the root's `reply_count` in the in-memory copy too, so the feed's affordance
  updates without a refetch.
- `select` does not close an open thread.

### Theme (`app/src/theme.rs`)

One module of named colour constants and nothing else — no `Color::rgb(...)` literals anywhere in the
view code. The palette is warm and light, taken from the mockup: a cream window, white cards, a
terracotta accent, three levels of text, one border and one hairline tone. v1 ships a single palette
and does not follow the system appearance; that is a deliberate simplification, not an oversight.

### Window

`WindowOptions` with `titlebar: Some(TitlebarOptions { title: None, appears_transparent: true,
traffic_light_position: Some(...) })`, default size 1280×820. The app's own top bar reserves space on
the left for the traffic lights and holds the sidebar toggle, back and forward, the
`Channel / Direct / Agents` segmented control, and `tuclaw · local` with a settings affordance on the
right.

## What Goes Where

- **Implementation Steps** — everything inside this repository.
- **Post-Completion** — running the app, judging how it feels, and anything needing a decision.

## Implementation Steps

### Task 1: Workspace skeleton and a green empty test run

**Files:**
- Create: `Cargo.toml`, `mise.toml`, `Makefile`, `.gitignore`, `rustfmt.toml`
- Create: `core/Cargo.toml`, `core/src/lib.rs`
- Create: `app/Cargo.toml`, `app/src/main.rs`

- [ ] create the workspace with members `core` and `app`; `core` must not list `gpui` as a dependency
- [ ] pin Rust 1.98.0 in `mise.toml` and add the two GPUI git dependencies with the rev from Toolchain
- [ ] add a `Makefile` with `build`, `run`, `test`, `lint`, `fmt` targets so every later task has one
      command per gate
- [ ] open an empty window from `app/src/main.rs` to prove the toolchain, Metal shaders and window
      creation all work
- [ ] write one placeholder test in each crate and confirm `cargo test --workspace` passes
- [ ] run the per-task gate

### Task 2: Domain types and the span parser

**Files:**
- Create: `core/src/model.rs`, `core/src/span.rs`
- Modify: `core/src/lib.rs`

- [ ] define every type from the Domain section, deriving `Debug`, `Clone`, `PartialEq` and, for the
      id newtypes, `Eq` and `Hash`
- [ ] implement the body encoding: a pure `parse(&str) -> Vec<Span>` and its inverse `render(&[Span]) -> String`
- [ ] choose a marker syntax that cannot collide with ordinary prose and state it in the module's doc
      comment; a message containing the marker characters literally must survive a round trip
- [ ] write tests: plain text with no markers, a mention mid-sentence, inline code, several spans in
      one body, an empty body, and a round trip through `render` then `parse`
- [ ] run the per-task gate

### Task 3: Day grouping

**Files:**
- Create: `core/src/grouping.rs`

- [ ] implement grouping of messages into day sections, oldest first, preserving input order inside a
      section; the section title is `Today`, `Yesterday`, or a formatted date
- [ ] take the calendar and `now` as parameters — never read the wall clock inside the function
- [ ] write tests: empty input, all messages today, today plus yesterday, a run spanning several
      older days, and two messages either side of local midnight landing in different sections
- [ ] run the per-task gate

### Task 4: SQLite store

**Files:**
- Create: `core/src/store.rs`, `core/src/schema.rs`

- [ ] implement `open`, `open_in_memory`, and schema creation on first open
- [ ] implement every read method with explicit ordering, including the `sent_at` then `id` tie-break
- [ ] implement `send` and `reply`; `reply` inserts and increments the root's `reply_count` in one
      transaction
- [ ] implement `seed_if_needed` writing the marker in the same transaction as the data
- [ ] write tests against in-memory databases: a message written is read back in its channel and not
      in another; a reply lands in the thread and not in the feed, and raises the root's count; a
      thread query on a message with no replies is empty; ordering holds when two messages share a
      timestamp
- [ ] write tests for seeding: seeding twice leaves one copy; a database carrying a marker but no rows
      is left alone
- [ ] run the per-task gate

### Task 5: Fixtures

**Files:**
- Create: `core/src/fixtures.rs`

- [ ] build the agents, channels and conversations described in the Fixtures section, all timestamps
      derived from the passed-in `now`
- [ ] give `movie-night` its 58 messages across 15 days with one thread root carrying 4 replies
- [ ] give every other channel except `personal` its own conversation across at least two days
- [ ] write tests: the exact channel and agent counts and order; `movie-night` holds 58 top-level
      messages; `personal` is empty and is the only empty channel; exactly one channel carries an
      unread count; the fixtures span 16 distinct days; a message seeded "yesterday" lands on the
      previous calendar day
- [ ] run the per-task gate

### Task 6: AppState

**Files:**
- Create: `app/src/state.rs`
- Modify: `app/src/main.rs`, `app/Cargo.toml`

- [ ] define `AppState` holding the store and the state listed in the State section
- [ ] implement every method from the State contract, each ending in `cx.notify()` when it changed
      something
- [ ] load channels, agents and the first channel's messages at construction
- [ ] write `#[gpui::test]` tests for each rule in the State section: blank draft sends nothing;
      sending appends only what the store accepted; selecting another channel leaves an open thread
      open; replying raises the root's in-memory `reply_count`
- [ ] run the per-task gate

### Task 7: App shell — window, titlebar and top bar

**Files:**
- Create: `app/src/shell.rs`, `app/src/theme.rs`
- Modify: `app/src/main.rs`

Open `docs/design/screenshots/01-full-mockup.png` before starting.

- [ ] define the theme constants; no colour literal may appear outside this module afterwards
- [ ] open the window with a transparent titlebar and the traffic lights positioned into the app's own
      bar, per the Window section
- [ ] build the top bar: left space for the traffic lights, sidebar toggle, back and forward, the
      segmented `Channel / Direct / Agents` control with the active segment raised, and
      `tuclaw · local` with a settings affordance on the right
- [ ] lay out the three regions — sidebar on the warm background, feed and thread as rounded cards
      with a border, a shadow and a gap between them
- [ ] write a `#[gpui::test]` that draws the shell and asserts it renders without panicking
- [ ] run the per-task gate

### Task 8: Sidebar

**Files:**
- Create: `app/src/sidebar.rs`

Open `docs/design/screenshots/02-sidebar.png` before starting.

- [ ] build the search affordance with its `⌘K` hint, the `Inbox` and `Agents` rows, the emoji-titled
      group sections, channel rows, the direct-message rows with initials chips and status dots, and
      the pinned footer with the user row and the settings gear
- [ ] render the trailing states the mockup shows: an unread badge, a plain dot, and the `2 running`
      pill
- [ ] make channel and direct rows clickable, calling `AppState::select`, with the selection highlight
      and a hover style
- [ ] write a `#[gpui::test]`: selecting a channel through the state changes the selected channel and
      loads that channel's messages; selecting the empty channel yields no messages
- [ ] run the per-task gate

### Task 9: Message row

**Files:**
- Create: `app/src/message.rs`

Open `docs/design/screenshots/03-feed-and-thread.png` before starting.

- [ ] build a row: initials chip, author name, the `AGENT` badge for agent authors, the timestamp, and
      the body
- [ ] render `Span::Mention` and `Span::Code` as inline chips inside the flowing paragraph, not as
      separate blocks
- [ ] render the "N replies" affordance when `reply_count > 0`, and a hover-revealed reply affordance
      when it is zero
- [ ] keep the row free of any identifier or channel knowledge — it is reused by the thread panel
- [ ] write tests for the pure parts: the reply label reads "1 reply" for one and "N replies"
      otherwise
- [ ] run the per-task gate

### Task 10: Feed card

**Files:**
- Create: `app/src/feed.rs`

- [ ] build the channel header: `#`, the channel name, the `N agents · N tasks running` line, and the
      trailing controls from the mockup
- [ ] render the conversation with `list()` and a `ListState` owned by the view, with
      `ListAlignment::Bottom`, so the feed opens on the newest message
- [ ] flatten day separators into the same item sequence as messages, so the virtualiser sees one list
- [ ] render the empty state for a channel with no messages, filling the card so the composer stays at
      the bottom
- [ ] build the status bar along the card's bottom edge
- [ ] write a `#[gpui::test]` drawing the feed for the seeded 58-message channel and for the empty
      channel without panicking
- [ ] run the per-task gate

### Task 11: Composer and text input

**Files:**
- Create: `app/src/composer.rs`, `app/src/input.rs`

This is the largest task in the plan, because GPUI ships no text field — see the GPUI facts section.
Build the input in `input.rs` as a reusable element: a `FocusHandle`, a `key_context`, `KeyBinding`s
for the keys v1 needs, and `EntityInputHandler` so IME and the system caret work. Selection and
mouse-driven caret placement are **not** required for v1; typing, backspace, arrow keys and the two
send bindings are.

- [ ] implement the input element with its focus handle, key context and input handler
- [ ] bind Enter to send and Shift+Enter to inserting a newline, and let the field grow to a few lines
- [ ] build the composer around it: placeholder naming the target, the icon row, the `Talk` affordance
      drawn but not wired, and the round accent send button, disabled while the draft is blank
- [ ] wire sending through `AppState::send` and scroll the feed to the end afterwards
- [ ] write `#[gpui::test]` tests using `simulate_input` and `simulate_keystrokes`: typing updates the
      draft; Enter sends and clears it; Shift+Enter leaves the draft holding a newline and sends
      nothing; Enter on a blank draft sends nothing
- [ ] run the per-task gate

### Task 12: Thread panel

**Files:**
- Create: `app/src/thread.rs`

- [ ] build the panel as its own card: the `Thread` header with the `#channel · author` subtitle and a
      close control, the root message, its replies, and a composer placeheld "Reply in thread"
- [ ] show the panel only when a thread is open; wire the close control to `AppState::close_thread`
- [ ] wire the reply composer through `AppState::reply_in_thread`
- [ ] make the feed's reply affordances open the thread
- [ ] write `#[gpui::test]` tests: opening a thread from a message sets the open thread and its
      channel; switching channels afterwards leaves it open; closing clears it; replying appends to
      the thread and raises the root's count
- [ ] run the per-task gate

### Task 13: Agents view

**Files:**
- Create: `app/src/agents.rs`

Open `docs/design/screenshots/05-agents-and-settings.png` before starting; build the card list on the
left of that screenshot and **not** the settings panel on its right, which is a non-goal.

- [ ] render one card per agent — initials chip, name, role, status — in `sort_index` order
- [ ] show a summary line counting the agents and how many are busy
- [ ] route the sidebar's `Agents` row and the top bar's `Agents` segment to this view
- [ ] write a `#[gpui::test]`: the agents route renders the seeded agents in order
- [ ] run the per-task gate

### Task 14: Startup, persistence and first run

**Files:**
- Modify: `app/src/main.rs`, `app/src/state.rs`, `core/src/store.rs`

- [ ] resolve the database path, create the directory if absent, open the store and seed it before the
      first render, so a clean first launch never shows an empty window
- [ ] handle an unopenable store by showing a plain failure view rather than panicking
- [ ] write tests: seeding runs on a fresh database and not on a second launch; a store that fails to
      open produces the failure state instead of a panic
- [ ] run the per-task gate

### Task 15: README and repository documentation

**Files:**
- Create: `README.md`, `CLAUDE.md`

- [ ] write the README: what the app is, the toolchain traps from the Toolchain section, how to build,
      run and test
- [ ] write `CLAUDE.md`: the crate split and why `core` must never depend on `gpui`, the state
      ownership rule, the two settled behaviours, and where the design lives
- [ ] run the per-task gate

### Task 16: Verify acceptance criteria

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `cargo build --workspace` — all clean
- [ ] confirm `core` has no `gpui` dependency: `cargo tree -p tuclaw-core | grep -c gpui` returns 0
- [ ] confirm no comments were added: no `//` line in either crate's `src` outside `///` doc comments
- [ ] confirm no colour literal exists outside `app/src/theme.rs`
- [ ] confirm every non-goal is still absent — nothing from that list crept in
- [ ] state in this plan what could not be verified without the user running the app

### Task 17: [Final] Hand over

- [ ] list in this plan what the user should check by hand: scroll smoothness at 58 messages, typing
      latency, whether the window still matches `docs/design/screenshots/01-full-mockup.png`
- [ ] move this plan to `docs/plans/completed/`

## Post-Completion

*No checkboxes — these need the user or a decision, not the agent.*

**Manual judgement.** Only the user can say whether it feels right: input latency, scrolling at 58
messages, and whether the chrome still reads as the mockup once things move.

**Decisions deferred out of v1.** Whether the app follows the system light/dark appearance; whether
the search field becomes real; whether the structured message cards from the mockup get built; whether
the SwiftUI implementation in `../tuclaw-client` is archived or deleted.

**Known risk carried forward.** GPUI is pre-1.0 and pinned to a git revision taken while its platform
crates were being split apart. Moving the pin later is a real task, not a version bump, and nothing in
v1 should assume the API is stable.
