# tuclaw-desktop

A native macOS client for the tuclaw agent system, written in Rust on
[GPUI](https://github.com/zed-industries/zed) — Zed's GPU-accelerated UI framework. It is the first
step toward replacing Telegram as the interface to a set of AI agents.

v1 shows the channels and direct messages of a single local workspace, renders one conversation
grouped by day, lets you type and send a message, open a thread on any message and reply in it, and
lists the agents. Data lives in SQLite, seeded once from fixtures on first launch. There is no
network: no daemon connection, no sync, no notifications.

The window draws its own chrome. The titlebar is transparent, the traffic lights are positioned
inside the app's own top bar, and the feed and thread are rounded cards floating on a warm
background. Nothing on screen is a system control.

Enter sends; Shift+Enter breaks the line. Clicking the composer focuses it, and it grows with its
text — there is no selection, no mouse caret placement and no height cap. Several controls are drawn
and inert on purpose: the search field and its `⌘K` hint, the sidebar toggle, the back and forward
arrows, the feed header's two trailing chips, and the composer's icon row and `Talk` chip. Times are
shown in UTC, not in the local zone.

## Layout

| Path | What it is |
|---|---|
| `core/` | `tuclaw-core`: domain types, the SQLite store, the fixtures, day grouping. No `gpui` dependency, so it is testable without a window. |
| `app/` | `tuclaw-desktop`: the binary — state, views, the text input element, the theme. |
| `docs/design/` | The designer's mockup and five screenshots of it. Look here before touching a view. |
| `docs/plans/completed/` | The implementation plan this repository was built from, archived complete. |

## Building

Prerequisites: macOS, [mise](https://mise.jdx.dev) — the Rust pin is enforced through it, see
Toolchain traps — and Xcode's Metal toolchain. Run `mise install` in the repository root to fetch the
version named in `mise.toml`.

Everything runs through mise tasks, defined in `mise.toml`. Read the Toolchain traps below before
running anything else.

```
mise run build       # cargo build --workspace --all-targets
mise run dev         # cargo run -p tuclaw-desktop
mise run test        # cargo test --workspace
mise run lint        # cargo clippy --workspace --all-targets -- -D warnings
mise run fmt         # cargo fmt --all
mise run fmt-check   # cargo fmt --all -- --check
```

The four gates that must be green before any change lands: `mise run fmt-check`, `mise run lint`,
`mise run test`, `mise run build`. A clean build takes about a minute; incremental builds are a few
seconds.

## Toolchain traps

Four things about this project's toolchain are non-obvious, and each one has cost a build.

**Rust 1.98.1, pinned in `mise.toml`.** The floor is 1.97: GPUI's main branch uses
`std::hint::cold_path`, and anything earlier fails to compile `gpui` with `E0658`. 1.98.1 (zed's own `rust-toolchain.toml`) is verified
against the pinned revision and compiles the whole dependency tree clean.

**Never run a bare `cargo`.** The pin does not reach it. On the author's machine `which cargo`
resolves to an older install placed ahead of mise's shims by the global mise config, and a
non-interactive session runs no directory hook, so `mise.toml` alone changes nothing. A mise task
runs inside the environment `mise.toml` declares, which is what makes the pin effective. Use
`mise run`; if you must call cargo directly, spell it `mise exec -- cargo ...`. Confirm the pin with:

```
mise exec -- rustc --version
```

**The entry point lives in `gpui_platform`, not `gpui`.** At the pinned revision `gpui::Application`
has no `new()`. The app starts with `gpui_platform::application().run(|cx: &mut App| { ... })`, opens
its window with `cx.open_window(options, |_, cx| cx.new(...))` and calls `cx.activate(true)`. Every
published GPUI example starts with `Application::new()`, which does not compile here.

**Xcode's Metal toolchain must be installed.** `gpui_apple` compiles `shaders.metal` in a build
script. Without the toolchain the build fails with `cannot execute tool 'metal'`. Install it with:

```
xcodebuild -downloadComponent MetalToolchain
```

It is about 690 MB.

## Dependencies

GPUI is pinned to a git revision, not a crates.io version — only a stale `gpui` core is published and
`gpui_platform` is not published at all, so the pin is mandatory:

```toml
gpui = { git = "https://github.com/zed-industries/zed", rev = "a84689073d296dfd39987bc7dd478e43ef76d83a" }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "a84689073d296dfd39987bc7dd478e43ef76d83a", features = ["font-kit"] }
```

Without the `font-kit` feature text lays out but renders no glyphs. `gpui` appears again under
`[dev-dependencies]` with `features = ["test-support"]`, which is what `#[gpui::test]` needs.

`rusqlite` carries `bundled` and `time`: `time` is what makes `OffsetDateTime` bind and read back,
and `bundled` alone does not.

GPUI is pre-1.0 and the pin was taken while its platform crates were being split apart. Moving the
pin is a real task, not a version bump.

## Data

The database lives at `~/Library/Application Support/tuclaw-desktop/tuclaw.sqlite`. It is created and
seeded from the fixtures on first launch; the seed marker is written in the same transaction as the
fixtures, so an interrupted first run leaves either everything or nothing. Delete the file to get a
fresh workspace on the next launch.

If the store cannot be opened, the window still opens and shows a failure view naming the path and
the error.

## Tests

Two tiers, split by what they need to run.

`tuclaw-core` uses plain `#[test]` — no window, no GPU. It covers domain invariants, the body
encoding round trip, day grouping, the schema and every store method, seeding idempotence, and the
fixture data itself. Store tests run against a fresh in-memory database each, so they are
order-independent and parallel-safe.

`tuclaw-desktop` uses `#[gpui::test]`, which needs `gpui` with `test-support`. It covers state
transitions through `AppState` methods, the input element through `simulate_input` and
`simulate_keystrokes`, click paths through `debug_selector` + `debug_bounds` + `simulate_click`, pure
view-model functions, and one draw test per view.

Both tiers run under `mise run test`.
