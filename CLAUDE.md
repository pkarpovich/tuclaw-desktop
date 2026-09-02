# CLAUDE.md

Rules for working in this repository. They are load-bearing: each one was settled the hard way, and
breaking one is how this app regresses. Read `README.md` for what the app is and for the toolchain
traps (the `mise exec --` rule above all — never run a bare `cargo`).

## The crate split

Two crates in one workspace.

`core/` is `tuclaw-core`: domain types (`model.rs`), the SQLite store (`store.rs`, `schema.rs`,
`paths.rs`), the fixtures (`fixtures.rs`) and day grouping (`grouping.rs`).

`app/` is `tuclaw-desktop`: the binary — `state.rs`, the views (`shell.rs`, `sidebar.rs`, `feed.rs`,
`message.rs`, `thread.rs`, `agents.rs`, `failure.rs`), the text input (`input.rs`), the composer
(`composer.rs`) and the theme (`theme.rs`).

**`core` must never depend on `gpui`.** Not directly, not transitively. Two reasons: the domain and
the store have to be testable with no window and no GPU, and storage concerns must stay out of the
render path. `cargo tree -p tuclaw-core` must not mention `gpui`. If a core type seems to need a
`gpui` type, the conversion belongs on the app side.

`schema.rs` and `fixtures.rs` are private modules, which is what keeps `rusqlite` and the fixture
types out of `tuclaw-core`'s public API. Public items in `core` carry `///` docs (`rustdoc` skill);
`app` items do not.

**`failure.rs` is not only a view: it owns the startup path.** `start(now) -> Startup` resolves the
database path, creates the directory, opens the store, seeds it and builds `AppState`, returning
`Ready(Box<AppState>)` or `Failed(FailureView)`; `main` opens one window with either as its root, so
a store failure still gets a window. `start_at(path, now)` is the testable half — look there, not in
`main.rs`. `AppState::new` takes no `Context` because `cx.new` cannot return a `Result`, so the state
is built before `cx.new(|_| state)`. `Ready` boxes its payload or clippy's `large_enum_variant` fails
the `-D warnings` gate.

## State ownership

One `AppState` entity owns all mutable application state: the channel list, the selected channel and
its messages, the open thread with its root message, which view is showing, and the last channel
visited of each kind.

- Views hold `Entity<AppState>` and read through it. **No view mutates another view's data, and no
  view talks to the store directly.**
- Every mutation is a method on `AppState` that ends in `cx.notify()` and, where a listener has to do
  more than repaint, `cx.emit(...)`.
- `active_segment()` is derived, not stored. Anything derivable stays derived — the feed header's
  `N agents`, the status bar's `N of M agents busy`, the sidebar's sections.

**The one thing `AppState` does not own is text being typed.** Each composer creates a `TextInput`
entity that owns its own buffer, because `EntityInputHandler` requires the element to hold the string
and because two composers on screen at once would fight over one shared field. The composer hands the
body to `AppState` on submit and clears the input **only if the state reports `Ok`** — a failed write
leaves the text in place so the user can retry. The placeholder names the *selected* channel, so it
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
| `MessageAppended` | feed | rebuild, `reset(count)`, then `scroll_to_end()` |
| `ReplyAppended` | feed, thread panel | rebuild items, repaint only — no reset |
| `ThreadOpened` | thread panel | focus its composer's input |
| `ThreadClosed` | feed | focus its composer's input |

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
3. Focus moves are explicit: the window focuses the feed composer when it opens, `ThreadOpened` moves
   focus to the thread composer, `ThreadClosed` returns it to the feed's. Both views take focus
   through a `Focus { Requested, Taken }` field consumed during render, because a focus call needs a
   `&mut Window` that a subscription callback does not have.

**Every range crossing `EntityInputHandler` is in UTF-16 code units, not byte offsets.** The element
keeps a `String` and converts at the boundary. ASCII-only tests cannot catch a byte-offset bug, so
input tests use Cyrillic and an emoji.

Multi-line text goes through `shape_text`, never `shape_line` — the latter carries
`debug_assert!(text.find('\n').is_none())` and debug asserts are live under `cargo run` and `cargo
test`.

## Two settled behaviours

- **The thread is independent of the selected channel.** Switching channels leaves an open thread
  open, so `AppState` caches the thread's root message and its channel; the panel never reads them
  from the current selection.
- **Enter sends, Shift+Enter breaks the line.** Both are explicit `KeyBinding`s registered in
  `input::bind_keys`, not defaults.

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

`core` is plain `#[test]` against in-memory SQLite, one database per test. `app` is `#[gpui::test]`
with `TestAppContext` / `VisualTestContext`.

Click paths are testable in-process: `InteractiveElement::debug_selector(|| "name".into())` on a
`div()` records its laid-out bounds under `test-support`, `VisualTestContext::debug_bounds("name")`
returns them, and `simulate_click(bounds.center(), Modifiers::default())` clicks it. The convention is
`"<view>-<thing>-<key>"` — `sidebar-row-movie-night`, `segment-agents`, `message-reply-<id>`,
`thread-close`. Selectors are test-only strings and never appear in rendered output. `TextInput::new`
takes its selector as a constructor argument, because it has to land on the same `div()` that owns
`track_focus`, and two inputs (`input-feed`, `input-thread`) are on screen at once.

**Any `#[gpui::test]` that simulates keys calls `cx.update(input::bind_keys)` before it builds its
harness.** The bindings live under the `TuclawInput` key context and are registered per `App`;
without them `enter` and `shift-enter` arrive as a literal newline and the test fails as if the logic
were wrong. Test-only accessors that reach across module privacy — `Feed::input_focus`,
`ThreadPanel::input_focus` — are `#[cfg(test)]`-gated, because an accessor with no caller in the bin
target is dead code under the `-D warnings` gate.

A draw test proves only that rendering did not panic. It says nothing about what was drawn, so it is
never the only test for a behaviour.

All four gates green before any change is done: `make fmt-check`, `make lint`, `make test`,
`make build`.
